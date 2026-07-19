mod migration;
mod scheduler;
mod store;

pub use scheduler::start_scheduler;
pub use store::ScheduleStore;

pub const DEFAULT_COOLDOWN_SECONDS: u64 = 300;
pub const DEFAULT_FEED_DURATION_MS: u64 = 1_000;
pub const MAX_COOLDOWN_SECONDS: u64 = 86_400;
pub const MIN_FEED_DURATION_MS: u64 = 100;
pub const MAX_FEED_DURATION_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub id: i64,
    pub scheduled_at: Option<String>,
    pub legacy_time: Option<String>,
    pub missed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub cooldown_seconds: u64,
    pub feed_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    pub current_time: String,
    pub last_feed_time: Option<String>,
}
