//! Topcoat サーバとバックグラウンドスケジューラを起動するバイナリ。

use pi_auto_feeder::{
    feed::{FeedService, Feeder},
    schedule::{self, ScheduleStore},
};
use std::{env, net::SocketAddr};
use tokio_util::sync::CancellationToken;
use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    router::{Router, RouterBuilderDiscoverExt},
    session::{OriginLayer, SessionConfig},
};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let feeder = Feeder::from_env()?;
    let camera = pi_auto_feeder::camera::Camera::from_env()?;
    let schedules = ScheduleStore::from_env().await?;
    println!("Feeder mode: {}", feeder.mode());
    println!("Camera mode: {}", camera.mode());

    let feed_service = FeedService::new(feeder, schedules.clone());
    let cancellation = CancellationToken::new();
    let scheduler = schedule::start_scheduler(
        schedules.clone(),
        feed_service.clone(),
        cancellation.clone(),
    );

    let router = Router::builder()
        .discover()
        .layer(OriginLayer::new())
        .assets(AssetBundle::load()?)
        .app_context(SessionConfig::default())
        .app_context(schedules)
        .app_context(feed_service)
        .app_context(camera)
        .build();
    let address = listen_address()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Listening on http://{address}");

    let server_result = topcoat::serve(listener, router).await;
    cancellation.cancel();
    scheduler.await?;
    server_result?;
    Ok(())
}

/// 既存配置の `LEPTOS_SITE_ADDR` を移行期間中も受け入れ、Topcoat の `HOST` / `PORT`
/// と同じループバック既定値を使う。
fn listen_address() -> color_eyre::Result<SocketAddr> {
    if let Ok(address) = env::var("LEPTOS_SITE_ADDR") {
        return Ok(address.parse()?);
    }
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_owned());
    Ok(format!("{host}:{port}").parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_listen_address_is_loopback() {
        if env::var_os("LEPTOS_SITE_ADDR").is_none()
            && env::var_os("HOST").is_none()
            && env::var_os("PORT").is_none()
        {
            assert_eq!(listen_address().unwrap(), "127.0.0.1:3000".parse().unwrap());
        }
    }
}
