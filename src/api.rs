use dioxus::prelude::*;

use crate::shared::types::{DailyUsagePointDto, DataStatusDto, SchedulerStatusDto};

#[server(LatestDataStatus)]
pub async fn latest_data_status() -> Result<Option<DataStatusDto>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::backend::GLOBAL_DB;

        let Some(db) = GLOBAL_DB.get() else {
            eprintln!("latest_data_status: DB not initialized");
            return Ok(None);
        };
        match db.get_latest_data_status().await {
            Ok(Some(r)) => Ok(Some(DataStatusDto {
                remaining_percentage: r.remaining_percentage,
                remaining_data_mb: r.remaining_data_mb,
                date_time: r.date_time.to_rfc3339(),
            })),
            Ok(None) => Ok(None),
            Err(e) => {
                eprintln!("latest_data_status query error: {e}");
                Ok(None)
            }
        }
    }
    #[cfg(not(feature = "server"))]
    {
        Ok(None)
    }
}

#[server(GetSchedulerStatus)]
pub async fn get_scheduler_status() -> Result<SchedulerStatusDto, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::backend::scheduler::{SCHED_HANDLE, STATUS};

        if let Some(st) = STATUS.get() {
            let s = st.read().await.clone();
            // Derive true running status from the join handle, if present
            let running = if let Some(hcell) = SCHED_HANDLE.get() {
                let h = hcell.read().await;
                h.as_ref().map(|j| !j.is_finished()).unwrap_or(false)
            } else {
                false
            };
            return Ok(SchedulerStatusDto {
                started: s.started,
                running,
                db_url: s.db_url,
                last_loop_at: s.last_loop_at,
                last_event: s.last_event,
                last_error: s.last_error,
                next_iteration_at: s.next_iteration_at,
            });
        }
        return Ok(SchedulerStatusDto {
            started: false,
            running: false,
            db_url: String::new(),
            last_loop_at: None,
            last_event: Some("not started".into()),
            last_error: None,
            next_iteration_at: None,
        });
    }
    #[cfg(not(feature = "server"))]
    {
        Ok(SchedulerStatusDto {
            started: false,
            running: false,
            db_url: String::new(),
            last_loop_at: None,
            last_event: None,
            last_error: None,
            next_iteration_at: None,
        })
    }
}

#[server(GetDailyUsage)]
pub async fn get_daily_usage() -> Result<Vec<DailyUsagePointDto>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::backend::GLOBAL_DB;
        use chrono::{Duration, Utc};
        let Some(db) = GLOBAL_DB.get() else {
            eprintln!("get_daily_usage: DB not initialized");
            return Ok(vec![]);
        };

        let since = Utc::now() - Duration::days(90);
        let mut rows = match db.get_rows_since(since).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("get_daily_usage query error: {e}");
                return Ok(vec![]);
            }
        };

        // Sort rows by timestamp ascending to make choosing the last sample per day easy
        rows.sort_by_key(|r| r.date_time);

        // For each day, keep the LAST reading (latest timestamp) of remaining_data_mb
        use std::collections::BTreeMap;
        let mut last_by_day: BTreeMap<String, (chrono::DateTime<Utc>, i32)> = BTreeMap::new();
        for r in rows.into_iter() {
            let day = r.date_time.date_naive().to_string();
            match last_by_day.get(&day) {
                Some((ts, _)) if r.date_time <= *ts => {
                    // keep existing (we want the last of the day)
                }
                _ => {
                    last_by_day.insert(day, (r.date_time, r.remaining_data_mb));
                }
            }
        }

        // Build output for last 90 days in order, computing usage as prev_day_remaining - curr_day_remaining when both exist
        let mut filled = Vec::new();
        let mut prev_remaining: Option<i32> = None;
        for i in (0..90).rev() {
            let d = (Utc::now() - Duration::days(i)).date_naive().to_string();
            let curr_remaining = last_by_day.get(&d).map(|(_, v)| *v);
            let used = match (prev_remaining, curr_remaining) {
                (Some(prev), Some(curr)) => {
                    let diff = prev - curr;
                    if diff > 0 {
                        diff
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            if let Some(curr) = curr_remaining {
                // eprintln!(
                //     "get_daily_usage: {d}: prev={} curr={} used={}",
                //     prev_remaining.unwrap_or(-1),
                //     curr,
                //     used
                // );
                prev_remaining = Some(curr);
            } else {
                // eprintln!(
                //     "get_daily_usage: {d}: prev={} curr=NA used=0",
                //     prev_remaining.unwrap_or(-1)
                // );
                // do not update prev_remaining when there's no reading for this day
            }
            filled.push(DailyUsagePointDto {
                date: d,
                used_mb: used,
            });
        }
        return Ok(filled);
    }
    #[cfg(not(feature = "server"))]
    {
        Ok(vec![])
    }
}
