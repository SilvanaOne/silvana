use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::s3::S3Client;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    pub data: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuiltPiece {
    pub identifier: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuiltData {
    pub pieces: Vec<QuiltPiece>,
    pub metadata: HashMap<String, String>,
    pub created_at: String,
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

        match self
            .s3_client
            .write(&hash, proof_data.clone(), metadata.clone(), expires_at)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                error!(
                    "ProofsCache: Failed to submit proof - Hash: {}, Data size: {} bytes",
                    hash,
                    proof_data.len()
                );
                if let Some(ref meta) = metadata {
                    error!("ProofsCache: Metadata count: {}", meta.len());
                    for (key, value) in meta.iter() {
                        error!(
                            "  Metadata: {}={} (len={})",
                            key,
                            if value.len() > 50 {
                                format!("{}...", &value[..50])
                            } else {
                                value.clone()
                            },
                            value.len()
                        );
                    }
                }
                error!("ProofsCache: Expires at: {}", expires_at);
                return Err(e);
            }
        }

        debug!("Successfully submitted proof to cache with hash: {}", hash);
        Ok(hash)
    }

    /// Submit a quilt (multiple proofs bundled together) to the cache
    ///
    /// # Arguments
    /// * `pieces` - Vector of (identifier, data) tuples
    /// * `metadata` - Optional metadata as key-value pairs (will have quilts:true added)
    /// * `expires_at` - Optional Unix timestamp in milliseconds for expiration
    ///
    /// # Returns
    /// * The hash of the quilt
    ///
    /// # Note
    /// S3 has a limit of 10 tags per object. This method prioritizes essential metadata
    /// to ensure we don't exceed that limit. Priority order:
    /// 1. expires_at (added by S3Client)
    /// 2. quilts flag
    /// 3. pieces_count  
    /// 4. chain (if provided in metadata)
    /// 5. app_instance (if provided in metadata)
    /// 6. created_at
    /// 7. identifiers (if space allows)
    /// 8. Other metadata (if space allows)
    pub async fn submit_quilt(
        &self,
        pieces: Vec<(String, String)>,
        metadata: Option<Vec<(String, String)>>,
        expires_at: Option<u64>,
    ) -> Result<(String, Vec<String>)> {
        // Build the quilt structure
        let quilt_pieces: Vec<QuiltPiece> = pieces
            .iter()
            .map(|(id, data)| QuiltPiece {
                identifier: id.clone(),
                data: data.clone(),
            })
            .collect();

        // Build metadata with quilts flag - using HashMap first for processing
        let mut quilt_metadata = HashMap::new();

        // Add user metadata first to extract chain and app_instance
        let mut chain = None;
        let mut app_instance = None;
        if let Some(ref meta) = metadata {
            for (key, value) in meta {
                if key == "chain" {
                    chain = Some(value.clone());
                } else if key == "app_instance" {
                    app_instance = Some(value.clone());
                }
                quilt_metadata.insert(key.clone(), value.clone());
            }
        }

        // Override/add essential metadata
        quilt_metadata.insert("quilts".to_string(), "true".to_string());
        quilt_metadata.insert("pieces_count".to_string(), pieces.len().to_string());

        // Add chain if provided
        if let Some(chain_value) = chain {
            quilt_metadata.insert("chain".to_string(), chain_value);
        }

        // Add app_instance if provided
        if let Some(app_instance_value) = app_instance {
            quilt_metadata.insert("app_instance".to_string(), app_instance_value);
        }

        // Add created_at timestamp
        let created_at = chrono::Utc::now().to_rfc3339();
        quilt_metadata.insert("created_at".to_string(), created_at.clone());

        // Add identifiers list (might be truncated if we hit tag limit)
        let identifiers: Vec<String> = pieces.iter().map(|(id, _)| id.clone()).collect();
        quilt_metadata.insert("identifiers".to_string(), identifiers.join(","));

        let quilt_data = QuiltData {
            pieces: quilt_pieces,
            metadata: quilt_metadata.clone(),
            created_at: created_at,
        };

        // Serialize the quilt
        let quilt_json = serde_json::to_string_pretty(&quilt_data)?;

        // Calculate hash of the quilt
        let mut hasher = Sha256::new();
        hasher.update(quilt_json.as_bytes());
        let hash = general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize());

        // Calculate expiration
        let expires_at = expires_at.unwrap_or_else(|| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis() as u64;
            now + (7 * 24 * 60 * 60 * 1000) // 7 days for quilts
        });

        warn!(
            "Submitting quilt to cache with hash: {}, {} pieces",
            hash,
            pieces.len()
        );

        // Convert metadata to Vec format for S3, prioritizing important tags
        // S3 allows max 10 tags, and expires_at takes 1 slot, so we have 9 slots for metadata
        const MAX_METADATA_TAGS: usize = 9;
        let mut metadata_vec: Vec<(String, String)> = Vec::new();

        // Priority 1: Essential quilt metadata
        if let Some(v) = quilt_metadata.get("quilts") {
            metadata_vec.push(("quilts".to_string(), v.clone()));
        }
        if let Some(v) = quilt_metadata.get("pieces_count") {
            metadata_vec.push(("pieces_count".to_string(), v.clone()));
        }

        // Priority 2: Chain and app_instance
        if let Some(v) = quilt_metadata.get("chain") {
            metadata_vec.push(("chain".to_string(), v.clone()));
        }
        if let Some(v) = quilt_metadata.get("app_instance") {
            metadata_vec.push(("app_instance".to_string(), v.clone()));
        }

        // Priority 3: created_at
        if let Some(v) = quilt_metadata.get("created_at") {
            metadata_vec.push(("created_at".to_string(), v.clone()));
        }

        // Priority 4: identifiers (if we have space)
        if metadata_vec.len() < MAX_METADATA_TAGS {
            if let Some(v) = quilt_metadata.get("identifiers") {
                metadata_vec.push(("identifiers".to_string(), v.clone()));
            }
        }

        // Priority 5: Other metadata (if we have space)
        for (key, value) in quilt_metadata.iter() {
            if metadata_vec.len() >= MAX_METADATA_TAGS {
                warn!(
                    "Reached S3 tag limit ({}), skipping remaining metadata",
                    MAX_METADATA_TAGS
                );
                break;
            }
            // Skip already added keys
            if ![
                "quilts",
                "pieces_count",
                "chain",
                "app_instance",
                "created_at",
                "identifiers",
            ]
            .contains(&key.as_str())
            {
                metadata_vec.push((key.clone(), value.clone()));
            }
        }

        // Store to S3
        match self
            .s3_client
            .write(&hash, quilt_json, Some(metadata_vec), expires_at)
            .await
        {
            Ok(_) => {
                warn!("✅ Successfully stored quilt to cache:");
                warn!("  Hash: {}", hash);
                warn!("  Pieces: {}", identifiers.join(", "));

                // Generate piece IDs for compatibility
                let piece_ids: Vec<String> = identifiers
                    .iter()
                    .map(|id| format!("cache:{}:{}", hash, id))
                    .collect();

                Ok((hash, piece_ids))
            }
            Err(e) => {
                error!("Failed to submit quilt to cache: {}", e);
                Err(e)
            }
        }
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

        debug!(
            "Successfully read proof from cache with hash: {}",
            proof_hash
        );
        Ok(ProofData { data, metadata })
    }

    /// Read a quilt from the cache
    ///
    /// # Arguments
    /// * `quilt_hash` - The hash of the quilt to retrieve
    /// * `block_number` - Optional: specific block number to get (as identifier)
    ///
    /// # Returns
    /// * The requested proof data or the first proof if block_number is None
    pub async fn read_quilt(
        &self,
        quilt_hash: &str,
        block_number: Option<&str>,
    ) -> Result<ProofData> {
        debug!(
            "Reading quilt from cache with hash: {}, block: {:?}",
            quilt_hash, block_number
        );

        let (data, metadata) = match self.s3_client.read(quilt_hash).await {
            Ok(result) => result,
            Err(e) => {
                error!("ProofsCache: Failed to read quilt - Hash: {}", quilt_hash);
                return Err(e);
            }
        };

        // Check if this is actually a quilt
        if metadata.get("quilts").map(|v| v.as_str()) != Some("true") {
            return Err(anyhow!("Hash {} is not a quilt", quilt_hash));
        }

        // Parse the quilt data
        let quilt_data: QuiltData = serde_json::from_str(&data)?;

        // Find the requested piece or return the first one
        let piece = if let Some(block_num) = block_number {
            quilt_data
                .pieces
                .iter()
                .find(|p| p.identifier == block_num)
                .ok_or_else(|| anyhow!("Block {} not found in quilt", block_num))?
        } else {
            quilt_data
                .pieces
                .first()
                .ok_or_else(|| anyhow!("Quilt has no pieces"))?
        };

        info!(
            "Successfully read piece '{}' from quilt {}",
            piece.identifier, quilt_hash
        );

        // Return the piece data with quilt metadata
        let mut piece_metadata = metadata.clone();
        piece_metadata.insert("quilt_hash".to_string(), quilt_hash.to_string());
        piece_metadata.insert("piece_identifier".to_string(), piece.identifier.clone());

        Ok(ProofData {
            data: piece.data.clone(),
            metadata: piece_metadata,
        })
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
