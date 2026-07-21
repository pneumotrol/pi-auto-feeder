//! UI から呼び出す Leptos server functions。
//!
//! 読み書きはすべてサーバ側のサービスへ委譲し、ブラウザへ内部エラーの詳細を返さない。

use super::model::{InitialState, ScheduleView, SettingsView};
use leptos::prelude::*;

#[cfg(feature = "ssr")]
/// 運用ログには原因を残しつつ、レスポンスでは一定のエラー文へ変換する。
fn internal_error(operation: &str, error: impl std::fmt::Display) -> ServerFnError {
    eprintln!("{operation}: {error}");
    ServerFnError::new("サーバー処理に失敗しました")
}

#[server]
/// トップ画面で互いに整合している必要がある表示データをまとめて取得する。
pub async fn load_initial_state() -> Result<InitialState, ServerFnError> {
    use crate::schedule::ScheduleStore;
    let store = expect_context::<ScheduleStore>();
    let status = store
        .status()
        .await
        .map_err(|error| internal_error("Failed to load server status", error))?;
    let schedules = store
        .list()
        .await
        .map_err(|error| internal_error("Failed to load schedules", error))?
        .into_iter()
        .map(ScheduleView::from)
        .collect();
    Ok(InitialState {
        schedules,
        current_server_time: status.current_time,
        last_feed_time: status.last_feed_time,
        cooldown_remaining: store
            .cooldown_remaining()
            .await
            .map_err(|error| internal_error("Failed to load cooldown", error))?,
        feed_history: store
            .recent_feed_history()
            .await
            .map_err(|error| internal_error("Failed to load feed history", error))?,
    })
}

#[server]
/// 現在の給餌設定を取得する。
pub async fn load_settings() -> Result<SettingsView, ServerFnError> {
    use crate::schedule::ScheduleStore;
    let settings = expect_context::<ScheduleStore>()
        .settings()
        .await
        .map_err(|error| internal_error("Failed to load settings", error))?;
    Ok(SettingsView {
        cooldown_seconds: settings.cooldown_seconds,
        feed_duration_ms: settings.feed_duration_ms,
    })
}

#[server]
/// 共通の給餌サービスを通して手動給餌を実行する。
pub async fn feed_now() -> Result<String, ServerFnError> {
    use crate::feed::{FeedOutcome, FeedService};
    match expect_context::<FeedService>()
        .feed()
        .await
        .map_err(|error| internal_error("Feed request failed", error))?
    {
        FeedOutcome::Fed(fed_at) => Ok(fed_at),
        FeedOutcome::Cooldown(remaining) => Err(ServerFnError::new(format!(
            "クールタイム中です．残り {remaining} 秒"
        ))),
    }
}

#[server]
/// サーバのローカル日時として一回限りのスケジュールを登録する。
pub async fn add_schedule(scheduled_at: String) -> Result<ScheduleView, ServerFnError> {
    use crate::schedule::ScheduleStore;
    expect_context::<ScheduleStore>()
        .add(&scheduled_at)
        .await
        .map(ScheduleView::from)
        .map_err(|error| internal_error("Failed to add schedule", error))
}

#[server]
/// 指定したスケジュールを削除し、フォーム側へ同じ ID を返す。
pub async fn delete_schedule(id: i64) -> Result<i64, ServerFnError> {
    use crate::schedule::ScheduleStore;
    expect_context::<ScheduleStore>()
        .delete(id)
        .await
        .map_err(|error| internal_error("Failed to delete schedule", error))?;
    Ok(id)
}

#[server]
/// サーバ側の範囲検証を通して給餌設定を保存する。
pub async fn save_settings(
    cooldown_seconds: u64,
    feed_duration_ms: u64,
) -> Result<(), ServerFnError> {
    use crate::schedule::{ScheduleStore, Settings};
    expect_context::<ScheduleStore>()
        .update_settings(&Settings {
            cooldown_seconds,
            feed_duration_ms,
        })
        .await
        .map_err(|error| internal_error("Failed to save settings", error))
}

#[cfg(feature = "ssr")]
impl From<crate::schedule::Schedule> for ScheduleView {
    fn from(schedule: crate::schedule::Schedule) -> Self {
        Self {
            id: schedule.id,
            scheduled_at: schedule.scheduled_at,
            legacy_time: schedule.legacy_time,
            missed: schedule.missed,
            failure_reason: schedule.failure_reason,
        }
    }
}
