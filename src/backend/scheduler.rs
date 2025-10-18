#![cfg(feature = "server")]
use crate::backend::{db, windtre};
use once_cell::sync::OnceCell;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

pub static SCHED_INTERVAL_MINUTES: u64 = 60;

pub static SCHED_HANDLE: OnceCell<Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>> =
    OnceCell::new();
pub static STATUS: OnceCell<Arc<RwLock<SchedulerState>>> = OnceCell::new();

#[derive(Debug, Clone, Default, Serialize)]
pub struct SchedulerState {
    pub started: bool,
    pub db_url: String,
    pub last_loop_at: Option<String>,
    pub last_event: Option<String>,
    pub last_error: Option<String>,
    pub next_iteration_at: Option<String>,
}

pub async fn scheduler_task(db: Arc<db::Db>) {
    use chrono::{Timelike, Utc};
    use tokio::time::{timeout, Duration, Instant};

    eprintln!("[scheduler] background task started");
    if let Err(_elapsed) = timeout(Duration::from_secs(10), scheduler_run_once(&db)).await {
        eprintln!("[scheduler] initial run timed out; continuing to schedule");
        // set in status
        if let Some(st) = STATUS.get() {
            let mut w = st.write().await;
            w.last_error = Some("initial run timed out".into());
        }
    }
    let interval_secs = SCHED_INTERVAL_MINUTES * 60;
    let now = Utc::now();
    let secs_in_hour = (now.minute() as u64) * 60 + (now.second() as u64);
    let rem = secs_in_hour % interval_secs;
    let next_delay_secs = if rem == 0 {
        interval_secs
    } else {
        interval_secs - rem
    };
    let mins_until = (next_delay_secs + 59) / 60;
    // Compute and store next iteration timestamp
    let next_ts = (Utc::now() + chrono::Duration::seconds(next_delay_secs as i64)).to_rfc3339();
    if let Some(st) = STATUS.get() {
        let mut w = st.write().await;
        w.next_iteration_at = Some(next_ts.clone());
    }
    eprintln!(
        "[scheduler] next run in {} minute(s); cadence every {} minute(s)",
        mins_until, SCHED_INTERVAL_MINUTES
    );
    let start = Instant::now() + Duration::from_secs(next_delay_secs);
    let mut interval = tokio::time::interval_at(start, Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;

        // Run the scheduled task
        scheduler_run_once(&db).await;

        // Update next_iteration_at for the following tick
        if let Some(st) = STATUS.get() {
            let mut w = st.write().await;
            let next = Utc::now() + chrono::Duration::seconds(interval_secs as i64);
            w.next_iteration_at = Some(next.to_rfc3339());
        }
    }
}

pub async fn scheduler_run_once(db: &Arc<db::Db>) {
    eprintln!("[scheduler] run start");
    use chrono::{Duration as ChronoDuration, Utc};
    use windtre::{get_data_status_fresh, DataStatus};
    if let Some(st) = STATUS.get() {
        let mut s = st.write().await;
        s.last_loop_at = Some(Utc::now().to_rfc3339());
        s.last_event = Some("polling for data status".into());
    }
    let result = get_data_status_fresh(
        false,
        ChronoDuration::minutes((SCHED_INTERVAL_MINUTES - 1) as i64),
        ChronoDuration::seconds(30),
        ChronoDuration::seconds(2),
    )
    .await;
    match result {
        Ok(windtre::GetDataStatusEvent::Fresh {
            data_status:
                DataStatus {
                    remaining_percentage,
                    remaining_data_mb,
                    date_time,
                },
        }) => {
            eprintln!(
                "[scheduler] fresh data: {}% ({} MB) at {} (age: {} min)",
                remaining_percentage,
                remaining_data_mb,
                date_time,
                (Utc::now() - date_time).num_minutes()
            );
            if let Err(e) = db
                .insert_data_status(remaining_percentage, remaining_data_mb, date_time)
                .await
            {
                eprintln!("[scheduler] db insert error: {e}");
                if let Some(st) = STATUS.get() {
                    let mut w = st.write().await;
                    w.last_error = Some(format!("db insert error: {e}"));
                }
            } else if let Some(st) = STATUS.get() {
                let mut w = st.write().await;
                w.last_event = Some("stored fresh data".into());
            }
        }
        Ok(windtre::GetDataStatusEvent::Loading {
            data_status: _,
            is_stale,
        }) => {
            eprintln!("[scheduler] loading... stale={}", is_stale);
            if let Some(st) = STATUS.get() {
                let mut w = st.write().await;
                w.last_event = Some(format!("loading (stale={})", is_stale));
            }
        }
        Ok(windtre::GetDataStatusEvent::Error {
            error,
            data_status: _,
            is_stale,
        }) => {
            eprintln!("[scheduler] error: {} (stale={})", error, is_stale);
            if let Some(st) = STATUS.get() {
                let mut w = st.write().await;
                w.last_error = Some(format!("{}", error));
                w.last_event = Some(format!("error (stale={})", is_stale));
            }
        }
        Err(e) => {
            eprintln!("[scheduler] unexpected error: {e}");
            if let Some(st) = STATUS.get() {
                let mut w = st.write().await;
                w.last_error = Some(format!("unexpected error: {e}"));
            }
        }
    }
    eprintln!("[scheduler] run complete");
}

pub async fn ensure_scheduler_started_with(db: Arc<db::Db>, db_url: String) -> anyhow::Result<()> {
    let handle_cell = SCHED_HANDLE.get_or_init(|| Arc::new(RwLock::new(None)));
    {
        let h_opt = handle_cell.read().await;
        if let Some(h) = &*h_opt {
            if !h.is_finished() {
                return Ok(());
            }
        }
    }
    if STATUS.get().is_none() {
        let _ = STATUS.set(Arc::new(RwLock::new(SchedulerState {
            started: false,
            db_url: db_url.clone(),
            ..Default::default()
        })));
    }
    eprintln!("[scheduler] starting with DB: {}", db_url);
    let handle = tokio::spawn(scheduler_task(db));
    {
        let mut h_opt = handle_cell.write().await;
        *h_opt = Some(handle);
    }
    if let Some(st) = STATUS.get() {
        let mut w = st.write().await;
        w.started = true;
    }
    Ok(())
}

// --- Test data generator (server) ---
pub async fn generate_test_data(db: Arc<db::Db>, plan_total_mb: i32) -> anyhow::Result<()> {
    use chrono::{Datelike, Duration, Utc};
    use dotenvy::dotenv;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    dotenv().ok();
    let mut rng = StdRng::seed_from_u64(42);
    let total = plan_total_mb.max(1024 * 10);
    let now = Utc::now();
    let mut day = (now - Duration::days(90)).date_naive();
    let end_day = now.date_naive();
    let mut remaining: i32 = if day.day() == 1 {
        total
    } else {
        rng.gen_range((total as f32 * 0.3) as i32..=total)
    };
    while day <= end_day {
        let mut sample_times: Vec<chrono::DateTime<Utc>> = Vec::new();
        if day.day() == 1 {
            remaining = total;
            let reset_min = rng.gen_range(0..=29);
            let reset_sec = rng.gen_range(0..=59);
            let reset_naive = day
                .and_hms_opt(0, reset_min, reset_sec)
                .expect("valid midnight time");
            let reset_dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(reset_naive, Utc);
            let pct = ((remaining as f32) / (total as f32) * 100.0).round() as i32;
            if reset_dt <= now {
                let _ = db
                    .insert_data_status(pct.clamp(0, 100), remaining, reset_dt)
                    .await?;
            }
        }
        let k: usize = rng.gen_range(1..=3);
        let mut last_hour: u32 = if day.day() == 1 { 1 } else { 0 };
        for _ in 0..k {
            let remaining_slots = k - sample_times.len();
            let max_hour_cap = 23u32.saturating_sub((remaining_slots as u32 - 1) * 2);
            let hour = rng.gen_range(last_hour..=max_hour_cap.max(last_hour));
            let minute = rng.gen_range(0..=59);
            let second = rng.gen_range(0..=59);
            last_hour = hour.saturating_add(2).min(23);
            let naive = day
                .and_hms_opt(hour, minute, second)
                .expect("valid time for day");
            let ts = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            sample_times.push(ts);
        }
        let roll: u8 = rng.gen_range(0..100);
        let mut daily_usage_mb: i32 = if roll < 55 {
            rng.gen_range(0..=2_000)
        } else if roll < 85 {
            rng.gen_range(2_000..=5_000)
        } else if roll < 98 {
            rng.gen_range(5_000..=8_000)
        } else {
            rng.gen_range(8_000..=10_240)
        };
        if daily_usage_mb > remaining {
            daily_usage_mb = remaining;
        }
        let mut remaining_usage = daily_usage_mb;
        for (i, ts) in sample_times.into_iter().enumerate() {
            if ts > now {
                continue;
            }
            let drop_i: i32 = if i + 1 == k {
                remaining_usage
            } else {
                let even_share = (remaining_usage / ((k - i) as i32)).max(0);
                rand::Rng::gen_range(&mut rng, 0..=even_share)
            };
            remaining_usage = remaining_usage.saturating_sub(drop_i);
            remaining = remaining.saturating_sub(drop_i);
            let pct = ((remaining as f32) / (total as f32) * 100.0).round() as i32;
            let _ = db
                .insert_data_status(pct.clamp(0, 100), remaining, ts)
                .await?;
        }
        if let Some(next) = day.checked_add_signed(chrono::Duration::days(1)) {
            day = next;
        } else {
            break;
        }
    }
    eprintln!(
        "Inserted synthetic data for ~90 days ending at {} (monthly reset to {} MB)",
        end_day, total
    );
    Ok(())
}
