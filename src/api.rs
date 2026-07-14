use axum::http::StatusCode;

pub async fn feed() -> StatusCode {
    crate::feed::feed().await;
    StatusCode::NO_CONTENT
}
