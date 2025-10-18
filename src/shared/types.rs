use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataStatusDto {
    #[serde(rename = "remainingPercentage")]
    pub remaining_percentage: i32,
    #[serde(rename = "remainingDataMB")]
    pub remaining_data_mb: i32,
    #[serde(rename = "dateTime")]
    pub date_time: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerStatusDto {
    pub started: bool,
    pub running: bool,
    pub db_url: String,
    pub last_loop_at: Option<String>,
    pub last_event: Option<String>,
    pub last_error: Option<String>,
    pub next_iteration_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyUsagePointDto {
    pub date: String, // yyyy-mm-dd
    pub used_mb: i32, // usage within that day
}
