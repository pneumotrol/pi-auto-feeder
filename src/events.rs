//! サーバ状態の変更をブラウザへ知らせる Server-Sent Events エンドポイント。
//!
//! イベント本体には状態を含めず、受信側が server function から最新スナップショットを
//! 再取得することで、サーバを唯一の正とする。

use crate::schedule::ScheduleStore;
use axum::{
    Router,
    extract::State,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
    routing::get,
};
use futures_util::stream;
use leptos::prelude::LeptosOptions;

/// 状態変更通知を配信する `/events` ルートを構築する。
pub fn router(store: ScheduleStore) -> Router<LeptosOptions> {
    Router::new()
        .route("/events", get(events))
        .with_state(store)
}

async fn events(
    State(store): State<ScheduleStore>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = store.subscribe();
    let changes = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            // 通知を取りこぼしても最新状態を一度取得すればよいため、Lagged も変更扱いにする。
            Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
        }
        Some((Ok(Event::default().data("changed")), receiver))
    });
    Sse::new(changes).keep_alive(KeepAlive::default())
}
