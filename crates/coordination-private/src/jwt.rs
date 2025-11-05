//! JWT creation for coordinator authentication with state service

use ed25519_dalek::{Signer, SigningKey};
use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// JWT claims structure (matching state/src/auth.rs)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Claims {
    pub sub: String,      // Ed25519 public key (hex)
    pub exp: u64,         // Expiration timestamp
    pub iat: u64,         // Issued at timestamp
    pub aud: String,      // Audience
    pub scope: String,    // Space-separated permissions
}

/// Create a JWT token for coordinator access to state service
///
/// This creates a JWT token that can be used to authenticate with the state service
/// for agent operations. The token is signed with the coordinator's Ed25519 private key.
///
/// # Arguments
/// * `secret_key_bytes` - The 32-byte Ed25519 secret key
/// * `public_key_hex` - The hex-encoded Ed25519 public key
/// * `app_instance_id` - The app instance ID to grant permissions for
/// * `duration_secs` - How long the token should be valid (in seconds)
///
/// # Returns
/// A JWT token string that can be used in JWTAuth messages
pub fn create_coordinator_jwt(
    secret_key_bytes: &[u8; 32],
    public_key_hex: &str,
    app_instance_id: &str,
    duration_secs: u64,
) -> Result<String> {
    let signing_key = SigningKey::from_bytes(secret_key_bytes);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    // Scope: app_instance permission for this coordinator
    let scope = format!("app_instance {}", app_instance_id);

    // Audience from SILVANA_AUD env var (default: "silvana-state")
    let aud = std::env::var("SILVANA_AUD").unwrap_or_else(|_| "silvana-state".to_string());

    let claims = Claims {
        sub: public_key_hex.to_string(),
        exp: now + duration_secs,
        iat: now,
        aud,
        scope,
    };

    // Create JWT header (EdDSA algorithm)
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);

    // Encode header and payload
    let header_b64 = base64_url_encode(&serde_json::to_vec(&header)?);
    let payload_b64 = base64_url_encode(&serde_json::to_vec(&claims)?);

    // Sign the token
    let message = format!("{}.{}", header_b64, payload_b64);
    let signature = signing_key.sign(message.as_bytes());
    let signature_b64 = base64_url_encode(&signature.to_bytes());

    Ok(format!("{}.{}", message, signature_b64))
}

fn base64_url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn test_create_jwt() {
        unsafe { std::env::set_var("SILVANA_AUD", "test-service"); }

        let test_secret_key: [u8; 32] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32
        ];
        let signing_key = SigningKey::from_bytes(&test_secret_key);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.to_bytes());

        let jwt = create_coordinator_jwt(
            &test_secret_key,
            &public_key_hex,
            "test_app_123",
            3600,
        ).unwrap();

        // JWT should have 3 parts separated by dots
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Should not be empty
        assert!(!jwt.is_empty());
    }
}
