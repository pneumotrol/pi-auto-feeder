mod api;
mod feed;

use axum::{
    Router,
    response::Html,
    routing::{get, post},
};
use leptos::prelude::*;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let app = Router::new()
        .route("/", get(index))
        .route("/api/feed", post(api::feed));
    let address = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("Listening on http://{address}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<String> {
    Html(format!(
        "<!doctype html>{}",
        view! {
            <html lang="ja">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Pi Auto Feeder"</title>
                </head>
                <body>
                    <main>
                        <h1>"Pi Auto Feeder"</h1>
                    </main>
                </body>
            </html>
        }
        .to_html()
    ))
}
