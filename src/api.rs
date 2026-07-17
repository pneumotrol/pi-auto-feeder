use crate::{
    feed::{FeedOutcome, FeedService},
    schedule::{ScheduleStore, Settings},
};
use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, Redirect, Sse, sse::Event},
    routing::{get, post},
};
use futures_util::stream;
use leptos::prelude::*;
use pi_auto_feeder::{App, InitialState, NewSchedule, ScheduleView, SettingsPage, SettingsView};

#[derive(Clone)]
struct FeedState {
    feeder: FeedService,
}

pub fn router(feeder: FeedService, store: ScheduleStore) -> Router {
    let web_routes = Router::new()
        .route("/", get(index))
        .route("/schedules", post(add_schedule_from_form))
        .route("/api/schedules", post(add_schedule_from_json))
        .route("/schedules/{id}/delete", post(delete_schedule_from_form))
        .route("/api/schedules/{id}/delete", post(delete_schedule_from_api))
        .route("/api/state", get(state_from_api))
        .route("/api/events", get(events))
        .route("/settings", get(settings_page).post(update_settings))
        .with_state(store);
    let feed_routes = Router::new()
        .route("/feed", post(feed_from_form))
        .route("/api/feed", post(feed_from_api))
        .with_state(FeedState { feeder });
    web_routes.merge(feed_routes)
}

async fn index(State(store): State<ScheduleStore>) -> Result<Html<String>, StatusCode> {
    let initial_state = app_state(&store).await?;
    let app = view! {
        <App
            schedules=initial_state.schedules.clone()
            current_server_time=initial_state.current_server_time.clone()
            last_feed_time=initial_state.last_feed_time.clone()
            cooldown_remaining=initial_state.cooldown_remaining
            feed_history=initial_state.feed_history.clone()
        />
    }
    .to_html();
    let state_json = serde_json::to_string(&initial_state).map_err(internal_error)?;
    let head = format!(
        r#"<script id="initial-state" type="application/json">{state_json}</script>
<script type="module">import init from "/assets/pi-auto-feeder.js"; await init();</script>"#
    );
    Ok(Html(document("Pi Auto Feeder", &app, &head)))
}

async fn app_state(store: &ScheduleStore) -> Result<InitialState, StatusCode> {
    let status = store.status().await.map_err(internal_error)?;
    let schedules = store
        .list()
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(schedule_view)
        .collect();
    Ok(InitialState {
        schedules,
        current_server_time: status.current_time,
        last_feed_time: status.last_feed_time,
        cooldown_remaining: store.cooldown_remaining().await.map_err(internal_error)?,
        feed_history: store.recent_feed_history().await.map_err(internal_error)?,
    })
}

async fn state_from_api(
    State(store): State<ScheduleStore>,
) -> Result<Json<InitialState>, StatusCode> {
    app_state(&store).await.map(Json)
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

async fn feed_from_api(State(state): State<FeedState>) -> Result<String, (StatusCode, String)> {
    match state.feeder.feed().await.map_err(|error| {
        eprintln!("Failed to feed: {error}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "給餌に失敗しました".to_owned(),
        )
    })? {
        FeedOutcome::Fed(fed_at) => Ok(fed_at),
        FeedOutcome::Cooldown(remaining) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!("クールタイム中です．残り {remaining} 秒"),
        )),
    }
}

async fn feed_from_form(State(state): State<FeedState>) -> Result<Redirect, StatusCode> {
    match state.feeder.feed().await.map_err(internal_error)? {
        FeedOutcome::Fed(_) => Ok(Redirect::to("/")),
        FeedOutcome::Cooldown(_) => Err(StatusCode::TOO_MANY_REQUESTS),
    }
}

async fn add_schedule_from_form(
    State(store): State<ScheduleStore>,
    Form(request): Form<NewSchedule>,
) -> Result<Redirect, StatusCode> {
    add_schedule(&store, request).await?;
    Ok(Redirect::to("/"))
}

async fn add_schedule_from_json(
    State(store): State<ScheduleStore>,
    Json(request): Json<NewSchedule>,
) -> Result<Json<ScheduleView>, StatusCode> {
    add_schedule(&store, request).await.map(Json)
}

async fn add_schedule(
    store: &ScheduleStore,
    request: NewSchedule,
) -> Result<ScheduleView, StatusCode> {
    store
        .add(&request.scheduled_at)
        .await
        .map(schedule_view)
        .map_err(|error| {
            eprintln!("Failed to add schedule: {error}");
            StatusCode::BAD_REQUEST
        })
}

async fn delete_schedule_from_form(
    State(store): State<ScheduleStore>,
    Path(id): Path<i64>,
) -> Result<Redirect, StatusCode> {
    delete_schedule(&store, id).await?;
    Ok(Redirect::to("/"))
}

async fn delete_schedule_from_api(
    State(store): State<ScheduleStore>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    delete_schedule(&store, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_schedule(store: &ScheduleStore, id: i64) -> Result<(), StatusCode> {
    store.delete(id).await.map_err(internal_error)
}

async fn settings_page(State(store): State<ScheduleStore>) -> Result<Html<String>, StatusCode> {
    let settings = store.settings().await.map_err(internal_error)?;
    let page = view! {
        <SettingsPage settings=SettingsView {
            cooldown_seconds: settings.cooldown_seconds,
            feed_duration_ms: settings.feed_duration_ms,
        } />
    }
    .to_html();
    Ok(Html(document("設定", &page, "")))
}

async fn update_settings(
    State(store): State<ScheduleStore>,
    Form(settings): Form<SettingsView>,
) -> Result<Redirect, StatusCode> {
    store
        .update_settings(&Settings {
            cooldown_seconds: settings.cooldown_seconds,
            feed_duration_ms: settings.feed_duration_ms,
        })
        .await
        .map_err(|error| {
            eprintln!("Failed to update settings: {error}");
            StatusCode::BAD_REQUEST
        })?;
    Ok(Redirect::to("/settings"))
}

fn schedule_view(schedule: crate::schedule::Schedule) -> ScheduleView {
    ScheduleView {
        id: schedule.id,
        scheduled_at: schedule.scheduled_at,
        legacy_time: schedule.legacy_time,
        missed: schedule.missed,
        failure_reason: schedule.failure_reason,
    }
}

fn document(title: &str, body: &str, head: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>{head}</head><body>{body}</body></html>"
    )
}

fn internal_error(error: impl std::fmt::Display) -> StatusCode {
    eprintln!("Internal error: {error}");
    StatusCode::INTERNAL_SERVER_ERROR
}
