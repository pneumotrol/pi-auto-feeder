use crate::schedule::ScheduleStore;
use axum::{
    Router,
    extract::State,
    response::{Sse, sse::Event},
    routing::get,
};
use futures_util::stream;
use leptos::prelude::LeptosOptions;

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
        tokio::select! {
            result = receiver.recv() => {
                match result {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
            () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
        }
        Some((Ok(Event::default().data("changed")), receiver))
    });
    Sse::new(changes)
}
