use anyhow::Result;
use dotenvy;
use std::env;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

// Include the generated protobuf code
pub mod events {
    tonic::include_proto!("silvana.events");
}

// Application modules
mod buffer;
mod database;
#[path = "entity/mod.rs"]
mod entities;
mod log;
mod monitoring;
mod rpc;

use buffer::EventBuffer;
use database::EventDatabase;
use events::silvana_events_service_server::SilvanaEventsServiceServer;
use monitoring::{init_monitoring, spawn_monitoring_tasks, start_metrics_server};
use rpc::SilvanaEventsServiceImpl;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting Silvana RPC server");
    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize logging
    log::init_logging().await?;
    //println!("✅ Logging initialized");
    info!("✅ Logging initialized");

    // Initialize monitoring system
    init_monitoring()?;

    let database_url = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL environment variable must be set"))?;

    let server_address = env::var("SERVER_ADDRESS")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid SERVER_ADDRESS format: {}", e))?;

    let metrics_addr = env::var("METRICS_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9090".to_string())
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid METRICS_ADDR format: {}", e))?;

    // TLS configuration
    let tls_cert_path = env::var("TLS_CERT_PATH");
    let tls_key_path = env::var("TLS_KEY_PATH");
    let enable_tls = tls_cert_path.is_ok() && tls_key_path.is_ok();

    // Parse buffer configuration with memory-safe defaults
    let batch_size = env::var("BATCH_SIZE")
        .unwrap_or_else(|_| "100".to_string())
        .parse::<usize>()
        .unwrap_or(100);

    let flush_interval_ms = env::var("FLUSH_INTERVAL_MS")
        .unwrap_or_else(|_| "500".to_string())
        .parse::<u64>()
        .unwrap_or(500);

    let channel_capacity = env::var("CHANNEL_CAPACITY")
        .unwrap_or_else(|_| "500000".to_string())
        .parse::<usize>()
        .unwrap_or(500000);

    info!("📡 Server address: {} (gRPC + gRPC-Web)", server_address);
    info!("📊 Metrics address: {}", metrics_addr);
    if enable_tls {
        info!("🔒 TLS: Enabled (direct gRPC over HTTPS)");
    } else {
        info!("🔓 TLS: Disabled (plain gRPC over HTTP)");
    }
    info!("🗄️  Database: TiDB Serverless");
    info!("🌐 Protocols: gRPC (HTTP/2) and gRPC-Web (HTTP/1.1)");
    info!(
        "⚙️  Batch size: {} events (minimum trigger - actual batches may be larger)",
        batch_size
    );
    info!("⏱️  Flush interval: {}ms", flush_interval_ms);
    info!("📦 Channel capacity: {} events", channel_capacity);
    info!("🧠 Memory limit: {}MB", 100);

    // Initialize database connection
    let database = Arc::new(
        EventDatabase::new(&database_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?,
    );

    info!("✅ Connected to TiDB successfully");

    // Initialize event buffer with memory-safe configuration
    let event_buffer = EventBuffer::with_config(
        Arc::clone(&database),
        batch_size,
        Duration::from_millis(flush_interval_ms),
        channel_capacity,
    );

    info!("✅ Event buffer initialized with memory safety features");

    // Start monitoring tasks
    spawn_monitoring_tasks(event_buffer.clone());

    // Create gRPC service with Prometheus metrics layer
    let events_service = SilvanaEventsServiceImpl::new(event_buffer, Arc::clone(&database));
    let grpc_service = SilvanaEventsServiceServer::new(events_service);

    info!("🎯 Starting gRPC and gRPC-Web server on {}", server_address);

    // Configure CORS for gRPC-Web
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers(Any);

    // Build server with optional TLS
    let mut server_builder = Server::builder()
        .accept_http1(true) // Enable HTTP/1.1 for gRPC-Web
        .layer(cors)
        .layer(GrpcWebLayer::new());

    // Configure TLS if certificates are available
    if enable_tls {
        if let (Ok(cert_path), Ok(key_path)) = (&tls_cert_path, &tls_key_path) {
            match load_tls_config(cert_path, key_path).await {
                Ok(tls_config) => {
                    info!("✅ TLS certificates loaded successfully");
                    server_builder = server_builder.tls_config(tls_config)?;
                }
                Err(e) => {
                    warn!("⚠️  Failed to load TLS certificates: {}", e);
                    warn!(" Falling back to unencrypted gRPC");
                }
            }
        }
    }

    // Start both servers concurrently
    let grpc_server = server_builder
        .add_service(grpc_service)
        .serve(server_address);

    let metrics_server = start_metrics_server(metrics_addr);

    // Run both servers concurrently
    tokio::select! {
        result = grpc_server => {
            if let Err(e) = result {
                error!("gRPC server failed: {}", e);
                error!("Error details: {:?}", e);
                error!("Error source: {:?}", e.source());
            }
        }
        result = metrics_server => {
            if let Err(e) = result {
                error!("Metrics server failed: {}", e);
            }
        }
    }

    Ok(())
}

async fn load_tls_config(cert_path: &str, key_path: &str) -> Result<ServerTlsConfig> {
    use tokio::fs;

    let cert = fs::read(cert_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read certificate file {}: {}", cert_path, e))?;

    let key = fs::read(key_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read private key file {}: {}", key_path, e))?;

    let identity = Identity::from_pem(&cert, &key);

    Ok(ServerTlsConfig::new().identity(identity))
}
