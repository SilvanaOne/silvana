//! HTTP client for sending health metrics

use crate::config::ExporterConfig;
use crate::metrics::HealthMetrics;
use anyhow::{Result, anyhow};
use once_cell::sync::OnceCell;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};

/// Shared HTTP client instance (reused across all requests)
static HTTP_CLIENT: OnceCell<Arc<Client>> = OnceCell::new();

/// Get or create the shared HTTP client
///
/// The client is created with the specified timeout on first call.
/// Subsequent calls reuse the same client regardless of timeout parameter.
fn get_http_client(timeout_secs: u64) -> Result<Arc<Client>> {
    HTTP_CLIENT
        .get_or_try_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))
                .map(Arc::new)
        })
        .map(|client| client.clone())
}

/// Send health metrics to the endpoint with exponential backoff retry
///
/// Sends a POST request to the specified URL with the health metrics as JSON body.
/// Uses the JWT token for authentication via Authorization Bearer header.
/// Implements exponential backoff retry on failure (up to 3 attempts).
///
/// Retries are performed for network errors and server errors (5xx).
/// Client errors (4xx) are not retried as they indicate authentication
/// or request format issues that won't be resolved by retrying.
///
/// # Arguments
/// * `url` - The endpoint URL to send metrics to
/// * `jwt_token` - The JWT token for authentication
/// * `metrics` - The health metrics to send
/// * `config` - Configuration for retry behavior and timeouts
///
/// # Returns
/// Ok(()) if the request succeeds, Err otherwise
///
/// # Example
/// ```no_run
/// use health::{send_health_metrics, collect_health_metrics, ExporterConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let metrics = collect_health_metrics();
/// let config = ExporterConfig::default();
/// send_health_metrics("https://example.com/health", "jwt-token", &metrics, &config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn send_health_metrics(
    url: &str,
    jwt_token: &str,
    metrics: &HealthMetrics,
    config: &ExporterConfig,
) -> Result<()> {
    let client = get_http_client(config.http_timeout_secs)?;
    let mut last_error = None;

    for attempt in 0..config.max_retries {
        debug!(
            "Sending health metrics to {} (attempt {}/{})",
            url,
            attempt + 1,
            config.max_retries
        );

        match send_request(&client, url, jwt_token, metrics).await {
            Ok(()) => {
                debug!("Successfully sent health metrics to {}", url);
                return Ok(());
            }
            Err(e) => {
                let error_msg = e.to_string();
                last_error = Some(e);

                // Don't retry on client errors (4xx) - check error message
                if error_msg.contains("Client error (not retrying)") {
                    warn!("Client error detected, not retrying: {}", error_msg);
                    // last_error is guaranteed to be Some here since we set it above
                    if let Some(err) = last_error.take() {
                        return Err(err);
                    }
                    // This should never happen, but handle it gracefully
                    return Err(anyhow!("Client error detected but error information lost"));
                }

                // Don't retry on the last attempt
                if attempt < config.max_retries - 1 {
                    // Calculate exponential backoff: 1s, 2s, 4s, etc., capped at MAX_BACKOFF_SECS
                    // Safe: attempt is from 0..max_retries (max 2), so 2^2 = 4 is well within u64 limits
                    let backoff_secs = (config.initial_backoff_secs * 2_u64.pow(attempt as u32))
                        .min(config.max_backoff_secs);

                    warn!(
                        "Failed to send health metrics (attempt {}/{}): {}. Retrying in {}s...",
                        attempt + 1,
                        config.max_retries,
                        error_msg,
                        backoff_secs
                    );

                    sleep(Duration::from_secs(backoff_secs)).await;
                }
            }
        }
    }

    // All retries failed
    let final_error = last_error
        .ok_or_else(|| anyhow!("Failed to send health metrics: no error information available"))?;

    Err(anyhow!(
        "Failed to send health metrics after {} attempts: {}",
        config.max_retries,
        final_error
    ))
}

/// Send a single HTTP request (internal helper)
async fn send_request(
    client: &Client,
    url: &str,
    jwt_token: &str,
    metrics: &HealthMetrics,
) -> Result<()> {
    // Validate inputs
    if jwt_token.is_empty() {
        return Err(anyhow!("JWT token is empty"));
    }
    if url.is_empty() {
        return Err(anyhow!("URL is empty"));
    }

    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", jwt_token))
        .json(metrics)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send HTTP request: {}", e))?;

    let status = response.status();

    if !status.is_success() {
        let error_text = match response.text().await {
            Ok(text) => text,
            Err(_) => "Unknown error (failed to read response body)".to_string(),
        };

        // Don't retry on client errors (4xx) - these indicate auth or request issues
        if status.is_client_error() {
            return Err(anyhow!(
                "Client error (not retrying): status {}: {}",
                status,
                error_text
            ));
        }

        // Server errors (5xx) will be retried
        return Err(anyhow!(
            "Server error (will retry): status {}: {}",
            status,
            error_text
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::metrics::*;

    #[tokio::test]
    #[ignore] // Requires actual endpoint
    async fn test_send_health_metrics() {
        let _metrics = HealthMetrics {
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            cpu: CpuMetrics {
                usage_percent: 50.0,
            },
            memory: MemoryMetrics {
                used_bytes: 1000,
                total_bytes: 2000,
                available_bytes: 1000,
                usage_percent: 50.0,
            },
            disks: vec![DiskMetrics {
                name: "sda1".to_string(),
                mount_point: "/".to_string(),
                total_bytes: 10000,
                used_bytes: 5000,
                available_bytes: 5000,
                usage_percent: 50.0,
            }],
            endpoints: Vec::new(),
        };

        // This test requires a real endpoint and JWT token
        // Uncomment and provide real values to test
        // let config = ExporterConfig::default();
        // let result = send_health_metrics(
        //     "https://example.com/health",
        //     "test-jwt-token",
        //     &metrics,
        //     &config,
        // ).await;
        // assert!(result.is_ok());
    }
}
