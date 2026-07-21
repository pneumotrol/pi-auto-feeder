//! サーバとブラウザ向け Wasm で共有するクレートのエントリポイント。
//!
//! `ssr` ではハードウェアと永続化を含むサーバ用モジュールを公開し、`hydrate` では
//! 同じ [`app::App`] を既存の SSR DOM にハイドレートする。

pub mod app;

#[cfg(feature = "ssr")]
pub mod camera;
#[cfg(feature = "ssr")]
pub mod events;
#[cfg(feature = "ssr")]
pub mod feed;
#[cfg(feature = "ssr")]
pub mod schedule;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
/// SSR 済みの `<body>` に Leptos アプリを接続するブラウザ側エントリポイント。
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
