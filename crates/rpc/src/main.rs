use std::env;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

use proto::events::silvana_events_service_server::SilvanaEventsServiceServer;
use rpc::database::EventDatabase;
use rpc::log;
use rpc::monitoring::{init_monitoring, spawn_monitoring_tasks, start_metrics_server};
use rpc::SilvanaEventsServiceImpl;

// Import buffer directly
use buffer::EventBuffer;

// External import of EventWrapper and other adapters
use ::rpc::adapters::create_tidb_backend;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    println!("🚀 Starting Silvana RPC server");
    // Load environment variables from .env file

    // Initialize logging
    log::init_logging().await?;
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

    let database = Arc::new(
        EventDatabase::new(&database_url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?,
    );
    info!("✅ Connected to TiDB successfully");

    // Configuration parameters
    let batch_size = env::var("BATCH_SIZE")
        .unwrap_or_else(|_| "100".to_string())
        .parse::<usize>()
        .unwrap_or(100);

    let flush_interval_ms = env::var("FLUSH_INTERVAL_MS")
        .unwrap_or_else(|_| "1000".to_string())
        .parse::<u64>()
        .unwrap_or(1000);

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

    // Create NATS publisher if NATS_URL is configured
    let nats_publisher = match env::var("NATS_URL") {
        Ok(nats_url) => {
            info!("🔄 Attempting to connect to NATS server at: {}", nats_url);
            match nats::EventNatsPublisher::new().await {
                Ok(publisher) => Some(Arc::new(publisher)
                    as Arc<dyn buffer::EventPublisher<::rpc::adapters::EventWrapper>>),
                Err(e) => {
                    warn!(
                        "⚠️  Failed to connect to NATS server at {}: {}. Continuing without NATS.",
                        nats_url, e
                    );
                    None
                }
            }
        }
        Err(_) => {
            info!("ℹ️  NATS_URL not set. Running without NATS publishing.");
            None
        }
    };

    // Create event buffer with TiDB backend and optional NATS publisher
    let backend = Arc::new(create_tidb_backend(Arc::clone(&database)));
    let event_buffer = EventBuffer::with_config(
        backend,
        nats_publisher,
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
                    warn!("   Falling back to unencrypted gRPC");
                }
            }
        }
    }

    // Start both servers concurrently
    let grpc_server = server_builder
        .add_service(grpc_service)
        .serve(server_address);

    let metrics_server = start_metrics_server(metrics_addr);

    info!("🚀 Silvana gRPC server listening on {}", server_address);

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
