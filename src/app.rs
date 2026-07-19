mod home;
#[cfg(feature = "hydrate")]
mod live_sync;
mod model;
mod server_fns;
mod settings;

use home::HomePage;
use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{
    SsrMode,
    components::{Route, Router, Routes},
    path,
};
use settings::SettingsRoute;

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
                <Route path=path!("") view=HomePage ssr=SsrMode::InOrder />
                <Route path=path!("settings") view=SettingsRoute ssr=SsrMode::InOrder />
            </Routes>
        </Router>
    }
}
