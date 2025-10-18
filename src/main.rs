use dioxus::prelude::*;

mod api;
mod app;
mod components;
mod shared;
mod utils;

#[cfg(feature = "server")]
mod backend;

pub const FAVICON: Asset = asset!("/assets/favicon.ico");
pub const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

#[cfg(feature = "server")]
use std::sync::Arc;

fn main() {
    #[cfg(feature = "server")]
    {
        backend::init_tracing();

        // Initialize the global DB once at boot
        let db_url = backend::db::resolve_db_url();
        {
            use dotenvy::dotenv;
            dotenv().ok();
            let rt = tokio::runtime::Runtime::new().expect("rt");
            rt.block_on(async {
                match backend::Db::connect(&db_url).await {
                    Ok(db) => {
                        let _ = backend::GLOBAL_DB.set(Arc::new(db));
                        eprintln!("[db] initialized");
                    }
                    Err(e) => {
                        eprintln!("[db] failed to init: {e}");
                    }
                }
            });
        }

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
                    let Some(db) = backend::GLOBAL_DB.get() else {
                        eprintln!("[gen-test-data] GLOBAL_DB not initialized");
                        std::process::exit(1);
                    };
                    if let Err(e) =
                        backend::scheduler::generate_test_data(db.clone(), plan_total_mb).await
                    {
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
                    let Some(db) = backend::GLOBAL_DB.get() else {
                        eprintln!("[import-sms] GLOBAL_DB not initialized");
                        std::process::exit(1);
                    };
                    match backend::mikrotik::get_smses().await {
                        Ok(smss) => {
                            let mut total = 0usize;
                            let mut inserted = 0usize;
                            for sms in smss.iter() {
                                total += 1;
                                if let Some(ds) = backend::windtre::parse_data_status_from_sms(sms)
                                {
                                    match db
                                        .insert_data_status(
                                            ds.remaining_percentage,
                                            ds.remaining_data_mb,
                                            ds.date_time,
                                        )
                                        .await
                                    {
                                        Ok(rowid) => {
                                            if rowid != 0 {
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
                            eprintln!("import-sms: processed {}, inserted {}", total, inserted);
                        }
                        Err(e) => {
                            eprintln!("import-sms: failed to fetch SMS: {e}");
                            std::process::exit(1);
                        }
                    }
                });
                return;
            }
        }

        let _ = std::thread::Builder::new()
            .name("scheduler-rt".into())
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("scheduler rt");
                rt.block_on(async {
                    if let Some(db) = backend::GLOBAL_DB.get() {
                        if let Err(e) =
                            backend::ensure_scheduler_started_with(db.clone(), db_url).await
                        {
                            eprintln!("[scheduler] failed to start at boot: {e}");
                        }
                    } else {
                        eprintln!("[scheduler] GLOBAL_DB not initialized; scheduler not started");
                    }
                    // keep this runtime alive forever
                    futures::future::pending::<()>().await;
                });
            })
            .expect("spawn scheduler thread");
    }
    dioxus::launch(app::App);
}
