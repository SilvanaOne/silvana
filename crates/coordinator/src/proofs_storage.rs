use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::collections::HashMap;
use tracing::{info, warn};

// Re-export storage modules
use storage::walrus::{GetWalrusUrlParams, ReadFromWalrusParams, SaveToWalrusParams, WalrusClient};

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

/// Trait for proof storage backends
#[async_trait]
pub trait ProofStorageBackend: Send + Sync {
    /// Store a proof and return its descriptor
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor>;

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
            data: data_to_store,
            address: None,
            num_epochs: Some(2), // Default to max epochs TODO: calculate using expiires_at and epoch duration
        };

        match self.client.save_to_walrus(params).await {
            Ok(Some(blob_id)) => {
                info!("Successfully stored proof to Walrus: {}", blob_id);
                Ok(ProofDescriptor {
                    chain: "walrus".to_string(),
                    network: self.network.clone(),
                    hash: blob_id,
                })
            }
            Ok(None) => Err(anyhow!("Walrus returned no blob ID")),
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
        CacheStorage {
            network,
        }
    }
}

#[async_trait]
impl ProofStorageBackend for CacheStorage {
    async fn store_proof(
        &self,
        proof_data: &str,
        metadata: Option<ProofMetadata>,
    ) -> Result<ProofDescriptor> {
        info!("Storing proof to cache via gRPC ({})", self.network);

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
            proof_data: proof_data.to_string(),
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
            Err(e) => Err(anyhow!("gRPC error storing proof: {}", e)),
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

/// Main proof storage manager that routes to different backends
pub struct ProofStorage {
    walrus: Option<WalrusStorage>,
    cache: Option<CacheStorage>,
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
        let cache = if std::env::var("SILVANA_RPC_SERVER").is_ok() || std::env::var("RPC_ENDPOINT").is_ok() {
            info!("Cache storage enabled via shared gRPC client");
            Some(CacheStorage::new("public".to_string()))
        } else {
            warn!("SILVANA_RPC_SERVER not set, cache storage disabled");
            None
        };

        ProofStorage { walrus, cache }
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
