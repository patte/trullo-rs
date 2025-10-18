#![cfg(feature = "server")]
use crate::backend::mikrotik::{get_smses, send_sms, Sms};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct DataStatus {
    pub remaining_percentage: i32,
    pub remaining_data_mb: i32,
    pub date_time: DateTime<Utc>,
}

fn regex() -> &'static Regex {
    static REGEX: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r"Dati: hai ancora a disposizione il (\d+)% di ([\d,]+) GIGA.*").unwrap()
    });
    &REGEX
}

fn parse_sms_message(message: &str, date_time: DateTime<Utc>) -> Option<DataStatus> {
    let re = regex();
    let caps = re.captures(message)?;
    let remaining_percentage: i32 = caps.get(1)?.as_str().parse().ok()?;
    let total_gb_str = caps.get(2)?.as_str().replace(',', ".");
    let total_gb: f64 = total_gb_str.parse().ok()?;
    let total_mb = (total_gb * 1024.0).round() as i32;
    let remaining_data_mb =
        ((remaining_percentage as f64 / 100.0) * total_mb as f64).round() as i32;

    Some(DataStatus {
        remaining_percentage,
        remaining_data_mb,
        date_time,
    })
}

fn sms_date(sms: &Sms) -> Option<DateTime<Utc>> {
    if let Some(ts) = &sms.timestamp {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts) = &sms.received {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(t) = &sms.time {
        let norm = t.replace("Aug", "aug").replace("Sep", "sep");
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&norm, "%b/%d/%Y %H:%M:%S") {
            return Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
        }
    }
    None
}

pub async fn most_recent_data_status() -> Result<Option<DataStatus>> {
    let mut smss = get_smses().await?;
    smss.sort_by_key(|s| sms_date(s).map(|d| d.timestamp()).unwrap_or(0));
    smss.reverse();
    for sms in smss.iter() {
        if let Some(dt) = sms_date(sms) {
            if let Some(ds) = parse_sms_message(&sms.message, dt) {
                return Ok(Some(ds));
            }
        } else {
            eprintln!(
                "[windtre] could not parse date for SMS id={} from={:?}",
                sms.id, sms.from
            );
        }
    }
    Ok(None)
}

pub async fn request_data_status_sms() -> Result<()> {
    send_sms("4155", "Dati").await
}

pub fn parse_data_status_from_sms(sms: &Sms) -> Option<DataStatus> {
    let dt = sms_date(sms)?;
    parse_sms_message(&sms.message, dt)
}

pub enum GetDataStatusEvent {
    Loading {
        #[allow(dead_code)]
        data_status: Option<DataStatus>,
        is_stale: bool,
    },
    Fresh {
        data_status: DataStatus,
    },
    Error {
        error: anyhow::Error,
        #[allow(dead_code)]
        data_status: Option<DataStatus>,
        is_stale: bool,
    },
}

pub async fn get_data_status_fresh(
    force: bool,
    max_age: Duration,
    timeout: Duration,
    poll: Duration,
) -> Result<GetDataStatusEvent> {
    let now = Utc::now();
    let mut current = most_recent_data_status().await?;

    let stale = current
        .as_ref()
        .map(|d| now - d.date_time > max_age)
        .unwrap_or(true);

    if force || stale {
        if let Err(e) = request_data_status_sms().await {
            return Ok(GetDataStatusEvent::Error {
                error: e,
                data_status: current,
                is_stale: true,
            });
        }
        let start = Utc::now();
        loop {
            tokio::time::sleep(poll.to_std().unwrap()).await;
            current = most_recent_data_status().await?;
            if let Some(ds) = &current {
                if now - ds.date_time <= max_age {
                    return Ok(GetDataStatusEvent::Fresh {
                        data_status: ds.clone(),
                    });
                }
            }
            if Utc::now() - start > timeout {
                return Ok(GetDataStatusEvent::Error {
                    error: anyhow::anyhow!("Timeout waiting for new SMS"),
                    data_status: current,
                    is_stale: true,
                });
            }
        }
    } else if let Some(ds) = current {
        Ok(GetDataStatusEvent::Fresh { data_status: ds })
    } else {
        Ok(GetDataStatusEvent::Loading {
            data_status: None,
            is_stale: true,
        })
    }
}
