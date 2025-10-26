//! Standalone state service binary for on-premises deployment

use anyhow::Result;
use clap::Parser;
use dotenvy::dotenv;
use std::net::SocketAddr;
use state::{ServiceConfig, StateServiceRunner};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[clap(name = "state-service")]
#[clap(about = "Silvana State Service - Private state management for on-premises deployment")]
struct Args {
    /// Database connection URL (can also be set via STATE_DATABASE_URL env var)
    #[clap(long, env = "STATE_DATABASE_URL")]
    database_url: String,

    /// S3 bucket for large object storage (optional, can also be set via STATE_S3_BUCKET)
    #[clap(long, env = "STATE_S3_BUCKET")]
    s3_bucket: Option<String>,

    /// Listen address for gRPC server
    #[clap(long, default_value = "0.0.0.0:50052", env = "STATE_LISTEN_ADDR")]
    listen_addr: SocketAddr,

    /// Enable gRPC reflection
    #[clap(long, env = "STATE_ENABLE_REFLECTION")]
    enable_reflection: bool,

    /// Log level (trace, debug, info, warn, error)
    #[clap(long, default_value = "info", env = "RUST_LOG")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    dotenv().ok();

    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("state={},tower_http=debug", args.log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Silvana State Service");
    tracing::info!("Database URL: {}", mask_url(&args.database_url));
    tracing::info!("Listen address: {}", args.listen_addr);
    if let Some(bucket) = &args.s3_bucket {
        tracing::info!("S3 bucket: {}", bucket);
    } else {
        tracing::warn!("S3 bucket not configured - using inline storage only");
        tracing::warn!("Large objects (>1MB) will be stored in database, which may impact performance");
    }

    // Create service configuration
    let config = ServiceConfig {
        database_url: args.database_url,
        s3_bucket: args.s3_bucket,
        listen_addr: args.listen_addr,
        enable_reflection: args.enable_reflection,
    };

    // Create and run service
    let service = StateServiceRunner::new(config).await?;

    // Handle shutdown gracefully
    let shutdown = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        tracing::info!("Received shutdown signal");
    };

    tokio::select! {
        result = service.run() => {
            if let Err(e) = result {
                tracing::error!("Service error: {}", e);
                std::process::exit(1);
            }
        }
        _ = shutdown => {
            tracing::info!("Shutting down gracefully");
        }
    }

    tracing::info!("State service stopped");
    Ok(())
}

/// Mask sensitive parts of database URL for logging
fn mask_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(password) = parsed.password() {
            url.replace(password, "****")
        } else {
            url.to_string()
        }
    } else {
        // If parsing fails, just mask everything after @
        if let Some(at_pos) = url.find('@') {
            let before_at = &url[..at_pos];
            let after_at = &url[at_pos..];

            // Find the password part (between : and @)
            if let Some(colon_pos) = before_at.rfind(':') {
                let before_colon = &before_at[..colon_pos];
                format!("{}:****{}", before_colon, after_at)
            } else {
                url.to_string()
            }
        } else {
            url.to_string()
        }
    }
}