use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use aws_sdk_kms::{Client, primitives::Blob};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tracing::{info, error};

// Global static to store the KMS client
static KMS_CLIENT: OnceLock<Arc<Client>> = OnceLock::new();

/// Encrypted data container that includes both the encrypted data key and the encrypted content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    /// The encrypted data key from KMS (ciphertext blob for storage/later decryption)
    pub encrypted_data_key: Vec<u8>,
    /// The actual data encrypted with the plaintext data key
    pub encrypted_content: Vec<u8>,
    /// The nonce used for AES-GCM encryption
    pub nonce: Vec<u8>,
}

#[derive(Clone)]
pub struct KMS {
    client: Arc<Client>,
    key_id: String,
}

impl KMS {
    pub async fn new(key_id: impl Into<String>) -> Result<Self> {
        // Get or initialize the KMS client (reused across invocations)
        let client = if let Some(client) = KMS_CLIENT.get() {
            info!("Reusing existing KMS client from previous invocation");
            client.clone()
        } else {
            info!("Initializing new KMS client for first invocation");
            let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
            let new_client = Arc::new(Client::new(&config));
            
            // Try to set the client, but if another thread beat us to it, use theirs
            match KMS_CLIENT.set(new_client.clone()) {
                Ok(_) => {
                    info!("KMS client initialized and cached");
                    new_client
                }
                Err(_) => {
                    // Another thread initialized it first, use that one
                    info!("Another thread initialized KMS client, using that");
                    KMS_CLIENT.get()
                        .ok_or_else(|| anyhow!("KMS client not initialized despite concurrent set"))?
                        .clone()
                }
            }
        };

        Ok(Self {
            client,
            key_id: key_id.into(),
        })
    }

    pub async fn encrypt(&self, data: &[u8]) -> Result<EncryptedData> {
        info!("KMS: Generating data key...");
        let data_key_response = self
            .client
            .generate_data_key()
            .key_id(&self.key_id)
            .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to generate KMS data key: {}", e);
                anyhow!("Failed to generate KMS data key: {}", e)
            })?;

        let plaintext_key = data_key_response
            .plaintext()
            .ok_or_else(|| anyhow!("No plaintext key returned from KMS"))?;

        let encrypted_data_key = data_key_response
            .ciphertext_blob()
            .ok_or_else(|| anyhow!("No encrypted data key returned from KMS"))?;

        // Use the plaintext key for AES-GCM encryption
        let cipher = Aes256Gcm::new_from_slice(plaintext_key.as_ref())
            .map_err(|e| anyhow!("Failed to create AES cipher: {:?}", e))?;

        // Generate a random nonce
        let mut nonce_bytes = [0u8; 12]; // 96-bit nonce for GCM
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt the data
        let encrypted_content = cipher.encrypt(nonce, data)
            .map_err(|e| anyhow!("Failed to encrypt data: {:?}", e))?;

        Ok(EncryptedData {
            encrypted_data_key: encrypted_data_key.as_ref().to_vec(),
            encrypted_content,
            nonce: nonce_bytes.to_vec(),
        })
    }

    /// Decrypt data using KMS
    pub async fn decrypt(&self, encrypted_data: &EncryptedData) -> Result<Vec<u8>> {
        info!("KMS: Decrypting data key...");
        
        // Decrypt the data key using KMS
        let decrypt_response = self
            .client
            .decrypt()
            .ciphertext_blob(Blob::new(encrypted_data.encrypted_data_key.clone()))
            .key_id(&self.key_id)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to decrypt KMS data key: {}", e);
                anyhow!("Failed to decrypt KMS data key: {}", e)
            })?;

        let plaintext_key = decrypt_response
            .plaintext()
            .ok_or_else(|| anyhow!("No plaintext key returned from KMS decrypt"))?;

        // Use the plaintext key for AES-GCM decryption
        let cipher = Aes256Gcm::new_from_slice(plaintext_key.as_ref())
            .map_err(|e| anyhow!("Failed to create AES cipher: {}", e))?;

        // Reconstruct the nonce
        if encrypted_data.nonce.len() != 12 {
            return Err(anyhow!(
                "Invalid nonce length: expected 12 bytes, got {}",
                encrypted_data.nonce.len()
            ));
        }
        let nonce = Nonce::from_slice(&encrypted_data.nonce);

        // Decrypt the data
        let decrypted_data = cipher.decrypt(nonce, encrypted_data.encrypted_content.as_ref())
            .map_err(|e| anyhow!("Failed to decrypt data: {:?}", e))?;

        info!("KMS: Data decrypted");
        Ok(decrypted_data)
    }
}