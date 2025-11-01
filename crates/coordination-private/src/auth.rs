//! JWT authentication for Private Coordination layer

use crate::error::{PrivateCoordinationError, Result};
use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject - the public key of the signer
    pub sub: String,

    /// App instance ID
    pub app_instance_id: String,

    /// Issued at timestamp
    pub iat: u64,

    /// Expiry timestamp
    pub exp: u64,

    /// Optional nonce for replay protection
    pub nonce: Option<String>,
}

/// JWT authentication handler
pub struct JwtAuth {
    secret: Vec<u8>,
}

impl JwtAuth {
    /// Create a new JWT auth handler
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
        }
    }

    /// Create a JWT token
    pub fn create_token(&self, public_key: &str, app_instance_id: &str, validity_secs: u64) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| PrivateCoordinationError::Other(anyhow::anyhow!("Time error: {}", e)))?
            .as_secs();

        let claims = JwtClaims {
            sub: public_key.to_string(),
            app_instance_id: app_instance_id.to_string(),
            iat: now,
            exp: now + validity_secs,
            nonce: Some(uuid::Uuid::new_v4().to_string()),
        };

        let header = Header::new(Algorithm::HS256);
        let token = encode(
            &header,
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )?;

        Ok(token)
    }

    /// Verify a JWT token and extract claims
    pub fn verify_token(&self, token: &str) -> Result<JwtClaims> {
        let validation = Validation::new(Algorithm::HS256);
        let token_data = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(&self.secret),
            &validation,
        )?;

        // Check if token is expired
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| PrivateCoordinationError::Other(anyhow::anyhow!("Time error: {}", e)))?
            .as_secs();

        if token_data.claims.exp < now {
            return Err(PrivateCoordinationError::Authentication("Token expired".to_string()));
        }

        Ok(token_data.claims)
    }

    /// Verify Ed25519 signature (for additional security)
    pub fn verify_ed25519_signature(
        public_key_hex: &str,
        message: &[u8],
        signature_hex: &str,
    ) -> Result<bool> {
        let public_key_bytes = hex::decode(public_key_hex)?;
        let signature_bytes = hex::decode(signature_hex)?;

        if public_key_bytes.len() != 32 {
            return Err(PrivateCoordinationError::InvalidInput(
                "Invalid public key length".to_string(),
            ));
        }

        if signature_bytes.len() != 64 {
            return Err(PrivateCoordinationError::InvalidInput(
                "Invalid signature length".to_string(),
            ));
        }

        let public_key_array: [u8; 32] = public_key_bytes.try_into()
            .map_err(|_| PrivateCoordinationError::InvalidInput("Invalid public key length".to_string()))?;

        let public_key = VerifyingKey::from_bytes(&public_key_array)
            .map_err(|e| PrivateCoordinationError::Other(anyhow::anyhow!("Invalid public key: {}", e)))?;

        let signature_array: [u8; 64] = signature_bytes.try_into()
            .map_err(|_| PrivateCoordinationError::InvalidInput("Invalid signature length".to_string()))?;

        let signature = Signature::from_bytes(&signature_array);

        Ok(public_key.verify(message, &signature).is_ok())
    }
}

/// Extract app_instance_id from authorization header
pub fn extract_app_instance_from_auth(auth_header: &str, jwt_auth: &JwtAuth) -> Result<String> {
    // Remove "Bearer " prefix if present
    let token = if auth_header.starts_with("Bearer ") {
        &auth_header[7..]
    } else {
        auth_header
    };

    let claims = jwt_auth.verify_token(token)?;
    Ok(claims.app_instance_id)
}