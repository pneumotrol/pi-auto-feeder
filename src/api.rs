use axum::{http::StatusCode, response::Redirect};

pub async fn feed() -> StatusCode {
    crate::feed::feed().await;
    StatusCode::NO_CONTENT
}

pub async fn feed_from_web() -> Redirect {
    crate::feed::feed().await;
    Redirect::to("/")
}
