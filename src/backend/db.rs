#![cfg(feature = "server")]
use anyhow::Result;
use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::str::FromStr;
use std::sync::Arc;

pub static GLOBAL_DB: OnceCell<Arc<Db>> = OnceCell::new();

pub fn resolve_db_url() -> String {
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

#[derive(Debug, Clone)]
pub struct Db {
    pool: Pool<Sqlite>,
}

#[derive(Debug, Clone)]
pub struct DataStatusRow {
    #[allow(dead_code)]
    pub id: i64,
    pub remaining_percentage: i32,
    pub remaining_data_mb: i32,
    pub date_time: DateTime<Utc>,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await?;
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS data_status (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                remaining_percentage INTEGER NOT NULL,
                remaining_data_mb INTEGER NOT NULL,
                date_time TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_data_status(
        &self,
        remaining_percentage: i32,
        remaining_data_mb: i32,
        date_time: DateTime<Utc>,
    ) -> Result<i64> {
        let created_at = Utc::now();
        let rec = sqlx::query(
            r#"INSERT OR IGNORE INTO data_status
            (remaining_percentage, remaining_data_mb, date_time, created_at)
            VALUES (?1, ?2, ?3, ?4)"#,
        )
        .bind(remaining_percentage)
        .bind(remaining_data_mb)
        .bind(date_time.to_rfc3339())
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(rec.last_insert_rowid())
    }

    pub async fn get_latest_data_status(&self) -> Result<Option<DataStatusRow>> {
        let row = sqlx::query(
            r#"SELECT id, remaining_percentage, remaining_data_mb, date_time, created_at
            FROM data_status ORDER BY date_time DESC LIMIT 1"#,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            let id: i64 = r.try_get("id")?;
            let remaining_percentage: i32 = r.try_get("remaining_percentage")?;
            let remaining_data_mb: i32 = r.try_get("remaining_data_mb")?;
            let date_time_str: String = r.try_get("date_time")?;
            let created_at_str: String = r.try_get("created_at")?;

            let date_time =
                DateTime::parse_from_rfc3339(&date_time_str).map(|dt| dt.with_timezone(&Utc))?;
            let created_at =
                DateTime::parse_from_rfc3339(&created_at_str).map(|dt| dt.with_timezone(&Utc))?;

            Ok(Some(DataStatusRow {
                id,
                remaining_percentage,
                remaining_data_mb,
                date_time,
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_rows_since(&self, since: DateTime<Utc>) -> Result<Vec<DataStatusRow>> {
        let rows = sqlx::query(
            r#"SELECT id, remaining_percentage, remaining_data_mb, date_time, created_at
            FROM data_status
            WHERE date_time >= ?1
            ORDER BY date_time ASC"#,
        )
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let id: i64 = r.try_get("id")?;
            let remaining_percentage: i32 = r.try_get("remaining_percentage")?;
            let remaining_data_mb: i32 = r.try_get("remaining_data_mb")?;
            let date_time_str: String = r.try_get("date_time")?;
            let created_at_str: String = r.try_get("created_at")?;

            let date_time =
                DateTime::parse_from_rfc3339(&date_time_str).map(|dt| dt.with_timezone(&Utc))?;
            let created_at =
                DateTime::parse_from_rfc3339(&created_at_str).map(|dt| dt.with_timezone(&Utc))?;

            out.push(DataStatusRow {
                id,
                remaining_percentage,
                remaining_data_mb,
                date_time,
                created_at,
            });
        }
        Ok(out)
    }
}
