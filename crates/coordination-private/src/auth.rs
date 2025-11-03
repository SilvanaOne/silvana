//! Ed25519 authentication for coordinator requests

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hex;
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{PrivateCoordinationError, Result};

/// Coordinator authentication helper
pub struct CoordinatorAuth {
    signing_key: SigningKey,
    public_key_hex: String,
}

impl CoordinatorAuth {
    /// Create from hex-encoded private key
    pub fn from_private_key_hex(private_key_hex: &str) -> Result<Self> {
        let key_bytes = hex::decode(private_key_hex)
            .map_err(|e| PrivateCoordinationError::InvalidInput(
                format!("Invalid private key hex: {}", e)
            ))?;

        if key_bytes.len() != 32 {
            return Err(PrivateCoordinationError::InvalidInput(
                "Private key must be 32 bytes".to_string()
            ));
        }

        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);

        let signing_key = SigningKey::from_bytes(&key_array);
        let verifying_key: VerifyingKey = (&signing_key).into();
        let public_key_hex = hex::encode(verifying_key.to_bytes());

        Ok(Self {
            signing_key,
            public_key_hex,
        })
    }

    /// Get public key as hex string
    pub fn public_key(&self) -> &str {
        &self.public_key_hex
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
