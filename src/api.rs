use crate::feed::Feeder;
use axum::{extract::State, http::StatusCode, response::Redirect};

pub async fn feed(State(feeder): State<Feeder>) -> Result<StatusCode, StatusCode> {
    perform_feed(&feeder).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn feed_from_web(State(feeder): State<Feeder>) -> Result<Redirect, StatusCode> {
    perform_feed(&feeder).await?;
    Ok(Redirect::to("/"))
}

async fn perform_feed(feeder: &Feeder) -> Result<(), StatusCode> {
    feeder.feed().await.map_err(|error| {
        eprintln!("Failed to feed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
