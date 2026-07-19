use leptos::form::ActionForm;
use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};
use serde::{Deserialize, Serialize};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="ja">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ScheduleView {
    pub id: i64,
    pub scheduled_at: Option<String>,
    pub legacy_time: Option<String>,
    pub missed: bool,
    pub failure_reason: Option<String>,
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

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    view! {
        <Stylesheet id="leptos" href="/pkg/pi-auto-feeder.css" />
        <Title text="Pi Auto Feeder" />
        <Router>
            <Routes fallback=|| {
                view! {
                    <main>
                        <h1>"ページが見つかりません"</h1>
                    </main>
                }
            }>
                <Route path=path!("") view=HomePage />
                <Route path=path!("settings") view=SettingsRoute />
            </Routes>
        </Router>
    }
}

#[component]
fn HomePage() -> impl IntoView {
    let state = Resource::new(|| (), |_| load_initial_state());
    view! {
        <Suspense fallback=|| {
            view! {
                <main>
                    <p>"読み込み中…"</p>
                </main>
            }
        }>
            {move || Suspend::new(async move {
                match state.await {
                    Ok(state) => {
                        view! {
                            <Home
                                schedules=state.schedules
                                current_server_time=state.current_server_time
                                last_feed_time=state.last_feed_time
                                cooldown_remaining=state.cooldown_remaining
                                feed_history=state.feed_history
                            />
                        }
                            .into_any()
                    }
                    Err(_) => {
                        view! {
                            <main>
                                <p>"状態を読み込めませんでした"</p>
                            </main>
                        }
                            .into_any()
                    }
                }
            })}
        </Suspense>
    }
}

#[component]
fn Home(
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
    let feed = ServerAction::<FeedNow>::new();
    let delete = ServerAction::<DeleteSchedule>::new();
    let add = ServerAction::<AddSchedule>::new();
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
                Some(Err(message)) => message.to_string(),
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

            <ActionForm action=feed>
                <button type="submit" disabled=move || feed.pending().get()>
                    {move || if feed.pending().get() { "給餌中…" } else { "給餌する" }}
                </button>
            </ActionForm>
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
                                                }} <ActionForm action=delete>
                                                    <input type="hidden" name="id" value=id />
                                                    <button type="submit">"削除"</button>
                                                </ActionForm>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_any()
                    }
                }}

                <ActionForm action=add>
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
                </ActionForm>
                <p aria-live="polite">
                    {move || match add.value().get() {
                        Some(Ok(_)) => "スケジュールを追加しました",
                        Some(Err(_)) => "スケジュールの追加に失敗しました",
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

#[server]
async fn load_initial_state() -> Result<InitialState, ServerFnError> {
    use crate::schedule::ScheduleStore;
    let store = expect_context::<ScheduleStore>();
    let status = store.status().await.map_err(ServerFnError::new)?;
    let schedules = store
        .list()
        .await
        .map_err(ServerFnError::new)?
        .into_iter()
        .map(|schedule| ScheduleView {
            id: schedule.id,
            scheduled_at: schedule.scheduled_at,
            legacy_time: schedule.legacy_time,
            missed: schedule.missed,
            failure_reason: schedule.failure_reason,
        })
        .collect();
    Ok(InitialState {
        schedules,
        current_server_time: status.current_time,
        last_feed_time: status.last_feed_time,
        cooldown_remaining: store
            .cooldown_remaining()
            .await
            .map_err(ServerFnError::new)?,
        feed_history: store
            .recent_feed_history()
            .await
            .map_err(ServerFnError::new)?,
    })
}

#[component]
pub fn SettingsPage(settings: SettingsView) -> impl IntoView {
    let save = ServerAction::<SaveSettings>::new();
    view! {
        <main>
            <h1>"設定"</h1>
            <nav>
                <a href="/">"トップへ戻る"</a>
            </nav>
            <ActionForm action=save>
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
            </ActionForm>
            <p aria-live="polite">
                {move || match save.value().get() {
                    Some(Ok(())) => "設定を保存しました",
                    Some(Err(_)) => "設定を保存できませんでした",
                    None => "",
                }}
            </p>
        </main>
    }
}

#[component]
fn SettingsRoute() -> impl IntoView {
    let settings = Resource::new(|| (), |_| load_settings());
    view! {
        <Suspense fallback=|| {
            view! {
                <main>
                    <p>"読み込み中…"</p>
                </main>
            }
        }>
            {move || Suspend::new(async move {
                match settings.await {
                    Ok(settings) => view! { <SettingsPage settings /> }.into_any(),
                    Err(_) => {
                        view! {
                            <main>
                                <p>"設定を読み込めませんでした"</p>
                            </main>
                        }
                            .into_any()
                    }
                }
            })}
        </Suspense>
    }
}

#[server]
async fn load_settings() -> Result<SettingsView, ServerFnError> {
    use crate::schedule::ScheduleStore;
    let settings = expect_context::<ScheduleStore>()
        .settings()
        .await
        .map_err(ServerFnError::new)?;
    Ok(SettingsView {
        cooldown_seconds: settings.cooldown_seconds,
        feed_duration_ms: settings.feed_duration_ms,
    })
}

#[server]
async fn feed_now() -> Result<String, ServerFnError> {
    use crate::feed::{FeedOutcome, FeedService};
    match expect_context::<FeedService>()
        .feed()
        .await
        .map_err(ServerFnError::new)?
    {
        FeedOutcome::Fed(fed_at) => Ok(fed_at),
        FeedOutcome::Cooldown(remaining) => Err(ServerFnError::new(format!(
            "クールタイム中です．残り {remaining} 秒"
        ))),
    }
}

#[server]
async fn add_schedule(scheduled_at: String) -> Result<ScheduleView, ServerFnError> {
    use crate::schedule::ScheduleStore;
    let schedule = expect_context::<ScheduleStore>()
        .add(&scheduled_at)
        .await
        .map_err(ServerFnError::new)?;
    Ok(ScheduleView {
        id: schedule.id,
        scheduled_at: schedule.scheduled_at,
        legacy_time: schedule.legacy_time,
        missed: schedule.missed,
        failure_reason: schedule.failure_reason,
    })
}

#[server]
async fn delete_schedule(id: i64) -> Result<i64, ServerFnError> {
    use crate::schedule::ScheduleStore;
    expect_context::<ScheduleStore>()
        .delete(id)
        .await
        .map_err(ServerFnError::new)?;
    Ok(id)
}

#[server]
async fn save_settings(cooldown_seconds: u64, feed_duration_ms: u64) -> Result<(), ServerFnError> {
    use crate::schedule::{ScheduleStore, Settings};
    expect_context::<ScheduleStore>()
        .update_settings(&Settings {
            cooldown_seconds,
            feed_duration_ms,
        })
        .await
        .map_err(ServerFnError::new)
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

    let Ok(events) = web_sys::EventSource::new("/events") else {
        return;
    };
    let on_message = Closure::<dyn FnMut()>::new(move || {
        leptos::task::spawn_local(async move {
            let Ok(state) = load_initial_state().await else {
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
