//! Ed25519 authentication for coordinator requests

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hex;
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{PrivateCoordinationError, Result};

/// Coordinator authentication helper
pub struct CoordinatorAuth {
    signing_key: SigningKey,
    secret_key_bytes: [u8; 32],  // Store raw secret for JWT creation
    public_key_hex: String,
}

impl CoordinatorAuth {
    /// Create from hex-encoded OR bech32-encoded (suiprivkey) private key
    pub fn from_private_key(private_key: &str) -> Result<Self> {
        // Try parsing as Sui bech32 format first
        let secret_key_bytes = if private_key.starts_with("suiprivkey") {
            sui::keypair::parse_sui_private_key(private_key)
                .map_err(|e| PrivateCoordinationError::InvalidInput(
                    format!("Invalid Sui bech32 private key: {}", e)
                ))?
        } else {
            // Fall back to hex format
            let bytes = hex::decode(private_key)
                .map_err(|e| PrivateCoordinationError::InvalidInput(
                    format!("Invalid private key (expected hex or suiprivkey bech32): {}", e)
                ))?;

            if bytes.len() != 32 {
                return Err(PrivateCoordinationError::InvalidInput(
                    "Private key must be 32 bytes".to_string()
                ));
            }

            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&bytes);
            key_array
        };

        let signing_key = SigningKey::from_bytes(&secret_key_bytes);
        let verifying_key: VerifyingKey = (&signing_key).into();
        let public_key_hex = hex::encode(verifying_key.to_bytes());

        Ok(Self {
            signing_key,
            secret_key_bytes,
            public_key_hex,
        })
    }

    /// Get public key as hex string
    pub fn public_key(&self) -> &str {
        &self.public_key_hex
    }

    /// Get the raw secret key bytes (for JWT creation)
    pub fn secret_key_bytes(&self) -> &[u8; 32] {
        &self.secret_key_bytes
    }

    /// Sign a coordinator request
    pub fn sign_request(
        &self,
        app_instance_id: &str,
        operation: &str,
    ) -> proto::silvana::state::v1::CoordinatorAuth {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let nonce = Uuid::new_v4().to_string();

        // Create deterministic message to sign
        let message = format!(
            "{}:{}:{}:{}:{}",
            self.public_key_hex,
            app_instance_id,
            operation,
            timestamp,
            nonce
        );

        let signature: Signature = self.signing_key.sign(message.as_bytes());

        proto::silvana::state::v1::CoordinatorAuth {
            coordinator_public_key: self.public_key_hex.clone(),
            signature: hex::encode(signature.to_bytes()),
            timestamp,
            nonce,
        }
    }
}
