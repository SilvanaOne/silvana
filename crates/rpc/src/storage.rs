use anyhow::{Result, anyhow};
use storage::s3::S3Client;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// S3 storage handler for binary operations
pub struct S3Storage {
    client: Arc<RwLock<Option<S3Client>>>,
    bucket_name: String,
}

impl S3Storage {
    /// Create a new S3Storage instance
    pub fn new(bucket_name: String) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            bucket_name,
        }
    }

    /// Initialize the S3 client if not already initialized
    async fn ensure_client(&self) -> Result<()> {
        let mut client_guard = self.client.write().await;
        if client_guard.is_none() {
            debug!("Initializing S3 client for bucket: {}", self.bucket_name);
            let s3_client = S3Client::new(self.bucket_name.clone()).await?;
            *client_guard = Some(s3_client);
            info!("S3 client initialized for bucket: {}", self.bucket_name);
        }
        Ok(())
    }

    /// Write binary data to S3
    pub async fn write_binary(
        &self,
        data: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        expected_sha256: Option<String>,
    ) -> Result<(String, String)> {
        // Ensure client is initialized
        self.ensure_client().await?;
        
        let client_guard = self.client.read().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow!("S3 client not initialized"))?;

        debug!(
            "Writing binary to S3: file_name={}, size={} bytes, mime_type={}",
            file_name,
            data.len(),
            mime_type
        );

        // Call the S3 client's write_binary method
        let sha256 = client
            .write_binary(data, file_name, mime_type, None, expected_sha256)
            .await?;

        // Construct S3 URL
        let s3_url = format!("s3://{}/{}", self.bucket_name, file_name);

        info!(
            "Successfully wrote binary to S3: file_name={}, sha256={}",
            file_name, sha256
        );

        Ok((sha256, s3_url))
    }

    /// Read binary data from S3
    pub async fn read_binary(&self, file_name: &str) -> Result<(Vec<u8>, String)> {
        // Ensure client is initialized
        self.ensure_client().await?;
        
        let client_guard = self.client.read().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow!("S3 client not initialized"))?;

        debug!("Reading binary from S3: file_name={}", file_name);

        // Call the S3 client's read_binary method
        let result = client.read_binary(file_name).await?;

        info!(
            "Successfully read binary from S3: file_name={}, size={} bytes, sha256={}",
            file_name,
            result.data.len(),
            result.sha256
        );

        Ok((result.data, result.sha256))
    }

    /// Delete a binary file from S3
    pub async fn delete_binary(&self, file_name: &str) -> Result<()> {
        // Ensure client is initialized
        self.ensure_client().await?;
        
        let client_guard = self.client.read().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow!("S3 client not initialized"))?;

        debug!("Deleting binary from S3: file_name={}", file_name);

        client.delete(file_name).await?;

        info!("Successfully deleted binary from S3: file_name={}", file_name);

        Ok(())
    }

    /// Check if a binary file exists in S3
    pub async fn exists(&self, file_name: &str) -> Result<bool> {
        // Ensure client is initialized
        self.ensure_client().await?;
        
        let client_guard = self.client.read().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow!("S3 client not initialized"))?;

        debug!("Checking if binary exists in S3: file_name={}", file_name);

        let exists = client.exists(file_name).await?;

        debug!(
            "Binary {} in S3: file_name={}",
            if exists { "exists" } else { "does not exist" },
            file_name
        );

        Ok(exists)
    }
}