use axum::{http::StatusCode, response::Redirect};

pub async fn feed() -> Result<StatusCode, StatusCode> {
    perform_feed().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn feed_from_web() -> Result<Redirect, StatusCode> {
    perform_feed().await?;
    Ok(Redirect::to("/"))
}

async fn perform_feed() -> Result<(), StatusCode> {
    crate::feed::feed().await.map_err(|error| {
        eprintln!("Failed to feed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
