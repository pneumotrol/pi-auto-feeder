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
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
