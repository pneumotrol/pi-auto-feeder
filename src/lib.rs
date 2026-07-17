use leptos::prelude::*;
use serde::{Deserialize, Serialize};
#[cfg(feature = "hydrate")]
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(Clone, Deserialize, Serialize)]
pub struct ScheduleView {
    pub id: i64,
    pub scheduled_at: Option<String>,
    pub legacy_time: Option<String>,
    pub missed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct NewSchedule {
    pub scheduled_at: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct InitialState {
    pub schedules: Vec<ScheduleView>,
    pub current_server_time: String,
    pub last_feed_time: Option<String>,
    pub cooldown_remaining: u64,
    pub feed_history: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SettingsView {
    pub cooldown_seconds: u64,
    pub feed_duration_ms: u64,
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen(start)]
pub fn hydrate() {
    let state = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("initial-state"))
        .and_then(|element| element.text_content())
        .and_then(|json| serde_json::from_str::<InitialState>(&json).ok())
        .expect("initial application state must be present");
    leptos::mount::hydrate_body(move || {
        view! {
            <App
                schedules=state.schedules
                current_server_time=state.current_server_time
                last_feed_time=state.last_feed_time
                cooldown_remaining=state.cooldown_remaining
                feed_history=state.feed_history
            />
        }
    });
}

#[component]
pub fn App(
    schedules: Vec<ScheduleView>,
    current_server_time: String,
    last_feed_time: Option<String>,
    cooldown_remaining: u64,
    feed_history: Vec<String>,
) -> impl IntoView {
    let minimum_schedule = current_server_time[..16].replace(' ', "T");
    let (schedules, set_schedules) = signal(schedules);
    let (current_server_time, set_current_server_time) = signal(current_server_time);
    let (last_feed_time, set_last_feed_time) = signal(last_feed_time);
    let (cooldown_remaining, set_cooldown_remaining) = signal(cooldown_remaining);
    let (feed_history, set_feed_history) = signal(feed_history);
    let (new_scheduled_at, set_new_scheduled_at) = signal(String::new());
    let feed = Action::new_local(|_: &()| request_feed());
    let delete = Action::new_local(|id: &i64| request_delete(*id));
    let add = Action::new_local(|scheduled_at: &String| request_add(scheduled_at.clone()));
    setup_live_updates(
        set_schedules,
        set_current_server_time,
        set_last_feed_time,
        set_cooldown_remaining,
        set_feed_history,
    );

    Effect::new(move |_| {
        if let Some(Ok(id)) = delete.value().get() {
            set_schedules.update(|schedules| schedules.retain(|schedule| schedule.id != id));
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(schedule)) = add.value().get() {
            set_schedules.update(|schedules| {
                schedules.retain(|existing| existing.id != schedule.id);
                schedules.push(schedule);
                schedules.sort_by(|left, right| left.scheduled_at.cmp(&right.scheduled_at));
            });
            set_new_scheduled_at.set(String::new());
        }
    });

    let feed_status = move || {
        if feed.pending().get() {
            "給餌中…".to_owned()
        } else {
            match feed.value().get() {
                Some(Ok(_)) => "給餌しました".to_owned(),
                Some(Err(message)) => message,
                None => String::new(),
            }
        }
    };

    view! {
        <main>
            <h1>"Pi Auto Feeder"</h1>

            <nav>
                <a href="/settings">"設定"</a>
            </nav>

            <section>
                <h2>"カメラ"</h2>
                <img src="/camera/stream" alt="給餌器のカメラ映像" />
            </section>

            <form
                method="post"
                action="/feed"
                on:submit=move |event| {
                    event.prevent_default();
                    if !feed.pending().get_untracked() {
                        feed.dispatch(());
                    }
                }
            >
                <button type="submit" disabled=move || feed.pending().get()>
                    {move || if feed.pending().get() { "給餌中…" } else { "給餌する" }}
                </button>
            </form>
            <p aria-live="polite">{feed_status}</p>

            <section>
                <h2>"給餌スケジュール"</h2>
                {move || {
                    let schedules = schedules.get();
                    if schedules.is_empty() {
                        view! { <p>"スケジュールはありません"</p> }.into_any()
                    } else {
                        view! {
                            <ul>
                                {schedules
                                    .into_iter()
                                    .map(|schedule| {
                                        let id = schedule.id;
                                        let action = format!("/schedules/{id}/delete");
                                        let scheduled_at = schedule.scheduled_at.clone();
                                        let legacy_time = schedule.legacy_time.clone();
                                        let failure_reason = schedule.failure_reason.clone();
                                        let failed = schedule.missed || failure_reason.is_some();
                                        view! {
                                            <li>
                                                {match scheduled_at {
                                                    Some(scheduled_at) => {
                                                        let label = scheduled_at.replace('T', " ");
                                                        view! {
                                                            <div>
                                                                <time datetime=scheduled_at>{label}</time>
                                                                {failed
                                                                    .then(|| {
                                                                        view! {
                                                                            <p>
                                                                                <strong>"給餌失敗"</strong>
                                                                                {failure_reason
                                                                                    .clone()
                                                                                    .unwrap_or_else(|| {
                                                                                        "予定時刻に給餌されませんでした".to_owned()
                                                                                    })}
                                                                            </p>
                                                                        }
                                                                    })}
                                                            </div>
                                                        }
                                                            .into_any()
                                                    }
                                                    None => {
                                                        view! {
                                                            <p>
                                                                <strong>"日時未設定"</strong>
                                                                {legacy_time
                                                                    .map(|time| { format!("（旧設定時刻 {time}）") })}
                                                                " — 削除して再登録してください"
                                                            </p>
                                                        }
                                                            .into_any()
                                                    }
                                                }}
                                                <form
                                                    method="post"
                                                    action=action
                                                    on:submit=move |event| {
                                                        event.prevent_default();
                                                        delete.dispatch(id);
                                                    }
                                                >
                                                    <button type="submit">"削除"</button>
                                                </form>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_any()
                    }
                }}

                <form
                    method="post"
                    action="/schedules"
                    on:submit=move |event| {
                        event.prevent_default();
                        let scheduled_at = new_scheduled_at.get_untracked();
                        if !scheduled_at.is_empty() && !add.pending().get_untracked() {
                            add.dispatch(scheduled_at);
                        }
                    }
                >
                    <label for="schedule-datetime">"給餌日時"</label>
                    <input
                        id="schedule-datetime"
                        name="scheduled_at"
                        type="datetime-local"
                        required
                        min=minimum_schedule
                        prop:value=move || new_scheduled_at.get()
                        on:input=move |event| {
                            set_new_scheduled_at.set(event_target_value(&event))
                        }
                    />
                    <button type="submit" disabled=move || add.pending().get()>
                        {move || if add.pending().get() { "追加中…" } else { "追加" }}
                    </button>
                </form>
                <p aria-live="polite">
                    {move || match add.value().get() {
                        Some(Ok(_)) => "スケジュールを追加しました",
                        Some(Err(())) => "スケジュールの追加に失敗しました",
                        None => "",
                    }}
                </p>
            </section>

            <section>
                <h2>"サーバー情報"</h2>
                <dl>
                    <dt>"現在のサーバー時刻"</dt>
                    <dd>{move || current_server_time.get()}</dd>
                    <dt>"最終給餌時刻"</dt>
                    <dd>
                        {move || {
                            feed.value()
                                .get()
                                .and_then(Result::ok)
                                .or_else(|| last_feed_time.get())
                                .unwrap_or_else(|| "給餌履歴はありません".to_owned())
                        }}
                    </dd>
                </dl>
                <p>
                    {move || {
                        let remaining = cooldown_remaining.get();
                        (remaining > 0)
                            .then(|| format!("給餌可能になるまで残り {remaining} 秒"))
                    }}
                </p>
            </section>

            <section>
                <h2>"給餌履歴"</h2>
                {move || {
                    let history = feed_history.get();
                    if history.is_empty() {
                        view! { <p>"給餌履歴はありません"</p> }.into_any()
                    } else {
                        view! {
                            <ul>
                                {history
                                    .into_iter()
                                    .map(|fed_at| {
                                        view! {
                                            <li>
                                                <time>{fed_at}</time>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_any()
                    }
                }}
            </section>
        </main>
    }
}

#[cfg(feature = "hydrate")]
async fn request_feed() -> Result<String, String> {
    let response = gloo_net::http::Request::post("/api/feed")
        .send()
        .await
        .map_err(|_| "通信に失敗しました".to_owned())?;
    if !response.ok() {
        return Err(response
            .text()
            .await
            .unwrap_or_else(|_| "給餌できませんでした".to_owned()));
    }
    response
        .text()
        .await
        .map_err(|_| "応答を読み取れませんでした".to_owned())
}

#[cfg(not(feature = "hydrate"))]
async fn request_feed() -> Result<String, String> {
    Err("給餌できませんでした".to_owned())
}

#[component]
pub fn SettingsPage(settings: SettingsView) -> impl IntoView {
    view! {
        <main>
            <h1>"設定"</h1>
            <nav>
                <a href="/">"トップへ戻る"</a>
            </nav>
            <form method="post" action="/settings">
                <label for="cooldown-seconds">"給餌のクールタイム（秒）"</label>
                <input
                    id="cooldown-seconds"
                    name="cooldown_seconds"
                    type="number"
                    min="0"
                    max="86400"
                    required
                    value=settings.cooldown_seconds
                />
                <label for="feed-duration-ms">
                    "一回の給餌量・サーボ駆動時間（ミリ秒）"
                </label>
                <input
                    id="feed-duration-ms"
                    name="feed_duration_ms"
                    type="number"
                    min="100"
                    max="10000"
                    required
                    value=settings.feed_duration_ms
                />
                <button type="submit">"保存"</button>
            </form>
        </main>
    }
}

#[cfg(feature = "hydrate")]
async fn request_delete(id: i64) -> Result<i64, ()> {
    let response = gloo_net::http::Request::post(&format!("/api/schedules/{id}/delete"))
        .send()
        .await
        .map_err(|_| ())?;
    response.ok().then_some(id).ok_or(())
}

#[cfg(not(feature = "hydrate"))]
async fn request_delete(_id: i64) -> Result<i64, ()> {
    Err(())
}

#[cfg(feature = "hydrate")]
async fn request_add(scheduled_at: String) -> Result<ScheduleView, ()> {
    let response = gloo_net::http::Request::post("/api/schedules")
        .json(&NewSchedule { scheduled_at })
        .map_err(|_| ())?
        .send()
        .await
        .map_err(|_| ())?;
    if !response.ok() {
        return Err(());
    }
    response.json().await.map_err(|_| ())
}

#[cfg(not(feature = "hydrate"))]
async fn request_add(_scheduled_at: String) -> Result<ScheduleView, ()> {
    Err(())
}

#[cfg(feature = "hydrate")]
fn setup_live_updates(
    set_schedules: WriteSignal<Vec<ScheduleView>>,
    set_current_server_time: WriteSignal<String>,
    set_last_feed_time: WriteSignal<Option<String>>,
    set_cooldown_remaining: WriteSignal<u64>,
    set_feed_history: WriteSignal<Vec<String>>,
) {
    use wasm_bindgen::{JsCast, closure::Closure};

    let Ok(events) = web_sys::EventSource::new("/api/events") else {
        return;
    };
    let on_message = Closure::<dyn FnMut()>::new(move || {
        leptos::task::spawn_local(async move {
            let Ok(response) = gloo_net::http::Request::get("/api/state").send().await else {
                return;
            };
            let Ok(state) = response.json::<InitialState>().await else {
                return;
            };
            set_schedules.set(state.schedules);
            set_current_server_time.set(state.current_server_time);
            set_last_feed_time.set(state.last_feed_time);
            set_cooldown_remaining.set(state.cooldown_remaining);
            set_feed_history.set(state.feed_history);
        });
    });
    events.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    std::mem::forget(events);
}

#[cfg(not(feature = "hydrate"))]
fn setup_live_updates(
    _set_schedules: WriteSignal<Vec<ScheduleView>>,
    _set_current_server_time: WriteSignal<String>,
    _set_last_feed_time: WriteSignal<Option<String>>,
    _set_cooldown_remaining: WriteSignal<u64>,
    _set_feed_history: WriteSignal<Vec<String>>,
) {
}
