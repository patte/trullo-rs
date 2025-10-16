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

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Home {},
    #[route("/blog/:id")]
    Blog { id: i32 },
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const HEADER_SVG: Asset = asset!("/assets/header.svg");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[allow(non_snake_case)]
#[component]
fn App() -> Element {
    // Ensure the server-side scheduler is started (no-op on wasm)
    let _ = use_resource(|| async move {
        let _ = start_scheduler().await;
    });
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
    }
}

#[allow(non_snake_case)]
#[component]
pub fn Hero() -> Element {
    rsx! {
        div {
            id: "hero",
            img { src: HEADER_SVG, id: "header" }
            div { id: "links",
                a { href: "https://dioxuslabs.com/learn/0.6/", "📚 Learn Dioxus" }
                a { href: "https://dioxuslabs.com/awesome", "🚀 Awesome Dioxus" }
                a { href: "https://github.com/dioxus-community/", "📡 Community Libraries" }
                a { href: "https://github.com/DioxusLabs/sdk", "⚙️ Dioxus Development Kit" }
                a { href: "https://marketplace.visualstudio.com/items?itemName=DioxusLabs.dioxus", "💫 VSCode Extension" }
                a { href: "https://discord.gg/XgGxMSkvUM", "👋 Community Discord" }
            }
        }
    }
}

/// Home page
#[allow(non_snake_case)]
#[component]
fn Home() -> Element {
    rsx! {
        Hero {}
        Echo {}
        DataStatusView {}
    }
}

/// Blog page
#[allow(non_snake_case)]
#[component]
pub fn Blog(id: i32) -> Element {
    rsx! {
        div {
            id: "blog",

            // Content
            h1 { "This is blog #{id}!" }
            p { "In blog #{id}, we show how the Dioxus router works and how URL parameters can be passed as props to our route components." }

            // Navigation links
            Link {
                to: Route::Blog { id: id - 1 },
                "Previous"
            }
            span { " <---> " }
            Link {
                to: Route::Blog { id: id + 1 },
                "Next"
            }
        }
    }
}

/// Shared navbar component.
#[allow(non_snake_case)]
#[component]
fn Navbar() -> Element {
    rsx! {
        div {
            id: "navbar",
            Link {
                to: Route::Home {},
                "Home"
            }
            Link {
                to: Route::Blog { id: 1 },
                "Blog"
            }
        }

        Outlet::<Route> {}
    }
}

/// Echo component that demonstrates fullstack server functions.
#[allow(non_snake_case)]
#[component]
fn Echo() -> Element {
    let mut response = use_signal(|| String::new());

    rsx! {
        div {
            id: "echo",
            h4 { "ServerFn Echo" }
            input {
                placeholder: "Type here to echo...",
                oninput:  move |event| async move {
                    let data = echo_server(event.value()).await.unwrap();
                    response.set(data);
                },
            }

            if !response().is_empty() {
                p {
                    "Server echoed: "
                    i { "{response}" }
                }
            }
        }
    }
}

/// Echo the user input on the server.
#[server(EchoServer)]
async fn echo_server(input: String) -> Result<String, ServerFnError> {
    Ok(input)
}

// --- Scheduler & DB wiring (server only) ---
#[cfg(feature = "server")]
static STARTED: OnceCell<bool> = OnceCell::new();

#[cfg(feature = "server")]
async fn scheduler_task(db: Arc<db::Db>) {
    use chrono::{Duration, Timelike, Utc};
    use windtre::{get_data_status_fresh, DataStatus};
    loop {
        // attempt to refresh or get current
        let result = get_data_status_fresh(
            false,
            Duration::minutes(75),
            Duration::seconds(30),
            Duration::seconds(2),
        )
        .await;
        if let Ok(windtre::GetDataStatusEvent::Fresh {
            data_status:
                DataStatus {
                    remaining_percentage,
                    remaining_data_mb,
                    date_time,
                },
        }) = result
        {
            let _ = db
                .insert_data_status(remaining_percentage, remaining_data_mb, date_time)
                .await;
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
    rsx! {
        div { id: "data-status",
            h4 { "Latest Data Status" }
            {
                let data = latest.read_unchecked();
                match &*data {
                    Some(Some(ds)) => rsx! {
                        p { "Remaining: {ds.remaining_percentage}% ({ds.remaining_data_mb} MB)" }
                        p { "As of: {ds.date_time}" }
                    },
                    Some(None) => rsx! { p { "No data yet." } },
                    None => rsx! { p { "Loading..." } },
                }
            }
        }
    }
}
