//! Main state service implementation

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::info;

use crate::{
    concurrency::ConcurrencyController,
    database::Database,
    handlers::StateServiceImpl,
    proto::state_service_server::StateServiceServer,
    storage::PrivateStateStorage,
};

/// State service configuration
pub struct ServiceConfig {
    pub database_url: String,
    pub s3_bucket: Option<String>,
    pub listen_addr: SocketAddr,
    pub enable_reflection: bool,
}

/// Main state service
pub struct StateServiceRunner {
    config: ServiceConfig,
    db: Arc<Database>,
    storage: Option<PrivateStateStorage>,
    concurrency: Arc<ConcurrencyController>,
}

impl StateServiceRunner {
    /// Create a new state service
    pub async fn new(config: ServiceConfig) -> Result<Self> {
        // Initialize database
        let db = Arc::new(Database::new(&config.database_url).await?);

        // Initialize storage
        let storage = if let Some(bucket) = &config.s3_bucket {
            Some(PrivateStateStorage::new(bucket.clone()).await?)
        } else {
            info!("S3 bucket not configured - blob storage will be unavailable");
            None
        };

        // Initialize concurrency controller
        let concurrency = Arc::new(ConcurrencyController::new(db.connection_arc()));

        // Start background cleanup task for locks
        concurrency.clone().start_cleanup_task();

        Ok(Self {
            config,
            db,
            storage,
            concurrency,
        })
    }

    /// Run the gRPC server
    pub async fn run(self) -> Result<()> {
        let addr = self.config.listen_addr;

        // Create service implementation
        let service = Arc::new(StateServiceImpl::new(
            self.db.clone(),
            self.storage,
            self.concurrency.clone(),
        ));

        // Start background cleanup task for completed jobs
        service.clone().start_jobs_cleanup_task();

        // Build server with conditional reflection
        let server = if self.config.enable_reflection {
            let reflection = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(crate::proto::FILE_DESCRIPTOR_SET)
                .build_v1alpha()?;
            Server::builder()
                .add_service(reflection)
                .add_service(StateServiceServer::from_arc(service.clone()))
        } else {
            Server::builder()
                .add_service(StateServiceServer::from_arc(service.clone()))
        };

        info!("Starting state service on {}", addr);

        // Run server
        server
            .serve(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

        Ok(())
    }

    /// Get a handle to the database
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Get a handle to the concurrency controller
    pub fn concurrency(&self) -> &Arc<ConcurrencyController> {
        &self.concurrency
    }
}