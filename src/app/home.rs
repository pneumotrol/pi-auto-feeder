use super::{
    model::{InitialState, ScheduleView},
    server_fns::{AddSchedule, DeleteSchedule, FeedNow, load_initial_state},
};
use leptos::{form::ActionForm, prelude::*};
use leptos_router::components::A;

pub(super) type RefreshAction = Action<(), Result<InitialState, ServerFnError>>;

#[component]
pub(super) fn HomePage() -> impl IntoView {
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
                    Ok(state) => view! { <Home state /> }.into_any(),
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
fn Home(state: InitialState) -> impl IntoView {
    let schedules = RwSignal::new(state.schedules);
    let server_time_base = RwSignal::new(state.current_server_time.clone());
    let current_server_time = RwSignal::new(state.current_server_time);
    let last_feed_time = RwSignal::new(state.last_feed_time);
    let cooldown_base = RwSignal::new(state.cooldown_remaining);
    let cooldown_remaining = RwSignal::new(state.cooldown_remaining);
    let clock_started_at = RwSignal::new(0.0);
    let feed_history = RwSignal::new(state.feed_history);
    let signals = HomeSignals {
        schedules,
        server_time_base,
        current_server_time,
        last_feed_time,
        cooldown_base,
        cooldown_remaining,
        clock_started_at,
        feed_history,
    };
    let refresh = Action::new(|_: &()| load_initial_state());
    let new_scheduled_at = RwSignal::new(String::new());
    let feed = ServerAction::<FeedNow>::new();
    let delete = ServerAction::<DeleteSchedule>::new();
    let add = ServerAction::<AddSchedule>::new();

    synchronize_state(refresh, signals);
    refresh_after_success(feed, refresh);
    refresh_after_success(delete, refresh);
    Effect::new(move |_| {
        if matches!(add.value().get(), Some(Ok(_))) {
            new_scheduled_at.set(String::new());
            refresh.dispatch(());
        }
    });
    #[cfg(feature = "hydrate")]
    super::live_sync::use_live_sync(refresh, signals);

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
                            last_feed_time
                                .get()
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

fn refresh_after_success<S>(action: ServerAction<S>, refresh: RefreshAction)
where
    S: server_fn::ServerFn<Error = ServerFnError> + Clone + Send + Sync + 'static,
    S::Output: Clone + Send + Sync + 'static,
{
    Effect::new(move |_| {
        if matches!(action.value().get(), Some(Ok(_))) {
            refresh.dispatch(());
        }
    });
}

fn synchronize_state(refresh: RefreshAction, signals: HomeSignals) {
    Effect::new(move |_| {
        let Some(Ok(state)) = refresh.value().get() else {
            return;
        };
        signals.apply(state);
    });
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
                <button type="submit" disabled=move || delete.pending().get()>
                    {move || if delete.pending().get() { "削除中…" } else { "削除" }}
                </button>
            </ActionForm>
        </li>
    }
}

#[derive(Clone, Copy)]
pub(super) struct HomeSignals {
    pub(super) schedules: RwSignal<Vec<ScheduleView>>,
    pub(super) server_time_base: RwSignal<String>,
    pub(super) current_server_time: RwSignal<String>,
    pub(super) last_feed_time: RwSignal<Option<String>>,
    pub(super) cooldown_base: RwSignal<u64>,
    pub(super) cooldown_remaining: RwSignal<u64>,
    pub(super) clock_started_at: RwSignal<f64>,
    pub(super) feed_history: RwSignal<Vec<String>>,
}

impl HomeSignals {
    fn apply(self, state: InitialState) {
        self.schedules.set(state.schedules);
        self.server_time_base.set(state.current_server_time.clone());
        self.current_server_time.set(state.current_server_time);
        self.last_feed_time.set(state.last_feed_time);
        self.cooldown_base.set(state.cooldown_remaining);
        self.cooldown_remaining.set(state.cooldown_remaining);
        self.clock_started_at.set(performance_now());
        self.feed_history.set(state.feed_history);
    }
}

fn performance_now() -> f64 {
    #[cfg(feature = "hydrate")]
    {
        return web_sys::window()
            .and_then(|window| window.performance())
            .map(|performance| performance.now())
            .unwrap_or_default();
    }
    #[cfg(not(feature = "hydrate"))]
    0.0
}
