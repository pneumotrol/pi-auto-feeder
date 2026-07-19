use super::home::{HomeSignals, RefreshAction};
use leptos::{leptos_dom::helpers::set_interval_with_handle, prelude::*};
use std::time::Duration;
use wasm_bindgen::{JsCast, closure::Closure};

pub(super) fn use_live_sync(refresh: RefreshAction, signals: HomeSignals) {
    Effect::new(move |_| {
        signals.clock_started_at.set(performance_now());

        let Ok(events) = web_sys::EventSource::new("/events") else {
            return;
        };
        let on_message = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            refresh.dispatch(());
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

fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or_default()
}

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
