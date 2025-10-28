//! S3 storage integration for private authenticated blob storage

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use storage::S3Client;
use tracing::{debug, error, info, warn};

use crate::auth::ScopePermission;

/// Private authenticated S3 storage client
#[derive(Clone)]
pub struct PrivateStateStorage {
    client: Arc<S3Client>,
}

impl PrivateStateStorage {
    /// Create a new S3 storage client
    pub async fn new(bucket: String) -> Result<Self> {
        let client = S3Client::new(bucket).await?;
        info!("Connected to S3 for private state storage");

        Ok(Self {
            client: Arc::new(client),
        })
    }

    /// Store blob with owner metadata (authenticated)
    /// Returns storage key in format: resource_type/resource_id/hash
    pub async fn store(
        &self,
        data: &[u8],
        owner: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<String> {
        // Generate content-addressable key using SHA256
        let hash = Sha256::digest(data);
        let hash_hex = hex::encode(hash);
        // Key format: resource_type/resource_id/hash
        let storage_key = format!("{}/{}/{}", resource_type, resource_id, hash_hex);

        debug!("Storing {} bytes to S3: {} (owner: {})", data.len(), storage_key, owner);

        // Check if object already exists
        if self.client.exists(&storage_key).await.unwrap_or(false) {
            debug!("Object already exists in S3: {}", storage_key);
            return Ok(storage_key);
        }

        // Store metadata including owner and access control info
        let metadata = vec![
            ("content-hash".to_string(), hash_hex.clone()),
            ("content-length".to_string(), data.len().to_string()),
            ("owner".to_string(), owner.to_string()),
            ("resource_type".to_string(), resource_type.to_string()),
            ("resource_id".to_string(), resource_id.to_string()),
        ];

        // Upload to S3 using write_binary (stores binary data directly, no base64)
        self.client
            .write_binary(
                data.to_vec(),
                &storage_key,
                "application/octet-stream",
                Some(metadata),
                Some(hash_hex.clone()),
            )
            .await
            .map_err(|e| anyhow!("Failed to store data in S3: {}", e))?;

        info!("Stored {} bytes to S3: {} (owner: {})", data.len(), storage_key, owner);
        Ok(storage_key)
    }

    /// Retrieve blob with authentication
    pub async fn retrieve(
        &self,
        s3_key: &str,
        requester_pubkey: &str,
        scope_permissions: &[ScopePermission],
    ) -> Result<Vec<u8>> {
        debug!("Retrieving from S3: {} (requester: {})", s3_key, requester_pubkey);

        // First, read only metadata to check authentication WITHOUT downloading the binary
        // This is much more efficient for large files
        let metadata = self.client
            .read_metadata(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to read blob metadata: {}", e))?;

        // Check access control
        let owner = metadata.get("owner")
            .map(|v| v.as_str())
            .ok_or_else(|| anyhow!("Blob has no owner metadata"))?;

        // Allow if requester is owner
        let is_owner = owner == requester_pubkey;

        // Or if requester has scope permission for the resource
        let has_scope_permission = if let Some(resource_type) = metadata.get("resource_type") {
            if let Some(resource_id) = metadata.get("resource_id") {
                match resource_type.as_str() {
                    "app_instance" => scope_permissions.iter().any(|p| {
                        matches!(p, ScopePermission::AppInstance(id) if id == resource_id)
                    }),
                    "object" => scope_permissions.iter().any(|p| {
                        matches!(p, ScopePermission::Object(id) if id == resource_id)
                    }),
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        };

        if !is_owner && !has_scope_permission {
            warn!(
                "Access denied for blob {}: requester {} is not owner {} and has no scope permission",
                s3_key, requester_pubkey, owner
            );
            return Err(anyhow!("Access denied: you are not authorized to access this blob"));
        }

        // Auth passed - now download the actual binary data
        debug!("Auth passed, downloading binary data from S3: {}", s3_key);
        let result = self.client
            .read_binary(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to retrieve from S3: {}", e))?;

        debug!("Retrieved {} bytes from S3", result.data.len());

        // Verify hash matches the key (for content-addressed storage)
        if s3_key.contains('/') {
            let parts: Vec<&str> = s3_key.split('/').collect();
            if let Some(expected_hash) = parts.last() {
                let actual_hash = hex::encode(Sha256::digest(&result.data));
                if actual_hash != *expected_hash {
                    error!(
                        "S3 data integrity check failed for {}: expected {}, got {}",
                        s3_key, expected_hash, actual_hash
                    );
                    return Err(anyhow!("S3 data integrity check failed"));
                }
            }
        }

        info!("Successfully retrieved {} bytes from S3: {} (requester: {})", result.data.len(), s3_key, requester_pubkey);
        Ok(result.data)
    }

    /// Delete blob (owner only)
    pub async fn delete(&self, s3_key: &str, requester_pubkey: &str) -> Result<()> {
        debug!("Deleting from S3: {} (requester: {})", s3_key, requester_pubkey);

        // Read only metadata to check ownership WITHOUT downloading the binary
        let metadata = self.client
            .read_metadata(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to read blob metadata: {}", e))?;

        let owner = metadata.get("owner")
            .map(|v| v.as_str())
            .ok_or_else(|| anyhow!("Blob has no owner metadata"))?;

        // Only owner can delete
        if owner != requester_pubkey {
            warn!(
                "Delete denied for blob {}: requester {} is not owner {}",
                s3_key, requester_pubkey, owner
            );
            return Err(anyhow!("Access denied: only the owner can delete this blob"));
        }

        self.client
            .delete(s3_key)
            .await
            .map_err(|e| anyhow!("Failed to delete from S3: {}", e))?;

        info!("Deleted from S3: {} (requester: {})", s3_key, requester_pubkey);
        Ok(())
    }

    /// Check if an object exists in S3
    pub async fn exists(&self, s3_key: &str) -> Result<bool> {
        self.client.exists(s3_key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_generation() {
        let data = b"test data";
        let hash = Sha256::digest(data);
        let hash_hex = hex::encode(hash);

        // SHA256 should produce 64 character hex string
        assert_eq!(hash_hex.len(), 64);
    }
}