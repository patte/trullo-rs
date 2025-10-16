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
    // If invoked with CLI subcommands, handle them in server builds
    #[cfg(feature = "server")]
    {
        let mut args = std::env::args();
        let _bin = args.next();
        if let Some(cmd) = args.next() {
            if cmd == "gen-test-data" {
                // optional: plan total (MB). Default to 100 GB per story
                let plan_total_mb = args
                    .next()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(102_400);
                // block_on small runtime
                let rt = tokio::runtime::Runtime::new().expect("rt");
                rt.block_on(async move {
                    if let Err(e) = generate_test_data(plan_total_mb).await {
                        eprintln!("error generating test data: {e}");
                        std::process::exit(1);
                    }
                });
                return;
            }
            if cmd == "import-sms" {
                // Import all Mikrotik SMS that look like WindTre data status into the DB
                let rt = tokio::runtime::Runtime::new().expect("rt");
                rt.block_on(async move {
                    use dotenvy::dotenv;
                    dotenv().ok();
                    let db_url = resolve_db_url();
                    match db::Db::connect(&db_url).await {
                        Ok(db) => {
                            match mikrotik::get_smses().await {
                                Ok(smss) => {
                                    let mut total = 0usize;
                                    let mut inserted = 0usize;
                                    for sms in smss.iter() {
                                        total += 1;
                                        if let Some(ds) = windtre::parse_data_status_from_sms(sms) {
                                            match db
                                                .insert_data_status(
                                                    ds.remaining_percentage,
                                                    ds.remaining_data_mb,
                                                    ds.date_time,
                                                )
                                                .await
                                            {
                                                Ok(_rowid) => {
                                                    // rowid will be 0 on IGNORE; we still count as processed, not necessarily inserted
                                                    if _rowid != 0 {
                                                        inserted += 1;
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!(
                                                        "import-sms: db insert error for {}: {}",
                                                        ds.date_time, e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    eprintln!(
                                        "import-sms: processed {} SMS, inserted {} new records",
                                        total, inserted
                                    );
                                }
                                Err(e) => {
                                    eprintln!("import-sms: failed to fetch SMS: {e}");
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("import-sms: failed to connect DB: {e}");
                            std::process::exit(1);
                        }
                    }
                });
                return;
            }
        }
    }
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
        // Page container
        div { class: "min-h-screen bg-slate-950 text-slate-100 p-6 space-y-6",
            // Centered card (max-w-xl)
            div { class: "w-full max-w-xl mx-auto",
                DataStatusCard {}
            }
            // Full-width chart section
            div { class: "w-full max-w-6xl mx-auto",
                UsageChartView {}
            }
        }
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
        // sleep until next iteration
        let now = Utc::now();
        let next_hour = (now + Duration::minutes(5))
            .with_minute(0)
            .and_then(|d| d.with_second(0))
            .and_then(|d| d.with_nanosecond(0))
            .unwrap_or(now + Duration::minutes(5));
        let sleep_dur = (next_hour - now)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(3600));
        tokio::time::sleep(sleep_dur).await;
    }
}

#[cfg(feature = "server")]
async fn ensure_scheduler_started() -> anyhow::Result<()> {
    use dotenvy::dotenv;
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
fn DataStatusCard() -> Element {
    let latest = use_resource(|| async move {
        start_scheduler().await.ok();
        latest_data_status().await.ok().flatten()
    });
    let status = use_resource(|| async move { get_scheduler_status().await.ok() });
    rsx! {
            // Card
            div { class: "w-full rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-8 space-y-6",
                h1 { class: "text-2xl font-semibold tracking-tight text-slate-200", "WindTre Data Status" }
                {
                    let data = latest.read_unchecked();
                    match &*data {
                        Some(Some(ds)) => rsx! {
                            div { class: "flex flex-col items-center gap-3",
                                // Gauge + percentage
                                Gauge {
                                    value: ds.remaining_percentage,
                                    start_angle: 45.0,
                                    stop_angle: 315.0,
                                    size: 220,
                                    stroke: 14,
                                    track_class: "text-slate-800".to_string(),
                                    progress_class: "text-emerald-400".to_string(),
                                    div { class: "text-5xl font-bold text-emerald-400 tabular-nums", "{ds.remaining_percentage}%" }
                                }
                                // MB remaining
                                div { class: "text-lg text-slate-300", "{format_megabytes(ds.remaining_data_mb)} remaining" }
                                // Timestamp formatted in local time
                                div { class: "text-xs text-slate-400", "As of {format_local(&ds.date_time)}" }
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
                                if let Some(ts) = &st.last_loop_at { div { "Last loop: {format_local(ts)}" } }
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

// Human-readable size formatting for megabytes (SI units: 1000 MB = 1 GB)
fn format_megabytes(mb: i32) -> String {
    if mb.abs() >= 1_000 {
        let gb = mb as f64 / 1_000.0;
        if (gb.fract()).abs() < f64::EPSILON || (gb * 10.0).round() % 10.0 == 0.0 {
            // Near integer or first decimal is 0 -> no decimal
            format!("{} GB", gb.round() as i32)
        } else {
            format!("{:.1} GB", gb)
        }
    } else {
        format!("{} MB", mb)
    }
}

fn format_megabytes_f32(mb: f32) -> String {
    if mb.abs() >= 1_000.0 {
        let gb = mb as f64 / 1_000.0;
        if (gb.fract()).abs() < f64::EPSILON || (gb * 10.0).round() % 10.0 == 0.0 {
            format!("{} GB", gb.round() as i32)
        } else {
            format!("{:.1} GB", gb)
        }
    } else {
        format!("{} MB", mb.round() as i32)
    }
}

// --- Reusable SVG Gauge component ---
#[allow(non_snake_case)]
#[component]
fn Gauge(
    value: i32,
    start_angle: f32,
    stop_angle: f32,
    size: i32,
    stroke: i32,
    track_class: String,
    progress_class: String,
    children: Element,
) -> Element {
    // Normalize & clamp
    let val = value.clamp(0, 100) as f32;
    let span = (stop_angle - start_angle).abs().max(0.0001);
    let end_angle = start_angle + span * (val / 100.0);

    let c = (size as f32) / 2.0;
    let r = c - (stroke as f32) / 2.0 - 1.0; // small padding

    // Helpers to make arc paths
    fn to_rad(deg: f32) -> f32 {
        deg.to_radians()
    }
    fn polar(cx: f32, cy: f32, r: f32, ang: f32) -> (f32, f32) {
        let rad = to_rad(ang);
        (cx + r * rad.cos(), cy + r * rad.sin())
    }
    fn arc_path(cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> String {
        let (x0, y0) = polar(cx, cy, r, a0);
        let (x1, y1) = polar(cx, cy, r, a1);
        let delta = (a1 - a0).abs();
        let large_arc = if delta >= 180.0 { 1 } else { 0 };
        let sweep = if a1 >= a0 { 1 } else { 0 };
        format!("M {x0:.3} {y0:.3} A {r:.3} {r:.3} 0 {large_arc} {sweep} {x1:.3} {y1:.3}")
    }

    // Rotate gauge 90 degrees clockwise for more natural orientation
    let angle_offset = 90.0;
    let start0 = start_angle + angle_offset;
    let stop0 = stop_angle + angle_offset;
    let end0 = end_angle + angle_offset;

    let track_d = arc_path(c, c, r, start0, stop0);
    let progress_d = arc_path(c, c, r, start0, end0);

    let size_attr = size.to_string();
    let view_box = format!("0 0 {size} {size}");
    let stroke_width = stroke.to_string();
    let container_style = format!("width:{size}px;height:{size}px");

    rsx! {
        div { class: "relative", style: "{container_style}",
            svg { width: "{size_attr}", height: "{size_attr}", view_box: "{view_box}",
                // Track
                path { class: "{track_class}", d: "{track_d}", fill: "none", stroke: "currentColor", stroke_width: "{stroke_width}", stroke_linecap: "round" }
                // Progress
                path { class: "{progress_class}", d: "{progress_d}", fill: "none", stroke: "currentColor", stroke_width: "{stroke_width}", stroke_linecap: "round" }
            }
            // Center content
            div { class: "absolute inset-0 grid place-items-center", {children} }
        }
    }
}

// --- Daily usage API and chart ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyUsagePointDto {
    date: String, // yyyy-mm-dd
    used_mb: i32, // usage within that day
}

#[server(GetDailyUsage)]
async fn get_daily_usage() -> Result<Vec<DailyUsagePointDto>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use chrono::{Duration, Utc};
        use dotenvy::dotenv;
        dotenv().ok();
        let db_url = resolve_db_url();
        let db = match db::Db::connect(&db_url).await {
            Ok(db) => db,
            Err(e) => {
                eprintln!("get_daily_usage connect error: {e}");
                return Ok(vec![]);
            }
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

#[allow(non_snake_case)]
#[component]
fn UsageChartView() -> Element {
    // Fetch data
    let data = use_resource(|| async move { get_daily_usage().await.ok().unwrap_or_default() });
    let points = data.read_unchecked().clone().unwrap_or_default();
    // Hovered bar index (for tooltip)
    let mut hovered = use_signal(|| Option::<usize>::None);
    // Visual params
    let height = 180.0f32;
    let padding = 20.0f32;
    let bar_gap = 2.0f32;
    let n = points.len().max(1) as f32;
    let width = (n * (6.0 + bar_gap) + padding * 2.0).ceil();
    let max_used = points.iter().map(|p| p.used_mb).max().unwrap_or(1) as f32;
    let view_box = format!("0 0 {} {}", width, height + padding * 2.0);

    // Date formatter for tooltip (yyyy-mm-dd -> dd.mm.yyyy)
    let fmt_date = |s: &str| -> String {
        if s.len() >= 10 {
            format!("{}.{}.{}", &s[8..10], &s[5..7], &s[0..4])
        } else {
            s.to_string()
        }
    };

    // Month label iterator will track seen months internally
    use std::collections::HashSet;

    rsx! {
        div { class: "rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-6 space-y-3",
            div { class: "flex items-end justify-between",
                h2 { class: "text-lg font-medium text-slate-200", "Daily usage (last 90 days)" }
                    if max_used > 0.0 { div { class: "text-xs text-slate-400", "Peak: {format_megabytes_f32(max_used)}" } }
            }
            div { class: "w-full overflow-x-auto",
                svg { class: "block min-w-full", view_box: "{view_box}", width: "100%", height: "{(height + padding*2.0).to_string()}",
                    // subtle grid
                    line { x1: "{padding}", y1: "{padding}", x2: "{width - padding}", y2: "{padding}", stroke: "#1f2937", stroke_width: "1" }
                    line { x1: "{padding}", y1: "{padding + height}", x2: "{width - padding}", y2: "{padding + height}", stroke: "#1f2937", stroke_width: "1" }
                    {
                        points.iter().enumerate().map(|(i, p)| {
                            let x = padding + (i as f32) * (6.0 + bar_gap);
                            let h = if max_used <= 0.0 { 0.0 } else { (p.used_mb as f32) / max_used * height };
                            let y = padding + (height - h);
                            let cls = if p.used_mb == 0 { "text-slate-800" } else { "text-emerald-400/80" };
                            rsx!{
                                rect {
                                    key: "{i}",
                                    class: "{cls}",
                                    x: "{x}",
                                    y: "{y}",
                                    width: "6",
                                    height: "{h}",
                                    fill: "currentColor",
                                    rx: "2",
                                    onmouseenter: move |_| *hovered.write() = Some(i),
                                    onmouseleave: move |_| *hovered.write() = None,
                                    ontouchstart: move |_| *hovered.write() = Some(i),
                                    ontouchend: move |_| *hovered.write() = None,
                                }
                            }
                        })
                    }
                    {
                        // SVG tooltip overlay when hovering a bar
                        match *hovered.read() {
                            Some(i) => {
                                let p = &points[i];
                                let x = padding + (i as f32) * (6.0 + bar_gap) + 3.0; // center of bar
                                let h = if max_used <= 0.0 { 0.0 } else { (p.used_mb as f32) / max_used * height };
                                let y = padding + (height - h);
                                // Dynamic width based on text length; two-line content to avoid overflow
                                let date_label = fmt_date(&p.date);
                                let value_label = format!("{}", format_megabytes(p.used_mb));
                                let cw = 7.0f32; // approx char width at 11px
                                let content_w = (date_label.len().max(value_label.len()) as f32) * cw + 12.0; // padding
                                let tip_w = content_w.max(12.0).min(width - padding * 2.0);
                                let tip_h = 36.0f32; // two lines
                                let tip_x = (x - tip_w / 2.0).clamp(padding, (width - padding) - tip_w);
                                let tip_y = (y - 10.0 - tip_h).max(6.0);
                                rsx!{
                                    g { key: "tooltip",
                                        // connector
                                        line { x1: "{x}", y1: "{y}", x2: "{x}", y2: "{tip_y + tip_h}", stroke: "#10b981", stroke_width: "1" }
                                        // bubble
                                        rect { x: "{tip_x}", y: "{tip_y}", width: "{tip_w}", height: "{tip_h}", rx: "6", fill: "#0f172a", stroke: "#334155", stroke_width: "1" }
                                        text { x: "{tip_x + 8.0}", y: "{tip_y + 16.0}", class: "fill-current text-[11px] text-slate-300", "{date_label}" }
                                        text { x: "{tip_x + 8.0}", y: "{tip_y + 30.0}", class: "fill-current text-[11px] text-slate-200", "{value_label}" }
                                    }
                                }
                            }
                            None => rsx!{ Fragment {} }
                        }
                    }
                    {
                        points
                            .iter()
                            .enumerate()
                            .scan(HashSet::<String>::new(), |printed, (i, p)| {
                                if p.date.len() >= 7 {
                                    let m = &p.date[..7];
                                    if printed.insert(m.to_string()) {
                                        let x = padding + (i as f32) * (6.0 + bar_gap);
                                        let node = rsx!{ text { x: "{x}", y: "{height + padding + 14.0}", class: "text-slate-400 fill-current text-[10px]", "{m}" } };
                                        return Some(Some(node));
                                    }
                                }
                                Some(None)
                            })
                            .filter_map(|x| x)
                    }
                }
            }
            div { class: "text-xs text-slate-500", "Tip: bars are scaled to the highest day; zeros are hidden in a muted tone." }
        }
    }
}

// --- Test data generator (server) ---
#[cfg(feature = "server")]
async fn generate_test_data(plan_total_mb: i32) -> anyhow::Result<()> {
    use chrono::{Datelike, Duration, Utc};
    use dotenvy::dotenv;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    dotenv().ok();
    let db_url = resolve_db_url();
    let db = db::Db::connect(&db_url).await?;
    let mut rng = StdRng::seed_from_u64(42);

    // Total monthly allowance (e.g., 100 GB)
    let total = plan_total_mb.max(1024 * 10); // ensure at least 10 GB

    let now = Utc::now();
    let mut day = (now - Duration::days(90)).date_naive();
    let end_day = now.date_naive();

    // Choose a plausible starting remaining if we're mid-month; else start full on the 1st
    let mut remaining: i32 = if day.day() == 1 {
        total
    } else {
        // Start somewhere between 30% and 100% of the plan
        rng.gen_range((total as f32 * 0.3) as i32..=total)
    };

    while day <= end_day {
        // let day_start = NaiveDateTime::new(day, chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let mut sample_times: Vec<chrono::DateTime<Utc>> = Vec::new();

        // Monthly reset at the very start of each calendar month
        if day.day() == 1 {
            remaining = total;
            let reset_min = rng.gen_range(0..=29);
            let reset_sec = rng.gen_range(0..=59);
            let reset_naive = day
                .and_hms_opt(0, reset_min, reset_sec)
                .expect("valid midnight time");
            let reset_dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(reset_naive, Utc);
            // Record the reset reading (100%)
            let pct = ((remaining as f32) / (total as f32) * 100.0).round() as i32;
            if reset_dt <= now {
                let _ = db
                    .insert_data_status(pct.clamp(0, 100), remaining, reset_dt)
                    .await?;
            }
            // Ensure subsequent samples come after the reset
        }

        // Decide number of samples for the day (scheduler would run ~hourly; we simulate a few observations)
        let k: usize = rng.gen_range(1..=3);
        // Generate monotonically increasing times during the day
        let mut last_hour: u32 = if day.day() == 1 { 1 } else { 0 };
        for _ in 0..k {
            // Leave some space for the remaining samples
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

        // Determine daily usage profile (MB)
        let roll: u8 = rng.gen_range(0..100);
        let mut daily_usage_mb: i32 = if roll < 55 {
            // light day: 0–2 GB
            rng.gen_range(0..=2_000)
        } else if roll < 85 {
            // moderate: 2–5 GB
            rng.gen_range(2_000..=5_000)
        } else if roll < 98 {
            // heavy: 5–8 GB
            rng.gen_range(5_000..=8_000)
        } else {
            // extreme: 8–10 GB
            rng.gen_range(8_000..=10_240)
        };
        // Can't use more than we have remaining
        if daily_usage_mb > remaining {
            daily_usage_mb = remaining;
        }

        // Spread daily usage across the k samples
        let mut remaining_usage = daily_usage_mb;
        for (i, ts) in sample_times.into_iter().enumerate() {
            if ts > now {
                // Don't create future samples for today
                continue;
            }
            let drop_i: i32 = if i + 1 == k {
                remaining_usage
            } else {
                // up to an even share of what's left, with some variance
                let even_share = (remaining_usage / ((k - i) as i32)).max(0);
                rng.gen_range(0..=even_share)
            };
            remaining_usage = remaining_usage.saturating_sub(drop_i);
            remaining = remaining.saturating_sub(drop_i);

            let pct = ((remaining as f32) / (total as f32) * 100.0).round() as i32;
            let _ = db
                .insert_data_status(pct.clamp(0, 100), remaining, ts)
                .await?;
        }

        // Advance one day safely (compatible with chrono 0.4)
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
