//! 給餌スケジュール、設定、履歴の永続化とバックグラウンド実行をまとめる。

mod migration;
mod scheduler;
mod store;

pub use scheduler::start_scheduler;
pub use store::ScheduleStore;

/// 新規データベースで使用するクールタイムの既定値（秒）。
pub const DEFAULT_COOLDOWN_SECONDS: u64 = 300;
/// 新規データベースで使用するサーボ回転時間の既定値（ミリ秒）。
pub const DEFAULT_FEED_DURATION_MS: u64 = 1_000;
/// 新規データベースで使用する給餌速度（パーセント）。
pub const DEFAULT_FEED_SPEED_PERCENT: u64 = 100;
/// 設定可能なクールタイムの上限（秒）。
pub const MAX_COOLDOWN_SECONDS: u64 = 86_400;
/// 設定可能なサーボ回転時間の下限（ミリ秒）。
pub const MIN_FEED_DURATION_MS: u64 = 100;
/// 設定可能なサーボ回転時間の上限（ミリ秒）。
pub const MAX_FEED_DURATION_MS: u64 = 10_000;
/// 設定可能な給餌速度の下限（パーセント）。
pub const MIN_FEED_SPEED_PERCENT: u64 = 1;
/// 設定可能な給餌速度の上限（パーセント）。
pub const MAX_FEED_SPEED_PERCENT: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
/// SQLite に保存された一回限りの給餌予定。
pub struct Schedule {
    /// 行を一意に識別する ID。
    pub id: i64,
    /// サーバのローカル日時。
    pub scheduled_at: String,
    /// 未実行のまま予定日時を過ぎたか。
    pub missed: bool,
    /// 実行を開始したものの成功完了しなかった理由。
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 永続化される給餌動作の設定。
pub struct Settings {
    /// 給餌成功後に次の給餌を抑止する秒数。
    pub cooldown_seconds: u64,
    /// サーボを給餌方向へ回転させるミリ秒数。
    pub feed_duration_ms: u64,
    /// サーボを給餌方向へ回転させる速度（パーセント）。
    pub feed_speed_percent: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// トップ画面へ表示するサーバ時刻と直近の給餌時刻。
pub struct ServerStatus {
    /// SQLite が算出した現在のサーバローカル日時。
    pub current_time: String,
    /// 最後に正常記録された給餌日時。
    pub last_feed_time: Option<String>,
}
