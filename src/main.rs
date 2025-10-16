use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[cfg(feature = "server")]
mod db;
#[cfg(feature = "server")]
mod mikrotik;
#[cfg(feature = "server")]
mod windtre;

#[cfg(feature = "server")]
use once_cell::sync::OnceCell;
#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use tokio::sync::RwLock;

const FAVICON: Asset = asset!("/assets/favicon.ico");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[allow(non_snake_case)]
#[component]
fn App() -> Element {
    // Ensure the server-side scheduler is started (no-op on client)
    let _ = use_resource(|| async move {
        let _ = start_scheduler().await;
    });
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Stylesheet { href: TAILWIND_CSS }
        DataStatusView {}
    }
}

// --- Scheduler & DB wiring (server only) ---
#[cfg(feature = "server")]
static STARTED: OnceCell<bool> = OnceCell::new();
#[cfg(feature = "server")]
static STATUS: OnceCell<Arc<RwLock<SchedulerState>>> = OnceCell::new();

#[cfg(feature = "server")]
#[derive(Debug, Clone, Default, Serialize)]
struct SchedulerState {
    started: bool,
    db_url: String,
    last_loop_at: Option<String>,
    last_event: Option<String>,
    last_error: Option<String>,
}

#[cfg(feature = "server")]
async fn scheduler_task(db: Arc<db::Db>) {
    use chrono::{Duration, Timelike, Utc};
    use windtre::{get_data_status_fresh, DataStatus};
    eprintln!("[scheduler] background task started");
    loop {
        // mark loop start
        #[cfg(feature = "server")]
        if let Some(st) = STATUS.get() {
            let mut s = st.write().await;
            s.last_loop_at = Some(Utc::now().to_rfc3339());
            s.last_event = Some("polling for data status".into());
        }
        // attempt to refresh or get current
        let result = get_data_status_fresh(
            false,
            Duration::minutes(15),
            Duration::seconds(30),
            Duration::seconds(2),
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
                    "[scheduler] fresh data: {}% ({} MB) at {}",
                    remaining_percentage, remaining_data_mb, date_time
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
                } else {
                    if let Some(st) = STATUS.get() {
                        let mut w = st.write().await;
                        w.last_event = Some("stored fresh data".into());
                    }
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
        // sleep until next hour
        let now = Utc::now();
        let next_hour = (now + Duration::hours(1))
            .with_minute(0)
            .and_then(|d| d.with_second(0))
            .and_then(|d| d.with_nanosecond(0))
            .unwrap_or(now + Duration::hours(1));
        let sleep_dur = (next_hour - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(3600));
        tokio::time::sleep(sleep_dur).await;
    }
}

#[cfg(feature = "server")]
async fn ensure_scheduler_started() -> anyhow::Result<()> {
    use dotenvy::dotenv;
    use std::env;
    if STARTED.get().copied().unwrap_or(false) {
        return Ok(());
    }
    dotenv().ok();
    let db_url = resolve_db_url();
    // init status
    let _ = STATUS.set(Arc::new(RwLock::new(SchedulerState {
        started: true,
        db_url: db_url.clone(),
        ..Default::default()
    })));
    eprintln!("[scheduler] starting with DB: {}", db_url);
    let db = db::Db::connect(&db_url).await?;
    let db = Arc::new(db);
    let _ = STARTED.set(true);
    tokio::spawn(scheduler_task(db));
    Ok(())
}

#[server(StartScheduler)]
async fn start_scheduler() -> Result<(), ServerFnError> {
    #[cfg(feature = "server")]
    {
        if let Err(e) = ensure_scheduler_started().await {
            eprintln!("start_scheduler error: {e}");
            // Don't fail the request; just log the error to avoid 500s in dev
        }
    }
    Ok(())
}

// DataStatus DTO for client-side display
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataStatusDto {
    #[serde(rename = "remainingPercentage")]
    remaining_percentage: i32,
    #[serde(rename = "remainingDataMB")]
    remaining_data_mb: i32,
    #[serde(rename = "dateTime")]
    date_time: String,
}

#[server(LatestDataStatus)]
async fn latest_data_status() -> Result<Option<DataStatusDto>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use dotenvy::dotenv;
        use std::env;
        dotenv().ok();
        let db_url = resolve_db_url();
        match db::Db::connect(&db_url).await {
            Ok(db) => match db.get_latest_data_status().await {
                Ok(Some(r)) => {
                    return Ok(Some(DataStatusDto {
                        remaining_percentage: r.remaining_percentage,
                        remaining_data_mb: r.remaining_data_mb,
                        date_time: r.date_time.to_rfc3339(),
                    }))
                }
                Ok(None) => return Ok(None),
                Err(e) => {
                    eprintln!("latest_data_status query error: {e}");
                    return Ok(None);
                }
            },
            Err(e) => {
                eprintln!("latest_data_status connect error: {e}");
                return Ok(None);
            }
        }
    }
    #[cfg(not(feature = "server"))]
    {
        Ok(None)
    }
}

// Expose scheduler status to the frontend for diagnostics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerStatusDto {
    started: bool,
    db_url: String,
    last_loop_at: Option<String>,
    last_event: Option<String>,
    last_error: Option<String>,
}

#[server(GetSchedulerStatus)]
async fn get_scheduler_status() -> Result<SchedulerStatusDto, ServerFnError> {
    #[cfg(feature = "server")]
    {
        if let Some(st) = STATUS.get() {
            let s = st.read().await.clone();
            return Ok(SchedulerStatusDto {
                started: s.started,
                db_url: s.db_url,
                last_loop_at: s.last_loop_at,
                last_event: s.last_event,
                last_error: s.last_error,
            });
        }
        return Ok(SchedulerStatusDto {
            started: false,
            db_url: String::new(),
            last_loop_at: None,
            last_event: Some("not started".into()),
            last_error: None,
        });
    }
    #[cfg(not(feature = "server"))]
    {
        Ok(SchedulerStatusDto {
            started: false,
            db_url: String::new(),
            last_loop_at: None,
            last_event: None,
            last_error: None,
        })
    }
}

// Server-only helper: build a stable, writable sqlite URL and ensure parent dir exists
#[cfg(feature = "server")]
fn resolve_db_url() -> String {
    use std::{env, fs, path::PathBuf};
    if let Ok(url) = env::var("DATABASE_URL") {
        return url;
    }
    // Place DB under project_root/data/data.db
    let root = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(root);
    path.push("data");
    let _ = fs::create_dir_all(&path);
    path.push("data.db");
    // SQLx expects absolute paths in the form sqlite:///abs/path
    let path_str = path.to_string_lossy();
    let trimmed = path_str
        .strip_prefix('/')
        .map(|s| s.to_string())
        .unwrap_or_else(|| path_str.to_string());
    format!("sqlite:///{}?mode=rwc", trimmed)
}

#[allow(non_snake_case)]
#[component]
fn DataStatusView() -> Element {
    let latest = use_resource(|| async move {
        start_scheduler().await.ok();
        latest_data_status().await.ok().flatten()
    });
    let status = use_resource(|| async move { get_scheduler_status().await.ok() });
    rsx! {
        // Full-screen container
        div { class: "min-h-screen bg-slate-950 text-slate-100 flex items-center justify-center p-6",
            // Card
            div { class: "w-full max-w-xl rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-8 space-y-6",
                h1 { class: "text-2xl font-semibold tracking-tight text-slate-200", "WindTre Data Status" }
                {
                    let data = latest.read_unchecked();
                    match &*data {
                        Some(Some(ds)) => rsx! {
                            div { class: "flex flex-col items-center gap-3",
                                // Big percentage
                                div { class: "text-6xl font-bold text-emerald-400 tabular-nums", "{ds.remaining_percentage}%" }
                                // MB remaining
                                div { class: "text-lg text-slate-300", "{ds.remaining_data_mb} MB remaining" }
                                // Timestamp formatted in local time
                                div { class: "text-xs text-slate-400", "As of {format_local(&ds.date_time)} (local time)" }
                            }
                        },
                        Some(None) => rsx! {
                            div { class: "text-center text-slate-300",
                                p { class: "text-lg", "No data yet" }
                                p { class: "text-sm text-slate-400", "Awaiting SMS update from the router..." }
                            }
                        },
                        None => rsx! {
                            div { class: "animate-pulse space-y-3",
                                div { class: "h-9 w-28 bg-slate-800 rounded" }
                                div { class: "h-5 w-48 bg-slate-800 rounded" }
                                div { class: "h-3 w-40 bg-slate-800 rounded" }
                            }
                        },
                    }
                }
                // Diagnostics
                {
                    match &*status.read_unchecked() {
                        Some(Some(st)) => rsx!{
                            div { class: "pt-2 border-t border-slate-800 text-xs text-slate-400 space-y-1",
                                if let Some(err) = &st.last_error { div { class: "text-red-400 text-sm font-medium", "Error: {err}" } }
                                if let Some(ev) = &st.last_event { div { "Status: {ev}" } }
                                if let Some(ts) = &st.last_loop_at { div { "Last loop: {format_local(ts)} (local)" } }
                                // div { class: "truncate", "DB: {st.db_url}" }
                            }
                        },
                        Some(None) => rsx!{ div { class: "text-xs text-slate-500", "No status yet..." } },
                        None => rsx!{ div { class: "text-xs text-slate-500", "Loading status..." } },
                    }
                }
            }
        }
    }
}

// Format an RFC3339 date-time into local time "dd.mm.yyyy HH:MM"
#[allow(unused)]
fn pad2(n: i32) -> String {
    if n < 10 {
        format!("0{}", n)
    } else {
        n.to_string()
    }
}

#[cfg(all(feature = "web"))]
fn format_local(rfc3339: &str) -> String {
    // Use JS Date to handle local timezone on the client
    use js_sys::Date;
    // Safari doesn't parse RFC3339 with timezone space; but our string is ISO 8601, so Date should handle
    let d = Date::new(&wasm_bindgen::JsValue::from_str(rfc3339));
    if d.get_time().is_nan() {
        return rfc3339.to_string();
    }
    let day = d.get_date() as i32;
    let month = (d.get_month() as i32) + 1;
    let year = d.get_full_year() as i32;
    let hour = d.get_hours() as i32;
    let minute = d.get_minutes() as i32;
    format!(
        "{}.{}.{} {}:{}",
        pad2(day),
        pad2(month),
        year,
        pad2(hour),
        pad2(minute)
    )
}

#[cfg(not(all(feature = "web")))]
fn format_local(rfc3339: &str) -> String {
    rfc3339.to_string()
}
