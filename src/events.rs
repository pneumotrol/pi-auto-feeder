//! 永続状態の変更を通知する Topcoat Server-Sent Events エンドポイント。

use crate::schedule::ScheduleStore;
use futures_util::stream;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        content::sse::{Event, KeepAlive, Sse},
        route,
    },
};

#[route(GET "/events")]
async fn events(cx: &Cx) -> Result<Sse<impl futures_util::Stream<Item = Result<Event>> + use<>>> {
    let receiver = app_context::<ScheduleStore>(cx).subscribe();
    let changes = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
        }
        Some((Ok(Event::new().data("changed")), receiver))
    });
    Ok(Sse::new(changes).keep_alive(KeepAlive::new()))
}
