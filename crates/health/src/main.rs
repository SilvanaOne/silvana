//! Standalone health metrics exporter binary
//!
//! This binary collects system health metrics (CPU, memory, disk) and periodically
//! sends them to a configured endpoint using JWT authentication.
//!
//! ## Usage
//!
//! ```bash
//! # Run with default settings (reads JWT_HEALTH from environment)
//! cargo run -p health
//!
//! # Or in release mode
//! cargo run --release -p health
//!
//! # Override collection interval (in seconds)
//! cargo run -p health -- --interval 300
//!
//! # Provide JWT token via CLI
//! cargo run -p health -- --jwt "your-jwt-token"
//!
//! # Set log level
//! RUST_LOG=debug cargo run -p health
//!
//! # Build standalone binary
//! cargo build --release -p health
//! ./target/release/health --help
//! ```
//!
//! ## Environment Variables
//!
//! - `JWT_HEALTH`: JWT token containing `url` and `id` claims (required if not using --jwt)
//! - `RUST_LOG`: Log level (trace, debug, info, warn, error) - defaults to info
//! - `HEALTH_INTERVAL`: Collection interval in seconds (optional, default: 600)

use anyhow::{Result, anyhow};
use clap::Parser;
use dotenvy::dotenv;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[clap(name = "health")]
#[clap(about = "Silvana Health Metrics Exporter - System health monitoring with JWT authentication")]
#[clap(version = env!("CARGO_PKG_VERSION"))]
struct Args {
    /// JWT token for authentication (can also be set via JWT_HEALTH env var)
    /// Format: JWT containing url, id, and public key (sub) claims
    #[clap(long, env = "JWT_HEALTH")]
    jwt: Option<String>,

    /// Collection interval in seconds (default: 600 = 10 minutes)
    /// Minimum: 60 seconds, Maximum: 86400 seconds (24 hours)
    #[clap(long, env = "HEALTH_INTERVAL")]
    interval: Option<u64>,

    /// Log level (trace, debug, info, warn, error)
    #[clap(long, default_value = "info", env = "RUST_LOG")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (ignore if not found)
    dotenv().ok();

    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing with configurable log level
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("health={},reqwest=warn", args.log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();

    // Display startup banner
    info!("╔════════════════════════════════════════════════════════════╗");
    info!("║  Silvana Health Metrics Exporter v{}                    ║", env!("CARGO_PKG_VERSION"));
    info!("╚════════════════════════════════════════════════════════════╝");

    // Validate JWT token is provided
    if args.jwt.is_none() {
        error!("JWT token not provided!");
        error!("Please set JWT_HEALTH environment variable or use --jwt flag");
        error!("Example:");
        error!("  export JWT_HEALTH=\"your-jwt-token\"");
        error!("  cargo run -p health");
        error!("Or:");
        error!("  cargo run -p health -- --jwt \"your-jwt-token\"");
        return Err(anyhow!("JWT_HEALTH not set"));
    }

    // Set JWT_HEALTH environment variable if provided via CLI
    if let Some(jwt) = &args.jwt {
        // Safe to set before spawning any threads
        unsafe {
            std::env::set_var("JWT_HEALTH", jwt);
        }
    }

    // Display configuration
    info!("Configuration:");
    info!("  Interval: {} seconds ({} minutes)",
        args.interval.unwrap_or(600),
        args.interval.unwrap_or(600) / 60
    );
    info!("  Log level: {}", args.log_level);

    // Start the health exporter
    info!("Starting health metrics exporter...");
    let handle = health::start_health_exporter(args.interval)
        .map_err(|e| {
            error!("Failed to start health exporter: {}", e);
            e
        })?;

    info!("Health metrics exporter started successfully");
    info!("Press CTRL+C to stop");

    // Wait for the task to complete or handle shutdown signal
    tokio::select! {
        result = handle => {
            match result {
                Ok(()) => {
                    info!("Health exporter task completed normally");
                }
                Err(e) => {
                    error!("Health exporter task panicked: {}", e);
                    return Err(anyhow!("Task panic: {}", e));
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received CTRL+C signal");
            info!("Shutting down gracefully...");
        }
    }

    info!("Health metrics exporter stopped");
    Ok(())
}
