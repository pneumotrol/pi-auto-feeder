use leptos::form::ActionForm;
use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    components::{A, Route, Router, Routes},
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
    let schedules = RwSignal::new(schedules);
    #[cfg(feature = "hydrate")]
    let server_time_base = RwSignal::new(current_server_time.clone());
    let current_server_time = RwSignal::new(current_server_time);
    let last_feed_time = RwSignal::new(last_feed_time);
    #[cfg(feature = "hydrate")]
    let cooldown_base = RwSignal::new(cooldown_remaining);
    let cooldown_remaining = RwSignal::new(cooldown_remaining);
    #[cfg(feature = "hydrate")]
    let clock_started_at = RwSignal::new(0.0);
    let feed_history = RwSignal::new(feed_history);
    let new_scheduled_at = RwSignal::new(String::new());
    let feed = ServerAction::<FeedNow>::new();
    let delete = ServerAction::<DeleteSchedule>::new();
    let add = ServerAction::<AddSchedule>::new();
    #[cfg(feature = "hydrate")]
    use_client_sync(LiveStateSignals {
        schedules,
        server_time_base,
        current_server_time,
        last_feed_time,
        cooldown_base,
        cooldown_remaining,
        clock_started_at,
        feed_history,
    });

    Effect::new(move |_| {
        if let Some(Ok(id)) = delete.value().get() {
            schedules.update(|schedules| schedules.retain(|schedule| schedule.id != id));
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(schedule)) = add.value().get() {
            schedules.update(|schedules| {
                schedules.retain(|existing| existing.id != schedule.id);
                schedules.push(schedule);
                schedules.sort_by(|left, right| left.scheduled_at.cmp(&right.scheduled_at));
            });
            new_scheduled_at.set(String::new());
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
                <A href="/settings">"設定"</A>
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
                <Show
                    when=move || schedules.with(Vec::is_empty)
                    fallback=move || {
                        view! {
                            <ul>
                                <For
                                    each=move || schedules.get()
                                    key=|schedule| schedule.id
                                    children=move |schedule| {
                                        view! { <ScheduleItem schedule delete /> }
                                    }
                                />
                            </ul>
                        }
                    }
                >
                    <p>"スケジュールはありません"</p>
                </Show>

                <ActionForm action=add>
                    <label for="schedule-datetime">"給餌日時"</label>
                    <input
                        id="schedule-datetime"
                        name="scheduled_at"
                        type="datetime-local"
                        required
                        min=move || {
                            current_server_time
                                .get()
                                .get(..16)
                                .unwrap_or_default()
                                .replace(' ', "T")
                        }
                        prop:value=move || new_scheduled_at.get()
                        on:input=move |event| { new_scheduled_at.set(event_target_value(&event)) }
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
                <Show
                    when=move || feed_history.with(Vec::is_empty)
                    fallback=move || {
                        view! {
                            <ul>
                                <For
                                    each=move || feed_history.get()
                                    key=|fed_at| fed_at.clone()
                                    children=|fed_at| {
                                        view! {
                                            <li>
                                                <time>{fed_at}</time>
                                            </li>
                                        }
                                    }
                                />
                            </ul>
                        }
                    }
                >
                    <p>"給餌履歴はありません"</p>
                </Show>
            </section>
        </main>
    }
}

#[component]
fn ScheduleItem(schedule: ScheduleView, delete: ServerAction<DeleteSchedule>) -> impl IntoView {
    let id = schedule.id;
    let failed = schedule.missed || schedule.failure_reason.is_some();
    view! {
        <li>
            {match schedule.scheduled_at {
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
                                            {schedule
                                                .failure_reason
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
                            {schedule
                                .legacy_time
                                .map(|time| format!("（旧設定時刻 {time}）"))}
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
fn SettingsPage(settings: SettingsView) -> impl IntoView {
    let save = ServerAction::<SaveSettings>::new();
    view! {
        <main>
            <h1>"設定"</h1>
            <nav>
                <A href="/">"トップへ戻る"</A>
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
                <button type="submit" disabled=move || save.pending().get()>
                    {move || if save.pending().get() { "保存中…" } else { "保存" }}
                </button>
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
#[derive(Clone, Copy)]
struct LiveStateSignals {
    schedules: RwSignal<Vec<ScheduleView>>,
    server_time_base: RwSignal<String>,
    current_server_time: RwSignal<String>,
    last_feed_time: RwSignal<Option<String>>,
    cooldown_base: RwSignal<u64>,
    cooldown_remaining: RwSignal<u64>,
    clock_started_at: RwSignal<f64>,
    feed_history: RwSignal<Vec<String>>,
}

#[cfg(feature = "hydrate")]
fn use_client_sync(signals: LiveStateSignals) {
    use leptos::leptos_dom::helpers::set_interval_with_handle;
    use std::time::Duration;
    use wasm_bindgen::{JsCast, closure::Closure};

    Effect::new(move |_| {
        signals.clock_started_at.set(performance_now());

        let Ok(events) = web_sys::EventSource::new("/events") else {
            return;
        };
        let on_message = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            leptos::task::spawn_local(async move {
                let Ok(state) = load_initial_state().await else {
                    return;
                };
                signals.schedules.set(state.schedules);
                signals
                    .server_time_base
                    .set(state.current_server_time.clone());
                signals.current_server_time.set(state.current_server_time);
                signals.last_feed_time.set(state.last_feed_time);
                signals.cooldown_base.set(state.cooldown_remaining);
                signals.cooldown_remaining.set(state.cooldown_remaining);
                signals.clock_started_at.set(performance_now());
                signals.feed_history.set(state.feed_history);
            });
        })
        .into_js_value();
        events.set_onmessage(Some(on_message.unchecked_ref()));
        on_cleanup(move || {
            events.set_onmessage(None);
            events.close();
        });
    });

    Effect::new(move |_| {
        let Ok(handle) = set_interval_with_handle(
            move || {
                let elapsed_seconds =
                    ((performance_now() - signals.clock_started_at.get_untracked()) / 1_000.0)
                        .max(0.0) as u64;
                signals.current_server_time.set(advance_server_time(
                    &signals.server_time_base.get_untracked(),
                    elapsed_seconds,
                ));
                signals.cooldown_remaining.set(
                    signals
                        .cooldown_base
                        .get_untracked()
                        .saturating_sub(elapsed_seconds),
                );
            },
            Duration::from_secs(1),
        ) else {
            return;
        };
        on_cleanup(move || handle.clear());
    });
}

#[cfg(feature = "hydrate")]
fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or_default()
}

#[cfg(feature = "hydrate")]
fn advance_server_time(value: &str, elapsed_seconds: u64) -> String {
    use chrono::{Duration, NaiveDateTime};

    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|time| {
            i64::try_from(elapsed_seconds)
                .ok()
                .and_then(|seconds| time.checked_add_signed(Duration::seconds(seconds)))
        })
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| value.to_owned())
}
