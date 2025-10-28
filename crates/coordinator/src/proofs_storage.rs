use anyhow::{Result, anyhow};
use async_trait::async_trait;
use sha2::Digest;
use std::collections::HashMap;
use tracing::{error, info, warn};

// Re-export storage modules
use storage::s3::S3Client;
use storage::walrus::{
    GetWalrusUrlParams, QuiltPatch, ReadFromWalrusParams, SaveToWalrusParams, WalrusClient,
    WalrusData,
};

// Import proto types for gRPC communication
use proto::{GetProofRequest, SubmitProofRequest};
use tonic::Request;

/// Proof descriptor format: chain:network:hash
/// Examples:
/// - walrus:testnet:s04UEUvCT-CTJVKsPHDWWMWcagr2mzDJHlnFm-dHOeo
/// - ipfs:public:fhkvrefvejhfvjrvfekkkrfvefv
/// - s3:private:vhrevkevekkvvbhreb
/// - cache:public:hjfvhjvjhdv
#[derive(Debug, Clone)]
pub struct ProofDescriptor {
    pub chain: String,
    pub network: String,
    pub hash: String,
}

impl ProofDescriptor {
    /// Parse a descriptor string into components
    pub fn parse(descriptor: &str) -> Result<Self> {
        let parts: Vec<&str> = descriptor.split(':').collect();
        if parts.len() != 3 {
            return Err(anyhow!(
                "Invalid descriptor format. Expected chain:network:hash, got: {}",
                descriptor
            ));
        }

        Ok(ProofDescriptor {
            chain: parts[0].to_string(),
            network: parts[1].to_string(),
            hash: parts[2].to_string(),
        })
    }

    /// Convert back to string format
    pub fn to_string(&self) -> String {
        format!("{}:{}:{}", self.chain, self.network, self.hash)
    }
}

/// Metadata for proof storage
#[derive(Debug, Clone)]
pub struct ProofMetadata {
    pub data: HashMap<String, String>,
    pub expires_at: Option<u64>,
}

impl Default for ProofMetadata {
    fn default() -> Self {
        ProofMetadata {
            data: HashMap::new(),
            expires_at: None,
        }
    }
}

/// Result for quilt storage operations
#[derive(Debug, Clone)]
pub struct QuiltStorageResult {
    pub descriptor: ProofDescriptor,
    pub blob_id: String,
    pub tx_digest: Option<String>,
    pub quilt_patches: Vec<QuiltPatch>,
}

/// Trait for proof storage backends
#[async_trait]
pub trait ProofStorageBackend: Send + Sync {
    /// Store a proof and return its descriptor
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor>;

    /// Store multiple proofs as a quilt
    async fn store_quilt(
        &self,
        quilt_data: Vec<(String, String)>, // (identifier, data)
        metadata: Option<ProofMetadata>,
    ) -> Result<QuiltStorageResult>;

    /// Retrieve a proof by its descriptor
    async fn get_proof(&self, descriptor: &ProofDescriptor) -> Result<(String, ProofMetadata)>;

    /// Check if a proof exists
    #[allow(dead_code)]
    async fn proof_exists(&self, descriptor: &ProofDescriptor) -> Result<bool>;
}

/// Walrus storage backend implementation
pub struct WalrusStorage {
    client: WalrusClient,
    network: String,
}

impl WalrusStorage {
    pub fn new(network: String) -> Self {
        WalrusStorage {
            client: WalrusClient::new(),
            network,
        }
    }
}

#[async_trait]
impl ProofStorageBackend for WalrusStorage {
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor> {
        info!("Storing proof to Walrus ({})", self.network);

        // Store metadata as part of the proof data if provided
        let data_to_store = if let Some(ref meta) = metadata {
            let wrapper = serde_json::json!({
                "proof": proof_data,
                "metadata": meta.data,
                "expires_at": meta.expires_at,
            });
            wrapper.to_string()
        } else {
            proof_data.to_string()
        };

        let params = SaveToWalrusParams {
            data: WalrusData::Blob(data_to_store),
            address: None,
            num_epochs: Some(2), // TODO: calculate using expiires_at and epoch duration
        };

        match self.client.save_to_walrus(params).await {
            Ok(Some(result)) => {
                info!("Successfully stored proof to Walrus: {}", result.blob_id);
                if let Some(tx) = &result.tx_digest {
                    info!("Walrus transaction digest: {}", tx);
                }
                Ok(ProofDescriptor {
                    chain: "walrus".to_string(),
                    network: self.network.clone(),
                    hash: result.blob_id,
                })
            }
            Ok(None) => Err(anyhow!("Walrus returned no result")),
            Err(e) => Err(anyhow!("Failed to store proof to Walrus: {}", e)),
        }
    }

    async fn get_proof(&self, descriptor: &ProofDescriptor) -> Result<(String, ProofMetadata)> {
        if descriptor.chain != "walrus" {
            return Err(anyhow!(
                "Invalid chain for Walrus backend: {}",
                descriptor.chain
            ));
        }

        info!("Retrieving proof from Walrus: {}", descriptor.hash);

        let params = ReadFromWalrusParams {
            blob_id: descriptor.hash.clone(),
            quilt_identifier: None,
        };

        match self.client.read_from_walrus(params).await {
            Ok(Some(data)) => {
                // Try to parse as wrapped data with metadata
                if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&data) {
                    if let Some(proof) = wrapper.get("proof").and_then(|p| p.as_str()) {
                        let mut metadata = ProofMetadata::default();

                        if let Some(meta_obj) = wrapper.get("metadata").and_then(|m| m.as_object())
                        {
                            for (key, value) in meta_obj {
                                if let Some(val_str) = value.as_str() {
                                    metadata.data.insert(key.clone(), val_str.to_string());
                                }
                            }
                        }

                        if let Some(expires) = wrapper.get("expires_at").and_then(|e| e.as_u64()) {
                            metadata.expires_at = Some(expires);
                        }

                        info!("Successfully retrieved proof from Walrus with metadata");
                        return Ok((proof.to_string(), metadata));
                    }
                }

                // If not wrapped, return raw data
                info!("Successfully retrieved proof from Walrus (raw data)");
                Ok((data, ProofMetadata::default()))
            }
            Ok(None) => Err(anyhow!("Proof not found in Walrus: {}", descriptor.hash)),
            Err(e) => Err(anyhow!("Failed to read proof from Walrus: {}", e)),
        }
    }

    async fn store_quilt(
        &self,
        mut quilt_data: Vec<(String, String)>,
        metadata: Option<ProofMetadata>,
    ) -> Result<QuiltStorageResult> {
        // Always add metadata as a quilt piece
        if let Some(ref meta) = metadata {
            let metadata_json = serde_json::json!({
                "chain": meta.data.get("sui_chain").cloned().unwrap_or_else(|| "unknown".to_string()),
                "app_instance": meta.data.get("app_instance_id").cloned().unwrap_or_else(|| "unknown".to_string()),
                "block_numbers": quilt_data.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
                "metadata": meta.data,
                "created_at": chrono::Utc::now().to_rfc3339(),
            });
            quilt_data.push((
                "metadata".to_string(),
                serde_json::to_string(&metadata_json)?,
            ));
        }

        info!(
            "Storing quilt to Walrus ({}) with {} pieces (including metadata)",
            self.network,
            quilt_data.len()
        );

        let params = SaveToWalrusParams {
            data: WalrusData::Quilt(quilt_data),
            address: None,
            num_epochs: Some(53), // Long-term storage for quilts
        };

        match self.client.save_to_walrus(params).await {
            Ok(Some(result)) => {
                info!("✅ Successfully stored quilt to Walrus:");
                info!("  BlobId: {}", result.blob_id);
                if let Some(tx) = &result.tx_digest {
                    info!("  TxDigest: {}", tx);
                }

                let patches = result.quilt_patches.clone().unwrap_or_default();
                if !patches.is_empty() {
                    info!("  Quilt patches ({} total):", patches.len());
                    for patch in &patches {
                        info!("    - {}: {}", patch.identifier, patch.quilt_patch_id);
                    }
                }

                Ok(QuiltStorageResult {
                    descriptor: ProofDescriptor {
                        chain: "walrus".to_string(),
                        network: self.network.clone(),
                        hash: result.blob_id.clone(),
                    },
                    blob_id: result.blob_id,
                    tx_digest: result.tx_digest,
                    quilt_patches: patches,
                })
            }
            Ok(None) => Err(anyhow!("Walrus returned no result for quilt storage")),
            Err(e) => Err(anyhow!("Failed to store quilt to Walrus: {}", e)),
        }
    }

    async fn proof_exists(&self, descriptor: &ProofDescriptor) -> Result<bool> {
        if descriptor.chain != "walrus" {
            return Ok(false);
        }

        // Try to get the URL - if it succeeds, the proof exists
        let params = GetWalrusUrlParams {
            blob_id: descriptor.hash.clone(),
        };

        match self.client.get_walrus_url(params) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// Cache storage backend implementation (via gRPC)
pub struct CacheStorage {
    network: String,
}

impl CacheStorage {
    pub fn new(network: String) -> Self {
        CacheStorage { network }
    }
}

#[async_trait]
impl ProofStorageBackend for CacheStorage {
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor> {
        info!(
            "Storing proof to cache via gRPC ({}) - data size: {} bytes",
            self.network,
            proof_data.len()
        );

        let mut client = rpc_client::shared::get_shared_client()
            .await
            .map_err(|e| anyhow!("Failed to get shared RPC client: {}", e))?;

        let mut grpc_metadata = HashMap::new();
        let mut expires_at = None;

        if let Some(meta) = metadata {
            grpc_metadata = meta.data;
            expires_at = meta.expires_at;
        }

        let request = Request::new(SubmitProofRequest {
            data: Some(proto::submit_proof_request::Data::ProofData(
                proof_data.to_string(),
            )),
            metadata: grpc_metadata,
            expires_at,
        });

        match client.submit_proof(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.success {
                    info!("Successfully stored proof to cache: {}", resp.proof_hash);
                    Ok(ProofDescriptor {
                        chain: "cache".to_string(),
                        network: self.network.clone(),
                        hash: resp.proof_hash,
                    })
                } else {
                    Err(anyhow!("Failed to store proof to cache: {}", resp.message))
                }
            }
            Err(e) => {
                // Log full error details
                warn!(
                    "gRPC error storing proof - Status: {:?}, Message: {:?}, Details: {:?}",
                    e.code(),
                    e.message(),
                    e.details()
                );
                Err(anyhow!("gRPC error storing proof: {}", e))
            }
        }
    }

    async fn get_proof(&self, descriptor: &ProofDescriptor) -> Result<(String, ProofMetadata)> {
        if descriptor.chain != "cache" {
            return Err(anyhow!(
                "Invalid chain for cache backend: {}",
                descriptor.chain
            ));
        }

        info!("Retrieving proof from cache: {}", descriptor.hash);

        let mut client = rpc_client::shared::get_shared_client()
            .await
            .map_err(|e| anyhow!("Failed to get shared RPC client: {}", e))?;

        let request = Request::new(GetProofRequest {
            proof_hash: descriptor.hash.clone(),
            block_number: None, // TODO: Support block_number for quilt piece retrieval
        });

        match client.get_proof(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.success {
                    let metadata = ProofMetadata {
                        data: resp.metadata,
                        expires_at: None, // Note: expires_at is in metadata as a tag
                    };

                    info!("Successfully retrieved proof from cache");
                    Ok((resp.proof_data, metadata))
                } else {
                    Err(anyhow!("Failed to get proof from cache: {}", resp.message))
                }
            }
            Err(e) => Err(anyhow!("gRPC error getting proof: {}", e)),
        }
    }

    async fn store_quilt(
        &self,
        quilt_data: Vec<(String, String)>,
        metadata: Option<ProofMetadata>,
    ) -> Result<QuiltStorageResult> {
        warn!(
            "Storing quilt to cache via gRPC ({}) with {} pieces",
            self.network,
            quilt_data.len()
        );

        let mut client = rpc_client::shared::get_shared_client()
            .await
            .map_err(|e| anyhow!("Failed to get shared RPC client: {}", e))?;

        let mut grpc_metadata = HashMap::new();
        let mut expires_at = None;

        if let Some(meta) = metadata {
            grpc_metadata = meta.data;
            expires_at = meta.expires_at;
        }

        // Convert quilt data to proto format
        let quilt_pieces: Vec<proto::QuiltPiece> = quilt_data
            .iter()
            .map(|(identifier, data)| proto::QuiltPiece {
                identifier: identifier.clone(),
                data: data.clone(),
            })
            .collect();

        let request = Request::new(SubmitProofRequest {
            data: Some(proto::submit_proof_request::Data::QuiltData(
                proto::QuiltData {
                    pieces: quilt_pieces,
                },
            )),
            metadata: grpc_metadata,
            expires_at,
        });

        match client.submit_proof(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                if resp.success {
                    warn!("✅ Successfully stored quilt to cache:");
                    warn!("  Hash: {}", resp.proof_hash);
                    if !resp.quilt_piece_ids.is_empty() {
                        warn!("  Quilt pieces ({} total):", resp.quilt_piece_ids.len());
                        for (i, piece_id) in resp.quilt_piece_ids.iter().enumerate() {
                            if i < quilt_data.len() {
                                warn!("    - {}: {}", quilt_data[i].0, piece_id);
                            }
                        }
                    }

                    // Create QuiltPatch structures for compatibility
                    let patches: Vec<QuiltPatch> = quilt_data
                        .iter()
                        .enumerate()
                        .map(|(i, (identifier, _))| QuiltPatch {
                            identifier: identifier.clone(),
                            quilt_patch_id: resp.quilt_piece_ids.get(i).cloned().unwrap_or_else(
                                || format!("cache:{}:{}", resp.proof_hash, identifier),
                            ),
                        })
                        .collect();

                    Ok(QuiltStorageResult {
                        descriptor: ProofDescriptor {
                            chain: "cache".to_string(),
                            network: self.network.clone(),
                            hash: resp.proof_hash.clone(),
                        },
                        blob_id: resp.proof_hash,
                        tx_digest: None, // Cache doesn't have blockchain transactions
                        quilt_patches: patches,
                    })
                } else {
                    Err(anyhow!("Failed to store quilt to cache: {}", resp.message))
                }
            }
            Err(e) => {
                error!(
                    "gRPC error storing quilt - Status: {:?}, Message: {:?}, Details: {:?}",
                    e.code(),
                    e.message(),
                    e.details()
                );
                Err(anyhow!("gRPC error storing quilt: {}", e))
            }
        }
    }

    async fn proof_exists(&self, descriptor: &ProofDescriptor) -> Result<bool> {
        if descriptor.chain != "cache" {
            return Ok(false);
        }

        // Try to get the proof - if it succeeds, it exists
        match self.get_proof(descriptor).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// S3 storage backend implementation
pub struct S3Storage {
    bucket: String,
    network: String,
}

impl S3Storage {
    #[allow(dead_code)]
    pub fn new(bucket: String, network: String) -> Self {
        S3Storage { bucket, network }
    }

    async fn get_client(&self) -> Result<S3Client> {
        S3Client::new(self.bucket.clone()).await
    }
}

#[async_trait]
impl ProofStorageBackend for S3Storage {
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor> {
        info!("Storing proof to S3 ({}/{})", self.bucket, self.network);

        // Generate a unique key for the proof
        let proof_hash = format!("{:x}", sha2::Sha256::digest(proof_data.as_bytes()));
        let key = format!("{}/proofs/{}", self.network, proof_hash);

        // Convert metadata to tags format for S3
        let tags = if let Some(ref meta) = metadata {
            let mut tag_vec = Vec::new();
            for (k, v) in &meta.data {
                tag_vec.push((k.clone(), v.clone()));
            }
            if let Some(expires) = meta.expires_at {
                tag_vec.push(("expires_at".to_string(), expires.to_string()));
            }
            Some(tag_vec)
        } else {
            None
        };

        // Store to S3
        let client = self.get_client().await?;
        let expires_at = metadata
            .as_ref()
            .and_then(|m| m.expires_at)
            .unwrap_or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_millis() as u64;
                now + (2 * 24 * 60 * 60 * 1000) // 2 days default
            });

        client
            .write(&key, proof_data.to_string(), tags, expires_at)
            .await
            .map_err(|e| anyhow!("Failed to store proof to S3: {}", e))?;

        info!("Successfully stored proof to S3: {}", key);
        Ok(ProofDescriptor {
            chain: "s3".to_string(),
            network: self.network.clone(),
            hash: proof_hash,
        })
    }

    async fn store_quilt(
        &self,
        quilt_data: Vec<(String, String)>,
        metadata: Option<ProofMetadata>,
    ) -> Result<QuiltStorageResult> {
        warn!(
            "Storing quilt to S3 ({}/{}) with {} pieces",
            self.bucket,
            self.network,
            quilt_data.len()
        );

        // Create a JSON representation of the quilt
        let quilt_json = serde_json::json!({
            "type": "quilt",
            "pieces": quilt_data.iter().map(|(id, data)| {
                serde_json::json!({
                    "identifier": id,
                    "data": data,
                })
            }).collect::<Vec<_>>(),
            "metadata": metadata.as_ref().map(|m| &m.data),
            "created_at": chrono::Utc::now().to_rfc3339(),
        });

        let quilt_str = serde_json::to_string_pretty(&quilt_json)?;
        let quilt_hash = format!("{:x}", sha2::Sha256::digest(quilt_str.as_bytes()));
        let key = format!("{}/quilts/{}", self.network, quilt_hash);

        // Store metadata as tags
        let mut tag_vec = Vec::new();
        tag_vec.push(("type".to_string(), "quilt".to_string()));
        tag_vec.push(("pieces_count".to_string(), quilt_data.len().to_string()));
        tag_vec.push(("quilts".to_string(), "true".to_string())); // Flag for quilt identification

        if let Some(ref meta) = metadata {
            for (k, v) in &meta.data {
                tag_vec.push((k.clone(), v.clone()));
            }
        }

        // Store identifiers as a tag for searchability
        let identifiers: Vec<String> = quilt_data.iter().map(|(id, _)| id.clone()).collect();
        tag_vec.push(("identifiers".to_string(), identifiers.join(",")));

        // Store to S3
        let client = self.get_client().await?;
        let expires_at = metadata
            .as_ref()
            .and_then(|m| m.expires_at)
            .unwrap_or_else(|| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_millis() as u64;
                now + (7 * 24 * 60 * 60 * 1000) // 7 days default for quilts
            });

        client
            .write(&key, quilt_str, Some(tag_vec), expires_at)
            .await
            .map_err(|e| anyhow!("Failed to store quilt to S3: {}", e))?;

        warn!("✅ Successfully stored quilt to S3:");
        warn!("  Key: {}", key);
        warn!("  Hash: {}", quilt_hash);
        warn!("  Pieces: {}", identifiers.join(", "));

        // Create mock quilt patches for compatibility
        let patches: Vec<QuiltPatch> = quilt_data
            .iter()
            .map(|(id, _)| QuiltPatch {
                identifier: id.clone(),
                quilt_patch_id: format!("s3:{}:{}", quilt_hash, id),
            })
            .collect();

        Ok(QuiltStorageResult {
            descriptor: ProofDescriptor {
                chain: "s3".to_string(),
                network: self.network.clone(),
                hash: quilt_hash.clone(),
            },
            blob_id: quilt_hash,
            tx_digest: None, // S3 doesn't have blockchain transactions
            quilt_patches: patches,
        })
    }

    async fn get_proof(&self, descriptor: &ProofDescriptor) -> Result<(String, ProofMetadata)> {
        if descriptor.chain != "s3" {
            return Err(anyhow!(
                "Invalid chain for S3 backend: {}",
                descriptor.chain
            ));
        }

        info!("Retrieving proof from S3: {}", descriptor.hash);

        // Check both proofs and quilts paths
        let proof_key = format!("{}/proofs/{}", self.network, descriptor.hash);

        // Try proof path first
        let client = self.get_client().await?;
        let result = match client.read(&proof_key).await {
            Ok(result) => result,
            Err(_) => {
                // Try quilt path (remove the network prefix for key)
                let quilt_short_key = descriptor.hash.clone();
                client
                    .read(&quilt_short_key)
                    .await
                    .map_err(|e| anyhow!("Failed to retrieve from S3: {}", e))?
            }
        };
        let content = result.data;
        let tags = result.metadata;

        // Convert tags to metadata
        let mut metadata = ProofMetadata::default();
        for (key, value) in tags {
            if key == "expires_at" {
                if let Ok(expires) = value.parse::<u64>() {
                    metadata.expires_at = Some(expires);
                }
            } else {
                metadata.data.insert(key, value);
            }
        }

        info!("Successfully retrieved from S3");
        Ok((content, metadata))
    }

    async fn proof_exists(&self, descriptor: &ProofDescriptor) -> Result<bool> {
        if descriptor.chain != "s3" {
            return Ok(false);
        }

        // Check both paths
        let proof_key = format!("{}/proofs/{}", self.network, descriptor.hash);

        let client = self.get_client().await?;
        let quilt_key = format!("{}/quilts/{}", self.network, descriptor.hash);
        Ok(client.exists(&proof_key).await? || client.exists(&quilt_key).await?)
    }
}

/// Main proof storage manager that routes to different backends
pub struct ProofStorage {
    walrus: Option<WalrusStorage>,
    cache: Option<CacheStorage>,
    s3: Option<S3Storage>,
}

impl ProofStorage {
    /// Create a new proof storage manager with configured backends
    pub fn new() -> Self {
        // Get SUI_CHAIN from environment, default to devnet
        let sui_chain = std::env::var("SUI_CHAIN").unwrap_or_else(|_| "devnet".to_string());

        // Map SUI chain to walrus network (devnet/testnet use walrus testnet, mainnet uses walrus mainnet)
        let walrus_network = if sui_chain == "mainnet" {
            "mainnet".to_string()
        } else {
            "testnet".to_string()
        };

        // Initialize backends based on environment configuration
        let walrus = Some(WalrusStorage::new(walrus_network));

        // Use shared gRPC client for cache storage if SILVANA_RPC_SERVER is configured
        let cache = if std::env::var("SILVANA_RPC_SERVER").is_ok()
            || std::env::var("RPC_ENDPOINT").is_ok()
        {
            info!("Cache storage enabled via shared gRPC client");
            Some(CacheStorage::new("public".to_string()))
        } else {
            warn!("SILVANA_RPC_SERVER not set, cache storage disabled");
            None
        };

        ProofStorage {
            walrus,
            cache,
            s3: None,
        }
    }

    /// Store a proof using the specified chain
    pub async fn store_proof(
        &self,
        chain: &str,
        _network: &str,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor> {
        match chain {
            "walrus" => {
                if let Some(ref walrus) = self.walrus {
                    walrus.store_proof(proof_data, metadata).await
                } else {
                    Err(anyhow!("Walrus storage not configured"))
                }
            }
            "cache" => {
                if let Some(ref cache) = self.cache {
                    cache.store_proof(proof_data, metadata).await
                } else {
                    Err(anyhow!("Cache storage not configured"))
                }
            }
            _ => Err(anyhow!("Unsupported storage chain: {}", chain)),
        }
    }

    /// Store multiple proofs as a quilt using the specified chain
    pub async fn store_quilt(
        &self,
        chain: &str,
        _network: &str,
        quilt_data: Vec<(String, String)>,
        metadata: Option<ProofMetadata>,
    ) -> Result<QuiltStorageResult> {
        match chain {
            "walrus" => {
                if let Some(ref walrus) = self.walrus {
                    walrus.store_quilt(quilt_data, metadata).await
                } else {
                    Err(anyhow!("Walrus storage not configured"))
                }
            }
            "cache" => {
                if let Some(ref cache) = self.cache {
                    cache.store_quilt(quilt_data, metadata).await
                } else {
                    Err(anyhow!("Cache storage not configured"))
                }
            }
            "s3" => {
                if let Some(ref s3) = self.s3 {
                    s3.store_quilt(quilt_data, metadata).await
                } else {
                    Err(anyhow!("S3 storage not configured"))
                }
            }
            _ => Err(anyhow!("Unsupported storage chain: {}", chain)),
        }
    }

    /// Get a proof using its descriptor
    pub async fn get_proof(&self, descriptor_str: &str) -> Result<(String, ProofMetadata)> {
        let descriptor = ProofDescriptor::parse(descriptor_str)?;

        match descriptor.chain.as_str() {
            "walrus" => {
                if let Some(ref walrus) = self.walrus {
                    walrus.get_proof(&descriptor).await
                } else {
                    Err(anyhow!("Walrus storage not configured"))
                }
            }
            "cache" => {
                if let Some(ref cache) = self.cache {
                    cache.get_proof(&descriptor).await
                } else {
                    Err(anyhow!("Cache storage not configured"))
                }
            }
            _ => Err(anyhow!("Unsupported storage chain: {}", descriptor.chain)),
        }
    }

    /// Check if a proof exists
    #[allow(dead_code)]
    pub async fn proof_exists(&self, descriptor_str: &str) -> Result<bool> {
        let descriptor = ProofDescriptor::parse(descriptor_str)?;

        match descriptor.chain.as_str() {
            "walrus" => {
                if let Some(ref walrus) = self.walrus {
                    walrus.proof_exists(&descriptor).await
                } else {
                    Ok(false)
                }
            }
            "cache" => {
                if let Some(ref cache) = self.cache {
                    cache.proof_exists(&descriptor).await
                } else {
                    Ok(false)
                }
            }
            "s3" => {
                if let Some(ref s3) = self.s3 {
                    s3.proof_exists(&descriptor).await
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_parsing() {
        let descriptor_str = "walrus:testnet:s04UEUvCT-CTJVKsPHDWWMWcagr2mzDJHlnFm-dHOeo";
        let descriptor = ProofDescriptor::parse(descriptor_str).unwrap();

        assert_eq!(descriptor.chain, "walrus");
        assert_eq!(descriptor.network, "testnet");
        assert_eq!(
            descriptor.hash,
            "s04UEUvCT-CTJVKsPHDWWMWcagr2mzDJHlnFm-dHOeo"
        );
        assert_eq!(descriptor.to_string(), descriptor_str);
    }

    #[test]
    fn test_invalid_descriptor() {
        let invalid = "walrus:testnet";
        assert!(ProofDescriptor::parse(invalid).is_err());

        let invalid = "walrus";
        assert!(ProofDescriptor::parse(invalid).is_err());
    }

    #[test]
    fn test_various_descriptors() {
        let descriptors = vec![
            "ipfs:public:fhkvrefvejhfvjrvfekkkrfvefv",
            "s3:private:vhrevkevekkvvbhreb",
            "cache:public:hjfvhjvjhdv",
        ];

        for desc_str in descriptors {
            let descriptor = ProofDescriptor::parse(desc_str).unwrap();
            assert_eq!(descriptor.to_string(), desc_str);
        }
    }

    #[tokio::test]
    async fn test_proof_storage_creation() {
        let storage = ProofStorage::new();

        // Walrus should always be available
        assert!(storage.walrus.is_some());

        // Cache depends on SILVANA_RPC_SERVER or RPC_ENDPOINT environment variable
        if std::env::var("SILVANA_RPC_SERVER").is_ok() || std::env::var("RPC_ENDPOINT").is_ok() {
            assert!(storage.cache.is_some());
        } else {
            assert!(storage.cache.is_none());
        }
    }
}
