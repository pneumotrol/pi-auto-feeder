mod api;
mod camera;
mod feed;

use crate::{camera::Camera, feed::Feeder};
use axum::{
    Router,
    response::Html,
    routing::{get, post},
};
use leptos::prelude::*;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let feeder = Feeder::from_env()?;
    let camera = Camera::from_env()?;
    println!("Feeder mode: {}", feeder.mode());
    println!("Camera mode: {}", camera.mode());

    let app = Router::new()
        .route("/", get(index))
        .merge(
            Router::new()
                .route("/feed", post(api::feed_from_web))
                .route("/api/feed", post(api::feed))
                .with_state(feeder),
        )
        .merge(
            Router::new()
                .route("/camera/stream", get(camera::stream))
                .with_state(camera),
        );
    let address = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(address).await?;

    println!("Listening on http://{address}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<String> {
    Html(format!("<!doctype html>{}", view! { <App /> }.to_html()))
}

#[component]
fn App() -> impl IntoView {
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
                    <section>
                        <h2>"カメラ"</h2>
                        <img src="/camera/stream" alt="給餌器のカメラ映像" />
                    </section>
                    <form method="post" action="/feed">
                        <button type="submit">"給餌する"</button>
                    </form>
                </main>
            </body>
        </html>
    }
}
