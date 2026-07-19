use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScheduleView {
    pub id: i64,
    pub scheduled_at: Option<String>,
    pub legacy_time: Option<String>,
    pub missed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InitialState {
    pub schedules: Vec<ScheduleView>,
    pub current_server_time: String,
    pub last_feed_time: Option<String>,
    pub cooldown_remaining: u64,
    pub feed_history: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SettingsView {
    pub cooldown_seconds: u64,
    pub feed_duration_ms: u64,
}
