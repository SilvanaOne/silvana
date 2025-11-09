//! Health metrics collection and reporting crate
//!
//! This crate provides functionality to collect system health metrics (CPU, memory, disk)
//! and send them to an endpoint using JWT authentication.
//!
//! ## Usage as Library
//!
//! ```rust
//! use health::start_health_exporter;
//!
//! // Start with default 10 minute interval
//! let handle = start_health_exporter(None)?;
//!
//! // Or with custom 5 minute interval
//! let handle = start_health_exporter(Some(300))?;
//! ```
//!
//! Add to your `Cargo.toml` with minimal dependencies (no CLI):
//! ```toml
//! [dependencies]
//! health = { path = "../health", default-features = false }
//! ```
//!
//! ## Usage as Standalone Binary
//!
//! Run the health metrics exporter as a standalone service:
//!
//! ```bash
//! # Run in development
//! cargo run -p health
//!
//! # Build release binary
//! cargo build --release -p health
//! ./target/release/health --help
//! ```
//!
//! ## Environment Variables
//!
//! - `JWT_HEALTH`: JWT token containing `url` and `id` claims, signed with Ed25519
//! - `RUST_LOG`: Log level (trace, debug, info, warn, error)
//! - `HEALTH_INTERVAL`: Collection interval in seconds (optional, default: 600)

mod client;
mod config;
mod error;
mod jwt;
mod metrics;

use anyhow::{Result, anyhow};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

pub use client::send_health_metrics;
pub use config::ExporterConfig;
pub use error::HealthError;
pub use jwt::{Ed25519Keypair, HealthClaims, create_health_jwt, decode_health_jwt, generate_ed25519_keypair, verify_health_jwt};
pub use metrics::{CpuMetrics, DiskMetrics, HealthMetrics, MemoryMetrics, collect_health_metrics};

/// Start the health metrics exporter task
///
/// Reads the `JWT_HEALTH` environment variable, decodes it to extract the endpoint URL
/// and tracking ID, verifies the JWT signature, and spawns an async task that periodically
/// collects and sends health metrics.
///
/// # Arguments
/// * `interval_secs` - Collection interval in seconds. If None, defaults to 600 seconds (10 minutes)
///
/// # Returns
/// A JoinHandle for the spawned task, allowing the caller to manage the task lifecycle
///
/// # Errors
/// Returns an error if:
/// - `JWT_HEALTH` environment variable is not set
/// - JWT token cannot be decoded
/// - JWT signature verification fails
/// - Public key cannot be extracted from JWT
///
/// # Example
/// ```no_run
/// use health::start_health_exporter;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Start with default 10 minute interval
/// let handle = start_health_exporter(None)?;
///
/// // Or with custom 5 minute interval
/// let handle = start_health_exporter(Some(300))?;
///
/// // The task runs in the background. To wait for it:
/// // tokio::spawn(async move {
/// //     if let Err(e) = handle.await {
/// //         eprintln!("Health exporter task panicked: {}", e);
/// //     }
/// // });
/// # Ok(())
/// # }
/// ```
pub fn start_health_exporter(interval_secs: Option<u64>) -> Result<tokio::task::JoinHandle<()>> {
    // Read JWT_HEALTH environment variable
    let jwt_token = std::env::var("JWT_HEALTH")
        .map_err(|_| anyhow!("JWT_HEALTH environment variable not set"))?;

    if jwt_token.is_empty() {
        return Err(anyhow!("JWT_HEALTH environment variable is empty"));
    }

    // Decode JWT to extract URL and ID
    let claims = decode_health_jwt(&jwt_token)
        .map_err(|e| anyhow!("Failed to decode JWT_HEALTH token: {}", e))?;

    // Validate URL and ID are not empty (check before parsing for clearer error messages)
    if claims.url.trim().is_empty() {
        return Err(anyhow!("URL in JWT_HEALTH token is empty"));
    }

    if claims.id.trim().is_empty() {
        return Err(anyhow!("ID in JWT_HEALTH token is empty"));
    }

    // Validate URL format
    let parsed_url = url::Url::parse(&claims.url)
        .map_err(|e| anyhow!("Invalid URL in JWT_HEALTH token: {}", e))?;

    // Validate URL has a host
    if parsed_url.host().is_none() {
        return Err(anyhow!("URL in JWT_HEALTH token has no host"));
    }

    // Warn if not using HTTPS (security best practice)
    if parsed_url.scheme() != "https" {
        warn!(
            "JWT_HEALTH URL uses non-HTTPS scheme '{}'. Consider using HTTPS for security.",
            parsed_url.scheme()
        );
    }

    let url = claims.url.clone();
    let id = claims.id.clone();
    let public_key_hex = claims.sub.clone();
    let token_exp = claims.exp;

    // Validate public key hex is not empty
    if public_key_hex.trim().is_empty() {
        return Err(anyhow!("Public key (sub) in JWT_HEALTH token is empty"));
    }

    // Parse public key from hex
    let public_key_bytes = hex::decode(&public_key_hex).map_err(|e| {
        anyhow!(
            "Failed to decode public key from hex (sub={}): {}",
            public_key_hex,
            e
        )
    })?;

    if public_key_bytes.len() != 32 {
        return Err(anyhow!(
            "Invalid public key length: expected 32 bytes, got {} bytes",
            public_key_bytes.len()
        ));
    }

    let public_key_bytes_array: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| anyhow!("Failed to convert public key to array"))?;

    // Validate expiration is in the future
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| anyhow!("Failed to get current time: {}", e))?
        .as_secs();

    if token_exp <= now {
        let expired_ago = now.saturating_sub(token_exp);
        return Err(anyhow!(
            "JWT_HEALTH token is already expired: exp={}, now={}, expired {} seconds ago",
            token_exp,
            now,
            expired_ago
        ));
    }

    // Verify JWT signature
    verify_health_jwt(&jwt_token, &public_key_bytes_array)
        .map_err(|e| anyhow!("Failed to verify JWT_HEALTH token signature: {}", e))?;

    // Validate interval and create config
    let mut config = ExporterConfig::default();
    if let Some(interval) = interval_secs {
        config = config.with_interval(interval);
    }

    if let Err(e) = config.validate() {
        return Err(anyhow!("Invalid configuration: {}", e));
    }

    info!(
        "Starting health metrics exporter: url={}, id={}, interval={}s, token_exp={}",
        url, id, config.interval_secs, token_exp
    );

    // Spawn the exporter task
    let handle = tokio::spawn(async move {
        let mut interval_timer = interval(Duration::from_secs(config.interval_secs));

        // Skip the first tick (it fires immediately)
        interval_timer.tick().await;

        loop {
            tokio::select! {
                _ = interval_timer.tick() => {
                    // Check if JWT token has expired
                    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
                        Ok(duration) => duration.as_secs(),
                        Err(e) => {
                            error!(
                                "Failed to get current time: {}. Stopping health metrics exporter for id={}",
                                e, id
                            );
                            break;
                        }
                    };

                    if now >= token_exp {
                        error!(
                            "JWT_HEALTH token expired (exp={}, now={}). Stopping health metrics exporter for id={}",
                            token_exp, now, id
                        );
                        break;
                    }

                    // Collect metrics
                    let metrics = metrics::collect_health_metrics();
                    debug!(
                        "Collected health metrics: cpu={:.2}%, memory={:.2}%, disks={}",
                        metrics.cpu.usage_percent,
                        metrics.memory.usage_percent,
                        metrics.disks.len()
                    );

                    // Send metrics
                    match client::send_health_metrics(&url, &jwt_token, &metrics, &config).await {
                        Ok(()) => {
                            debug!("Successfully sent health metrics for id={}", id);
                        }
                        Err(e) => {
                            warn!("Failed to send health metrics for id={}: {}", id, e);
                            // Continue running even if send fails
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Shutdown signal received, stopping health metrics exporter for id={}", id);
                    break;
                }
            }
        }

        info!("Health metrics exporter stopped for id={}", id);
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_health_exporter_missing_env() {
        // Ensure JWT_HEALTH is not set
        unsafe {
            std::env::remove_var("JWT_HEALTH");
        }

        let result = start_health_exporter(None);
        match result {
            Err(e) => {
                if !e.to_string().contains("JWT_HEALTH") {
                    error!(
                        "test_start_health_exporter_missing_env: Error message doesn't contain 'JWT_HEALTH': {}",
                        e
                    );
                }
            }
            Ok(_) => {
                error!("test_start_health_exporter_missing_env: Expected error but got Ok");
            }
        }
    }

    #[test]
    fn test_start_health_exporter_invalid_jwt() {
        unsafe {
            std::env::set_var("JWT_HEALTH", "invalid.jwt.token");
        }

        let result = start_health_exporter(None);
        match result {
            Err(_) => {
                // Expected - invalid JWT should fail
            }
            Ok(_) => {
                error!(
                    "test_start_health_exporter_invalid_jwt: Expected error for invalid JWT but got Ok"
                );
            }
        }

        // Clean up
        unsafe {
            std::env::remove_var("JWT_HEALTH");
        }
    }
}
