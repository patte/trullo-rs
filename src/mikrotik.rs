#![cfg(feature = "server")]
use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::env;

static CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("client")
});

fn base_url() -> Result<String> {
    env::var("MIKROTIK_URL").map_err(|_| anyhow!("MIKROTIK_URL not set"))
}

fn auth_header() -> Result<String> {
    if let Ok(b64) = env::var("MIKROTIK_AUTH_BASE64") {
        Ok(format!("Basic {}", b64))
    } else if let (Ok(user), Ok(pass)) = (
        env::var("MIKROTIK_USER"),
        env::var("MIKROTIK_PASS").or_else(|_| env::var("MIKROTIK_PASSWORD")),
    ) {
        Ok(format!(
            "Basic {}",
            base64::encode(format!("{}:{}", user, pass))
        ))
    } else {
        Err(anyhow!(
            "Set MIKROTIK_AUTH_BASE64 or MIKROTIK_USER and MIKROTIK_PASSWORD (or MIKROTIK_PASS)"
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sms {
    #[serde(rename = ".id")]
    pub id: String,
    pub message: String,
    #[serde(default)]
    pub timestamp: Option<String>, // Some firmware exposes RFC3339, but often it's missing
    #[serde(rename = "time")]
    pub time: Option<String>, // RouterOS time string e.g. "aug/17/2024 15:27:02"
    #[serde(rename = "received")]
    pub received: Option<String>, // Sometimes present
    #[serde(rename = "from")]
    pub from: Option<String>,
}

pub async fn fetch_mikrotik<T: for<'de> Deserialize<'de> + Send + 'static>(
    path: &str,
    method: Method,
    body: Option<serde_json::Value>,
) -> Result<T> {
    let url = format!("{}{}", base_url()?, path);
    eprintln!("[mikrotik] {} {}", method.as_str(), url);
    let method_s = method.as_str().to_string();
    let auth = auth_header()?;
    let mut req = CLIENT
        .request(method, &url)
        .header("Content-Type", "application/json")
        .header("Authorization", auth)
        .header("Cache-Control", "no-store");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let res = req
        .send()
        .await
        .with_context(|| format!("sending {} {}", method_s, url))?;
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        eprintln!(
            "[mikrotik] request failed: status={} body=\n{}",
            status, text
        );
        return Err(anyhow!("Request failed {} {}", status, text));
    }
    // Read body once to allow better error messages when JSON decoding fails
    let bytes = res
        .bytes()
        .await
        .with_context(|| format!("reading body from {} {}", method_s, url))?;
    let data: T = serde_json::from_slice(&bytes).map_err(|e| {
        let snip = String::from_utf8_lossy(&bytes);
        let snip = snip.chars().take(300).collect::<String>();
        anyhow!(
            "decoding JSON from {} {} failed: {}\nBody snippet: {}",
            method_s,
            url,
            e,
            snip
        )
    })?;
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
    eprintln!("[mikrotik] sending SMS to {}", phone_number);
    let _: serde_json::Value =
        fetch_mikrotik("/rest/tool/sms/send", Method::POST, Some(body)).await?;
    Ok(())
}
