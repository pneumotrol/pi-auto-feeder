//! SSR サーバの構築、バックグラウンド処理の起動、正常終了を担当するバイナリ。

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    use axum::{Router, middleware};
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use pi_auto_feeder::{
        app::{App, shell},
        camera::{self, Camera},
        events,
        feed::{FeedService, Feeder},
        schedule::{self, ScheduleStore},
    };
    use tokio_util::sync::CancellationToken;

    color_eyre::install()?;
    let feeder = Feeder::from_env()?;
    let camera = Camera::from_env()?;
    let schedules = ScheduleStore::from_env().await?;
    println!("Feeder mode: {}", feeder.mode());
    println!("Camera mode: {}", camera.mode());

    // 手動給餌とスケジュール給餌で同じサービスを共有し、排他制御を一か所に集約する。
    let feed_service = FeedService::new(feeder, schedules.clone());
    let cancellation = CancellationToken::new();
    let scheduler = schedule::start_scheduler(
        schedules.clone(),
        feed_service.clone(),
        cancellation.clone(),
    );

    let configuration = get_configuration(None)?;
    let address = configuration.leptos_options.site_addr;
    let leptos_options = configuration.leptos_options;
    let routes = generate_route_list(App);
    let context_store = schedules.clone();
    let context_feed_service = feed_service.clone();

    // 専用ストリームのルートを先に結合し、残りを Leptos の SSR ルートへ委譲する。
    let app = Router::<LeptosOptions>::new()
        .merge(events::router(schedules))
        .merge(camera::router(camera))
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || {
                provide_context(context_store.clone());
                provide_context(context_feed_service.clone());
            },
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        .layer(middleware::from_fn(require_same_origin))
        .with_state(leptos_options);

    println!("Listening on http://{address}");
    let listener = tokio::net::TcpListener::bind(address).await?;
    let server_result = axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(cancellation.clone()))
        .await;
    // サーバ停止と同時にスケジューラを止め、タスクの終了を待ってからプロセスを抜ける。
    cancellation.cancel();
    scheduler.await?;
    server_result?;
    Ok(())
}

#[cfg(feature = "ssr")]
/// Ctrl+C またはサービスマネージャからの SIGTERM を待ち、全タスクへ停止を通知する。
async fn shutdown_signal(cancellation: tokio_util::sync::CancellationToken) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let Ok(mut terminate) = signal(SignalKind::terminate()) else {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("Failed to listen for a shutdown signal: {error}");
            }
            cancellation.cancel();
            return;
        };
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(error) = result {
                    eprintln!("Failed to listen for Ctrl+C: {error}");
                }
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("Failed to listen for Ctrl+C: {error}");
    }

    cancellation.cancel();
}

#[cfg(feature = "ssr")]
/// 状態変更リクエストを同一オリジンに限定するための Axum ミドルウェア。
async fn require_same_origin(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::{
        http::{Method, StatusCode},
        response::IntoResponse,
    };

    let is_safe_method = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !is_safe_method && !has_same_origin(request.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

#[cfg(feature = "ssr")]
/// `Origin` の authority とリバースプロキシから渡る `Host` が一致するかを調べる。
///
/// 通常の HTML フォーム送信では `Origin` が付かない場合があるため、その場合は許可する。
fn has_same_origin(headers: &axum::http::HeaderMap) -> bool {
    use axum::http::header;

    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let (Ok(origin), Some(host)) = (
        origin.to_str(),
        headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok()),
    ) else {
        return false;
    };
    origin
        .split_once("://")
        .is_some_and(|(_, authority)| authority.trim_end_matches('/') == host)
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, header};

    #[test]
    fn accepts_same_origin_and_requests_without_origin() {
        let mut headers = HeaderMap::new();
        assert!(has_same_origin(&headers));

        headers.insert(
            header::HOST,
            HeaderValue::from_static("feeder.example:3000"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://feeder.example:3000"),
        );
        assert!(has_same_origin(&headers));
    }

    #[test]
    fn rejects_cross_origin_and_malformed_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("feeder.example"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        assert!(!has_same_origin(&headers));

        headers.insert(header::ORIGIN, HeaderValue::from_static("null"));
        assert!(!has_same_origin(&headers));
    }
}

#[cfg(not(feature = "ssr"))]
/// Wasm ライブラリのビルド時にバイナリ側へサーバ依存を持ち込まないための空エントリ。
pub fn main() {}
