#![cfg(feature = "server")]

pub mod db;
pub mod mikrotik;
pub mod scheduler;
pub mod windtre;

pub use crate::backend::db::GLOBAL_DB;
pub use db::Db;
pub use scheduler::{ensure_scheduler_started_with, SCHED_INTERVAL_MINUTES};

pub use tracing_subscriber::{fmt, prelude::*, util::SubscriberInitExt, EnvFilter};

pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx::query=off,sqlx::query::describe=off"));
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}
