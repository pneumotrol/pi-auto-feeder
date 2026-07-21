//! server functions の境界を越えて SSR と Wasm の双方で使う表示用データ型。

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
/// 一覧表示に必要な給餌スケジュールの状態。
pub struct ScheduleView {
    /// 削除操作と keyed list に用いるデータベース ID。
    pub id: i64,
    /// 新形式のローカル日時。旧形式の行では `None`。
    pub scheduled_at: Option<String>,
    /// 移行前に保存されていた時刻のみの値。
    pub legacy_time: Option<String>,
    /// 予定日時を過ぎたまま実行されなかったか。
    pub missed: bool,
    /// 実行開始後に完了できなかった理由。
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
/// トップ画面を一貫した状態へ更新するためのサーバスナップショット。
pub struct InitialState {
    pub schedules: Vec<ScheduleView>,
    pub current_server_time: String,
    pub last_feed_time: Option<String>,
    pub cooldown_remaining: u64,
    pub feed_history: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
/// 設定画面へ公開する給餌パラメータ。
pub struct SettingsView {
    pub cooldown_seconds: u64,
    pub feed_duration_ms: u64,
}
