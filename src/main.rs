mod api;
mod camera;
mod feed;
mod schedule;

use crate::{
    camera::Camera,
    feed::{FeedService, Feeder},
    schedule::ScheduleStore,
};
use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let feeder = Feeder::from_env()?;
    let camera = Camera::from_env()?;
    let schedules = ScheduleStore::from_env().await?;
    println!("Feeder mode: {}", feeder.mode());
    println!("Camera mode: {}", camera.mode());

    let feed_service = FeedService::new(feeder, schedules.clone());
    schedule::start_scheduler(schedules.clone(), feed_service.clone());
    let app = Router::new()
        .merge(api::router(feed_service, schedules))
        .merge(camera::router(camera))
        .nest_service("/assets", ServeDir::new("dist"));
    let address = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(address).await?;

    println!("Listening on http://{address}");
    axum::serve(listener, app).await?;

    Ok(())
}
