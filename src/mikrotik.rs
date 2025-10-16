#![cfg(feature = "server")]
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::env;

static CLIENT: Lazy<Client> = Lazy::new(|| Client::builder().build().expect("client"));

fn base_url() -> Result<String> {
    env::var("MIKROTIK_URL").map_err(|_| anyhow!("MIKROTIK_URL not set"))
}

fn auth_header() -> Result<String> {
    if let Ok(b64) = env::var("MIKROTIK_AUTH_BASE64") {
        Ok(format!("Basic {}", b64))
    } else if let (Ok(user), Ok(pass)) = (env::var("MIKROTIK_USER"), env::var("MIKROTIK_PASS")) {
        Ok(format!(
            "Basic {}",
            base64::encode(format!("{}:{}", user, pass))
        ))
    } else {
        Err(anyhow!(
            "Set MIKROTIK_AUTH_BASE64 or MIKROTIK_USER and MIKROTIK_PASS"
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sms {
    pub id: String,
    pub message: String,
    pub timestamp: String, // ISO 8601 string
}

pub async fn fetch_mikrotik<T: for<'de> Deserialize<'de> + Send + 'static>(
    path: &str,
    method: Method,
    body: Option<serde_json::Value>,
) -> Result<T> {
    let url = format!("{}{}", base_url()?, path);
    let auth = auth_header()?;
    let mut req = CLIENT
        .request(method, &url)
        .header("Content-Type", "application/json")
        .header("Authorization", auth)
        .header("Cache-Control", "no-store");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req.send().await?;
    if !res.status().is_success() {
        return Err(anyhow!(
            "Request failed {} {}",
            res.status(),
            res.text().await.unwrap_or_default()
        ));
    }
    let data = res.json::<T>().await?;
    Ok(data)
}

pub async fn get_smses() -> Result<Vec<Sms>> {
    fetch_mikrotik("/rest/tool/sms/inbox", Method::GET, None).await
}

pub async fn send_sms(phone_number: &str, message: &str) -> Result<()> {
    let body = serde_json::json!({
        "phone-number": phone_number,
        "message": message,
    });
    let _: serde_json::Value =
        fetch_mikrotik("/rest/tool/sms/send", Method::POST, Some(body)).await?;
    Ok(())
}
