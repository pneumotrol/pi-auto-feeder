use super::{
    model::SettingsView,
    server_fns::{SaveSettings, load_settings},
};
use leptos::{form::ActionForm, prelude::*};
use leptos_router::components::A;

#[component]
pub(super) fn SettingsRoute() -> impl IntoView {
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
