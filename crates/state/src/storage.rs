//! S3 storage integration for large objects

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use storage::S3Client;
use tracing::{debug, error, info, warn};

/// Threshold for storing data in S3 (1MB)
const S3_STORAGE_THRESHOLD: usize = 1024 * 1024;

/// S3 storage client for state management
#[derive(Clone)]
pub struct StateStorage {
    client: Arc<S3Client>,
}

impl StateStorage {
    /// Create a new S3 storage client
    pub async fn new(bucket: String) -> Result<Self> {
        let client = S3Client::new(bucket).await?;
        info!("Connected to S3 for state storage");

        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Store data in S3 if it exceeds the threshold
    /// Returns Some(s3_key) if stored in S3, None if data is small
    pub async fn store_if_large(&self, data: &[u8], prefix: &str) -> Result<Option<String>> {
        if data.len() <= S3_STORAGE_THRESHOLD {
            return Ok(None);
        }

        let s3_key = self.store(data, prefix).await?;
        Ok(Some(s3_key))
    }

    /// Store data in S3 (always)
    pub async fn store(&self, data: &[u8], prefix: &str) -> Result<String> {
        // Generate content-addressable key using SHA256
        let hash = Sha256::digest(data);
        let hash_hex = hex::encode(hash);
        let s3_key = format!("{}/{}", prefix, hash_hex);

        debug!("Storing {} bytes to S3: {}", data.len(), s3_key);

        // Check if object already exists
        if self.client.exists(&s3_key).await.unwrap_or(false) {
            debug!("Object already exists in S3: {}", s3_key);
            return Ok(s3_key);
        }

        // Set expiration to 10 years from now (effectively permanent for state data)
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis() as u64 + (10 * 365 * 24 * 60 * 60 * 1000);

        // Convert binary data to base64 for storage
        use base64::{Engine as _, engine::general_purpose};
        let data_base64 = general_purpose::STANDARD.encode(data);

        // Store metadata about the data
        let metadata = vec![
            ("content-type".to_string(), "application/octet-stream".to_string()),
            ("content-hash".to_string(), hash_hex.clone()),
            ("content-length".to_string(), data.len().to_string()),
        ];

        // Upload to S3
        self.client
            .write(&s3_key, data_base64, Some(metadata), expires_at)
            .await
            .map_err(|e| anyhow!("Failed to store data in S3: {}", e))?;

        info!("Stored {} bytes to S3: {}", data.len(), s3_key);
        Ok(s3_key)
    }

    /// Retrieve data from S3
    pub async fn retrieve(&self, s3_key: &str) -> Result<Vec<u8>> {
        debug!("Retrieving from S3: {}", s3_key);

        let (data_base64, _metadata) = self.client
            .read(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to retrieve from S3: {}", e))?;

        // Decode from base64
        use base64::{Engine as _, engine::general_purpose};
        let data = general_purpose::STANDARD.decode(&data_base64)
            .map_err(|e| anyhow!("Failed to decode S3 data: {}", e))?;

        debug!("Retrieved {} bytes from S3", data.len());

        // Verify hash matches the key (for content-addressed storage)
        if s3_key.contains('/') {
            let parts: Vec<&str> = s3_key.split('/').collect();
            if let Some(expected_hash) = parts.last() {
                let actual_hash = hex::encode(Sha256::digest(&data));
                if actual_hash != *expected_hash {
                    error!(
                        "S3 data integrity check failed for {}: expected {}, got {}",
                        s3_key, expected_hash, actual_hash
                    );
                    return Err(anyhow!("S3 data integrity check failed"));
                }
            }
        }

        Ok(data)
    }

    /// Delete data from S3
    pub async fn delete(&self, s3_key: &str) -> Result<()> {
        debug!("Deleting from S3: {}", s3_key);

        self.client
            .delete(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to delete from S3: {}", e))?;

        info!("Deleted from S3: {}", s3_key);
        Ok(())
    }

    /// Check if an object exists in S3
    pub async fn exists(&self, s3_key: &str) -> Result<bool> {
        self.client.exists(s3_key).await
    }
}

/// Helper to store data with automatic S3 offloading
pub struct HybridStorage {
    s3: Option<StateStorage>,
}

impl HybridStorage {
    pub fn new(s3: Option<StateStorage>) -> Self {
        Self { s3 }
    }

    /// Store data, returning (inline_data, s3_key)
    pub async fn store(&self, data: &[u8], prefix: &str) -> Result<(Option<Vec<u8>>, Option<String>)> {
        if data.len() <= S3_STORAGE_THRESHOLD {
            // Store inline
            Ok((Some(data.to_vec()), None))
        } else if let Some(s3) = &self.s3 {
            // Store in S3
            let s3_key = s3.store(data, prefix).await?;
            Ok((None, Some(s3_key)))
        } else {
            // No S3 configured, store inline anyway (warning: might be large)
            warn!("Storing large data ({} bytes) inline - S3 not configured", data.len());
            Ok((Some(data.to_vec()), None))
        }
    }

    /// Retrieve data from inline or S3
    pub async fn retrieve(
        &self,
        inline_data: &Option<Vec<u8>>,
        s3_key: &Option<String>,
    ) -> Result<Vec<u8>> {
        match (inline_data, s3_key) {
            (Some(data), None) => Ok(data.clone()),
            (None, Some(key)) if self.s3.is_some() => {
                self.s3.as_ref().unwrap().retrieve(key).await
            }
            (None, Some(_)) => {
                Err(anyhow!("S3 key provided but S3 storage not configured"))
            }
            _ => {
                Err(anyhow!("No data available (neither inline nor S3)"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold() {
        let small_data = vec![0u8; 1024]; // 1KB
        let large_data = vec![0u8; 2 * 1024 * 1024]; // 2MB

        assert!(small_data.len() <= S3_STORAGE_THRESHOLD);
        assert!(large_data.len() > S3_STORAGE_THRESHOLD);
    }

    #[test]
    fn test_hash_generation() {
        let data = b"test data";
        let hash = Sha256::digest(data);
        let hash_hex = hex::encode(hash);

        // SHA256 should produce 64 character hex string
        assert_eq!(hash_hex.len(), 64);
    }
}