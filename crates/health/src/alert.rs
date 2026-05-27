//! Out-of-band alert delivery for prolonged worker failures.
//!
//! Pairs with `ErrorDurationTracker`: when the tracker returns an
//! `AlertKind`, the caller invokes `alert(...)` to forward the message to
//! the monitoring lambda, which fans it out to Telegram with server-side
//! dedup.
//!
//! Auth reuses `JWT_HEALTH` byte-identically. The alert POST lands on the
//! same endpoint as periodic metrics (the JWT's `url` claim) and is
//! distinguished by a `"type": "alert"` discriminator in the body — the
//! lambda routes on that field. Existing operator tokens stay valid; no
//! rotation needed.
//!
//! When `JWT_HEALTH` is unset, this is a no-op + `warn!` so the host
//! service stays usable in dev.

use crate::config::ExporterConfig;
use crate::jwt::decode_health_jwt;
use crate::tracker::AlertKind;
use anyhow::{Result, anyhow};
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{debug, warn};

/// Wire format posted to the monitoring lambda for alerts.
///
/// `type` is the only field that distinguishes this from a `HealthMetrics`
/// payload — `HealthMetrics` never sets it, so the lambda peeks at this
/// field to dispatch.
#[derive(Debug, Clone, Serialize)]
pub struct AlertPayload<'a> {
    /// Fixed `"alert"` discriminator. Lambda routes on this.
    #[serde(rename = "type")]
    pub kind_tag: &'static str,
    /// Tracking id pulled from the JWT `id` claim. The receiver uses this
    /// as the node identifier when looking up the chat to message.
    pub node_id: &'a str,
    /// Logical bucket — e.g. "updates_worker".
    pub category: &'a str,
    /// Dedup scope within `category` — e.g. "updates_worker:<party_id>".
    pub dedup_key: &'a str,
    /// One of "first" | "reminder" | "recovered".
    pub kind: &'static str,
    /// Pre-formatted single-line summary, HTML-safe.
    pub message: &'a str,
    /// Unix-seconds at the moment of send.
    pub ts: u64,
}

impl AlertKind {
    /// Wire-format string used in `AlertPayload.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            AlertKind::First => "first",
            AlertKind::Reminder => "reminder",
            AlertKind::Recovered => "recovered",
        }
    }
}

static HTTP_CLIENT: OnceCell<Arc<Client>> = OnceCell::new();

fn get_client(timeout_secs: u64) -> Result<Arc<Client>> {
    HTTP_CLIENT
        .get_or_try_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))
                .map(Arc::new)
        })
        .map(|c| c.clone())
}

/// Send a single alert via the monitoring lambda.
///
/// Returns `Ok(())` even when `JWT_HEALTH` is unset or `alert_url` is
/// missing — alerts are best-effort and should never tear down the caller's
/// reconnect loop. Errors only surface for genuine HTTP / serialization
/// failures, and callers in this codebase ignore them by convention.
pub async fn alert(
    category: &str,
    dedup_key: &str,
    kind: AlertKind,
    message: &str,
) -> Result<()> {
    let jwt_token = match std::env::var("JWT_HEALTH") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            warn!(
                category,
                dedup_key,
                "health::alert called but JWT_HEALTH is not set — alert dropped"
            );
            return Ok(());
        }
    };

    let claims = decode_health_jwt(&jwt_token)
        .map_err(|e| anyhow!("Failed to decode JWT_HEALTH for alert: {}", e))?;

    // Alerts land on the same endpoint as periodic metrics (`url` claim).
    // The lambda forks on `type: "alert"` in the body — no path tricks, no
    // new JWT claim, no rotation. Empty/missing url means the JWT is
    // unusable; bail loudly.
    let alert_url = if claims.url.trim().is_empty() {
        warn!(
            id = %claims.id,
            category,
            dedup_key,
            "JWT_HEALTH has empty url claim — alert dropped"
        );
        return Ok(());
    } else {
        claims.url.clone()
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let payload = AlertPayload {
        kind_tag: "alert",
        node_id: &claims.id,
        category,
        dedup_key,
        kind: kind.as_str(),
        message,
        ts,
    };

    let config = ExporterConfig::default();
    let client = get_client(config.http_timeout_secs)?;
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..config.max_retries {
        match post_alert(&client, &alert_url, &jwt_token, &payload).await {
            Ok(()) => {
                debug!(
                    category,
                    dedup_key,
                    kind = kind.as_str(),
                    "alert sent to monitoring"
                );
                return Ok(());
            }
            Err(e) => {
                let msg = e.to_string();
                last_err = Some(e);
                if msg.contains("Client error (not retrying)") {
                    warn!(category, dedup_key, "alert client error: {}", msg);
                    return Err(last_err.take().unwrap_or_else(|| anyhow!("client error")));
                }
                if attempt + 1 < config.max_retries {
                    let backoff = (config.initial_backoff_secs * 2_u64.pow(attempt as u32))
                        .min(config.max_backoff_secs);
                    warn!(
                        category,
                        dedup_key,
                        attempt = attempt + 1,
                        "alert send failed, retrying in {}s: {}",
                        backoff,
                        msg
                    );
                    sleep(Duration::from_secs(backoff)).await;
                }
            }
        }
    }

    Err(anyhow!(
        "Failed to send alert after {} attempts: {}",
        config.max_retries,
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ))
}

async fn post_alert(
    client: &Client,
    url: &str,
    jwt_token: &str,
    payload: &AlertPayload<'_>,
) -> Result<()> {
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", jwt_token))
        .json(payload)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send alert HTTP request: {}", e))?;

    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<no body>".to_string());

    if status.is_client_error() {
        Err(anyhow!(
            "Client error (not retrying): status {}: {}",
            status,
            body
        ))
    } else {
        Err(anyhow!("Server error: status {}: {}", status, body))
    }
}
