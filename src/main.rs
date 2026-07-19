#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    use axum::Router;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use pi_auto_feeder::{
        api,
        app::{App, shell},
        camera::{self, Camera},
        feed::{FeedService, Feeder},
        schedule::{self, ScheduleStore},
    };

    color_eyre::install()?;
    let feeder = Feeder::from_env()?;
    let camera = Camera::from_env()?;
    let schedules = ScheduleStore::from_env().await?;
    println!("Feeder mode: {}", feeder.mode());
    println!("Camera mode: {}", camera.mode());

    let feed_service = FeedService::new(feeder, schedules.clone());
    schedule::start_scheduler(schedules.clone(), feed_service.clone());

    let configuration = get_configuration(None)?;
    let address = configuration.leptos_options.site_addr;
    let leptos_options = configuration.leptos_options;
    let routes = generate_route_list(App);
    let context_store = schedules.clone();

    let app = Router::<LeptosOptions>::new()
        .merge(api::router(feed_service, schedules))
        .merge(camera::router(camera))
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || provide_context(context_store.clone()),
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    println!("Listening on http://{address}");
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[cfg(not(feature = "ssr"))]
pub fn main() {}
