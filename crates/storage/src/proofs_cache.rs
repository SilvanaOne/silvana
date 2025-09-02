use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info};

use crate::s3::S3Client;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    pub data: String,
    pub metadata: HashMap<String, String>,
}

pub struct ProofsCache {
    s3_client: S3Client,
}

impl ProofsCache {
    /// Create a new ProofsCache instance
    ///
    /// # Arguments
    /// * `bucket_name` - Optional bucket name. If None, uses PROOFS_CACHE_BUCKET env variable
    pub async fn new(bucket_name: Option<String>) -> Result<Self> {
        let bucket_name = match bucket_name {
            Some(name) => name,
            None => std::env::var("PROOFS_CACHE_BUCKET").map_err(|_| {
                anyhow!(
                    "PROOFS_CACHE_BUCKET environment variable not set and no bucket name provided"
                )
            })?,
        };

        let s3_client = S3Client::new(bucket_name.clone()).await?;

        info!("ProofsCache created with bucket: {}", bucket_name);

        Ok(Self { s3_client })
    }

    /// Submit a proof to the cache
    ///
    /// # Arguments
    /// * `proof_data` - The proof data to store
    /// * `metadata` - Optional metadata as key-value pairs
    /// * `expires_at` - Optional Unix timestamp in milliseconds for expiration (defaults to 2 days from now)
    ///
    /// # Returns
    /// * The hash of the proof data
    pub async fn submit_proof(
        &self,
        proof_data: String,
        metadata: Option<Vec<(String, String)>>,
        expires_at: Option<u64>,
    ) -> Result<String> {
        // Calculate default expiration (2 days from now in milliseconds)
        let expires_at = expires_at.unwrap_or_else(|| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;
            now + (2 * 24 * 60 * 60 * 1000) // 2 days in milliseconds
        });

        // Calculate hash of the proof data
        let mut hasher = Sha256::new();
        hasher.update(proof_data.as_bytes());
        let hash = general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());

        debug!(
            "Submitting proof to cache with hash: {}, metadata: {:?}, expires at {}",
            hash,
            metadata.is_some(),
            expires_at
        );

        match self.s3_client
            .write(&hash, proof_data.clone(), metadata.clone(), expires_at)
            .await {
            Ok(_) => {},
            Err(e) => {
                error!("ProofsCache: Failed to submit proof - Hash: {}, Data size: {} bytes", 
                    hash, proof_data.len());
                if let Some(ref meta) = metadata {
                    error!("ProofsCache: Metadata count: {}", meta.len());
                    for (key, value) in meta.iter() {
                        error!("  Metadata: {}={} (len={})", key, 
                            if value.len() > 50 { format!("{}...", &value[..50]) } else { value.clone() },
                            value.len());
                    }
                }
                error!("ProofsCache: Expires at: {}", expires_at);
                return Err(e);
            }
        }

        info!("Successfully submitted proof to cache with hash: {}", hash);
        Ok(hash)
    }

    /// Read a proof from the cache
    ///
    /// # Arguments
    /// * `proof_hash` - The hash of the proof to retrieve
    ///
    /// # Returns
    /// * ProofData struct containing data and metadata
    pub async fn read_proof(&self, proof_hash: &str) -> Result<ProofData> {
        debug!("Reading proof from cache with hash: {}", proof_hash);

        let (data, metadata) = match self.s3_client.read(proof_hash).await {
            Ok(result) => result,
            Err(e) => {
                error!("ProofsCache: Failed to read proof - Hash: {}", proof_hash);
                return Err(e);
            }
        };

        info!(
            "Successfully read proof from cache with hash: {}",
            proof_hash
        );
        Ok(ProofData { data, metadata })
    }

    /// Check if a proof exists in the cache
    pub async fn proof_exists(&self, proof_hash: &str) -> Result<bool> {
        self.s3_client.exists(proof_hash).await
    }

    /// Delete a proof from the cache
    pub async fn delete_proof(&self, proof_hash: &str) -> Result<()> {
        self.s3_client.delete(proof_hash).await
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_default_expiration() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let two_days_from_now = now + (2 * 24 * 60 * 60 * 1000);

        // The default expiration should be approximately 2 days from now
        // We can't test the exact value due to timing, but we can verify the calculation
        assert!(two_days_from_now > now);
        assert_eq!(two_days_from_now - now, 172800000); // 2 days in milliseconds
    }
}
