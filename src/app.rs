//! Topcoat のページ、レイアウト、フォーム処理を提供する Web UI。

use crate::{
    feed::{FeedOutcome, FeedService},
    schedule::{
        MAX_COOLDOWN_SECONDS, MAX_FEED_DURATION_MS, MIN_FEED_DURATION_MS, Schedule, ScheduleStore,
        Settings,
    },
};
use serde::Deserialize;
use std::fmt::Display;
use topcoat::{
    Error, Result,
    asset::{Asset, asset},
    context::{Cx, app_context},
    htmx::{HxLocation, SwapOption, hx_request},
    router::{
        IntoResponse, Response, content::Form, error::see_other, layout, page, query_params, route,
    },
    view::{component, view},
};

const STYLESHEET: Asset = asset!("./app/style.css");
const CLIENT_SCRIPT: Asset = asset!("./app/client.js");
const HTMX: Asset = asset!("https://cdn.jsdelivr.net/npm/htmx.org@2.0.10/dist/htmx.min.js");

#[layout("/")]
async fn document(cx: &Cx, slot: Result) -> Result {
    if hx_request(cx) {
        return slot;
    }

    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Pi Auto Feeder"</title>
                <link rel="stylesheet" href=(STYLESHEET)>
                <script src=(HTMX) defer="defer"></script>
                <script src=(CLIENT_SCRIPT) defer="defer"></script>
                topcoat::dev::script()
            </head>
            <body><div id="app">(slot?)</div></body>
        </html>
    }
}

#[topcoat::router::query_params(error = bad_request)]
struct PageQuery {
    notice: Option<String>,
    remaining: Option<u64>,
}

#[page("/")]
async fn home(cx: &Cx) -> Result {
    let query = query_params::<PageQuery>(cx)?;
    let store = store(cx);
    let status = store
        .status()
        .await
        .map_err(|error| internal_error("Failed to load server status", error))?;
    let schedules = store
        .list()
        .await
        .map_err(|error| internal_error("Failed to load schedules", error))?;
    let cooldown_remaining = store
        .cooldown_remaining()
        .await
        .map_err(|error| internal_error("Failed to load cooldown", error))?;
    let feed_history = store
        .recent_feed_history()
        .await
        .map_err(|error| internal_error("Failed to load feed history", error))?;
    let schedule_min = status
        .current_time
        .get(..16)
        .unwrap_or_default()
        .replace(' ', "T");
    let notice = home_notice(query.notice.as_deref(), query.remaining);

    view! {
        <main data-page="home">
            <header class="page-header">
                <div>
                    <p class="eyebrow">"Raspberry Pi"</p>
                    <h1>"Pi Auto Feeder"</h1>
                </div>
                <nav><a class="button secondary" href="/settings">"設定"</a></nav>
            </header>

            if let Some(notice) = notice {
                <p class="notice" aria-live="polite">(notice)</p>
            }

            <section class="card camera-card">
                <div class="section-heading">
                    <h2>"カメラ"</h2>
                    <span class="status-dot">"ライブ"</span>
                </div>
                <img src="/camera/stream" alt="給餌器のカメラ映像">
            </section>

            <section class="card feed-card">
                <h2>"手動給餌"</h2>
                <form
                    action="/feed"
                    method="post"
                    hx-post="/feed"
                    hx-disabled-elt="find button"
                >
                    <button class="button primary feed-button" type="submit">
                        "給餌する"
                    </button>
                </form>
                if cooldown_remaining > 0 {
                    <p
                        class="cooldown"
                        data-cooldown=(cooldown_remaining.to_string())
                        data-loaded-at=""
                    >
                        "給餌可能になるまで残り "
                        <strong data-cooldown-value="">
                            (cooldown_remaining.to_string())
                        </strong>
                        " 秒"
                    </p>
                }
            </section>

            <div class="grid">
                <section class="card schedules-card">
                    <h2>"給餌スケジュール"</h2>
                    if schedules.is_empty() {
                        <p class="empty">"スケジュールはありません"</p>
                    } else {
                        <ul class="schedule-list">
                            for schedule in &schedules {
                                schedule_item(schedule: schedule)
                            }
                        </ul>
                    }

                    <form
                        class="stacked-form"
                        action="/schedules"
                        method="post"
                        hx-post="/schedules"
                        hx-disabled-elt="find button"
                    >
                        <label for="schedule-datetime">"給餌日時"</label>
                        <div class="form-row">
                            <input
                                id="schedule-datetime"
                                name="scheduled_at"
                                type="datetime-local"
                                min=(schedule_min)
                                required="required"
                            >
                            <button class="button primary" type="submit">
                                "追加"
                            </button>
                        </div>
                    </form>
                </section>

                <section class="card status-card">
                    <h2>"サーバー情報"</h2>
                    <dl>
                        <dt>"現在のサーバー時刻"</dt>
                        <dd>
                            <time
                                data-server-time=(status.current_time.clone())
                                data-loaded-at=""
                            >
                                (status.current_time)
                            </time>
                        </dd>
                        <dt>"最終給餌時刻"</dt>
                        <dd>
                            (status
                                .last_feed_time
                                .as_deref()
                                .unwrap_or("給餌履歴はありません"))
                        </dd>
                    </dl>
                </section>
            </div>
            <section class="card history-card">
                <h2>"給餌履歴"</h2>
                if feed_history.is_empty() {
                    <p class="empty">"給餌履歴はありません"</p>
                } else {
                    <ol class="history-list">
                        for fed_at in feed_history {
                            <li><time>(fed_at)</time></li>
                        }
                    </ol>
                }
            </section>
        </main>
    }
}

#[component]
async fn schedule_item(schedule: &Schedule) -> Result {
    let failed = schedule.missed || schedule.failure_reason.is_some();
    let failure_reason = schedule
        .failure_reason
        .as_deref()
        .unwrap_or("予定時刻に給餌されませんでした");

    view! {
        <li>
            <div>
                if let Some(scheduled_at) = &schedule.scheduled_at {
                    <time datetime=(scheduled_at)>
                        (scheduled_at.replace('T', " "))
                    </time>
                    if failed {
                        <p class="error">
                            <strong>"給餌失敗: "</strong>
                            (failure_reason)
                        </p>
                    }
                } else {
                    <p>
                        <strong>"日時未設定"</strong>
                        if let Some(legacy_time) = &schedule.legacy_time {
                            "（旧設定時刻 "
                            (legacy_time)
                            "）"
                        }
                        " — 削除して再登録してください"
                    </p>
                }
            </div>
            <form
                action="/schedules/delete"
                method="post"
                hx-post="/schedules/delete"
                hx-disabled-elt="find button"
            >
                <input type="hidden" name="id" value=(schedule.id.to_string())>
                <button class="button danger" type="submit">"削除"</button>
            </form>
        </li>
    }
}

#[page("/settings")]
async fn settings_page(cx: &Cx) -> Result {
    let settings = store(cx)
        .settings()
        .await
        .map_err(|error| internal_error("Failed to load settings", error))?;
    let notice = query_params::<PageQuery>(cx)?
        .notice
        .as_deref()
        .and_then(settings_notice);

    view! {
        <main class="settings-page">
            <header class="page-header">
                <div>
                    <p class="eyebrow">"Configuration"</p>
                    <h1>"設定"</h1>
                </div>
                <nav><a class="button secondary" href="/">"トップへ戻る"</a></nav>
            </header>
            if let Some(notice) = notice {
                <p class="notice" aria-live="polite">(notice)</p>
            }
            <section class="card">
                <form
                    class="stacked-form"
                    action="/settings"
                    method="post"
                    hx-post="/settings"
                    hx-disabled-elt="find button"
                >
                    <label for="cooldown-seconds">
                        "給餌のクールタイム（秒）"
                    </label>
                    <input
                        id="cooldown-seconds"
                        name="cooldown_seconds"
                        type="number"
                        min="0"
                        max=(MAX_COOLDOWN_SECONDS.to_string())
                        value=(settings.cooldown_seconds.to_string())
                        required="required"
                    >
                    <label for="feed-duration-ms">
                        "一回の給餌量・サーボ駆動時間（ミリ秒）"
                    </label>
                    <input
                        id="feed-duration-ms"
                        name="feed_duration_ms"
                        type="number"
                        min=(MIN_FEED_DURATION_MS.to_string())
                        max=(MAX_FEED_DURATION_MS.to_string())
                        value=(settings.feed_duration_ms.to_string())
                        required="required"
                    >
                    <button class="button primary" type="submit">"保存"</button>
                </form>
            </section>
        </main>
    }
}

#[derive(Deserialize)]
struct AddSchedule {
    scheduled_at: String,
}

#[derive(Deserialize)]
struct DeleteSchedule {
    id: i64,
}

#[derive(Deserialize)]
struct SaveSettings {
    cooldown_seconds: u64,
    feed_duration_ms: u64,
}

#[route(POST "/feed")]
async fn feed(cx: &Cx) -> Result<Response> {
    let location = match feed_service(cx).feed().await {
        Ok(FeedOutcome::Fed(_)) => "/?notice=fed".to_owned(),
        Ok(FeedOutcome::Cooldown(remaining)) => {
            format!("/?notice=cooldown&remaining={remaining}")
        }
        Err(error) => {
            eprintln!("Feed request failed: {error}");
            "/?notice=error".to_owned()
        }
    };
    redirect_after_form(cx, location)
}

#[route(POST "/schedules")]
async fn add_schedule(cx: &Cx, Form(input): Form<AddSchedule>) -> Result<Response> {
    let location = match store(cx).add(&input.scheduled_at).await {
        Ok(_) => "/?notice=schedule_added",
        Err(error) => {
            eprintln!("Failed to add schedule: {error}");
            "/?notice=schedule_error"
        }
    };
    redirect_after_form(cx, location)
}

#[route(POST "/schedules/delete")]
async fn delete_schedule(cx: &Cx, Form(input): Form<DeleteSchedule>) -> Result<Response> {
    let location = match store(cx).delete(input.id).await {
        Ok(()) => "/?notice=schedule_deleted",
        Err(error) => {
            eprintln!("Failed to delete schedule: {error}");
            "/?notice=error"
        }
    };
    redirect_after_form(cx, location)
}

#[route(POST "/settings")]
async fn save_settings(cx: &Cx, Form(input): Form<SaveSettings>) -> Result<Response> {
    let feed_settings = Settings {
        cooldown_seconds: input.cooldown_seconds,
        feed_duration_ms: input.feed_duration_ms,
    };
    let location = match store(cx).update_settings(&feed_settings).await {
        Ok(()) => "/settings?notice=saved",
        Err(error) => {
            eprintln!("Failed to save settings: {error}");
            "/settings?notice=settings_error"
        }
    };
    redirect_after_form(cx, location)
}

fn store(cx: &Cx) -> &ScheduleStore {
    app_context(cx)
}

fn feed_service(cx: &Cx) -> &FeedService {
    app_context(cx)
}

fn redirect_after_form(cx: &Cx, location: impl Into<String>) -> Result<Response> {
    let location = location.into();
    if hx_request(cx) {
        return (
            HxLocation::new(location)
                .target("#app")
                .swap(SwapOption::InnerHtml),
            (),
        )
            .into_response(cx);
    }
    see_other(&location).into_response(cx)
}

fn home_notice(notice: Option<&str>, remaining: Option<u64>) -> Option<String> {
    match notice {
        Some("fed") => Some("給餌しました".to_owned()),
        Some("cooldown") => Some(format!(
            "クールタイム中です．残り {} 秒",
            remaining.unwrap_or_default()
        )),
        Some("schedule_added") => Some("スケジュールを追加しました".to_owned()),
        Some("schedule_deleted") => Some("スケジュールを削除しました".to_owned()),
        Some("schedule_error") => Some("未来の有効な日時を指定してください".to_owned()),
        Some("error") => Some("サーバー処理に失敗しました".to_owned()),
        _ => None,
    }
}

fn settings_notice(notice: &str) -> Option<&'static str> {
    match notice {
        "saved" => Some("設定を保存しました"),
        "settings_error" => Some("設定を保存できませんでした"),
        _ => None,
    }
}

fn internal_error(operation: &str, error: impl Display) -> Error {
    eprintln!("{operation}: {error}");
    std::io::Error::other("server operation failed").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_do_not_expose_internal_errors() {
        assert_eq!(
            home_notice(Some("error"), None).as_deref(),
            Some("サーバー処理に失敗しました")
        );
        assert!(home_notice(Some("unknown"), None).is_none());
    }
}
