//! JWT authentication with Ed25519 signatures

use anyhow::{anyhow, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, TokenData, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// JWT claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // Ed25519 public key (hex)
    pub app_instance_id: String,
    pub exp: u64,  // Expiration timestamp
    pub iat: u64,  // Issued at timestamp
}

/// Verification result containing authenticated information
#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub public_key: String,  // Ed25519 public key (hex)
    pub app_instance_id: String,
}

/// Verify a JWT token signed with Ed25519
pub fn verify_jwt(token: &str) -> Result<AuthInfo> {
    // Decode the header to ensure it's using EdDSA
    let header = decode_header(token)?;

    if header.alg != Algorithm::EdDSA {
        return Err(anyhow!("Invalid algorithm: expected EdDSA, got {:?}", header.alg));
    }

    // Split the token to get signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    // Decode the claims without verification first to get the public key
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.insecure_disable_signature_validation();

    let token_data = decode::<Claims>(token, &DecodingKey::from_secret(&[]), &validation)?;
    let claims = &token_data.claims;

    // Parse the public key from hex
    let public_key_bytes = hex::decode(&claims.sub)?;
    if public_key_bytes.len() != 32 {
        return Err(anyhow!("Invalid public key length"));
    }

    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap())?;

    // Create the message to verify (header.payload)
    let message = format!("{}.{}", parts[0], parts[1]);

    // Decode the signature from base64url
    let signature_bytes = base64_url_decode(parts[2])?;
    if signature_bytes.len() != 64 {
        return Err(anyhow!("Invalid signature length"));
    }

    let signature = Signature::from_bytes(&signature_bytes.try_into().unwrap());

    // Verify the signature
    public_key.verify(message.as_bytes(), &signature)
        .map_err(|_| anyhow!("Invalid signature"))?;

    // Check expiration
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    if claims.exp <= now {
        return Err(anyhow!("Token expired"));
    }

    Ok(AuthInfo {
        public_key: claims.sub.clone(),
        app_instance_id: claims.app_instance_id.clone(),
    })
}

/// Create a JWT token (for testing purposes)
pub fn create_jwt(
    private_key_bytes: &[u8],
    public_key_hex: &str,
    app_instance_id: &str,
    duration_secs: u64,
) -> Result<String> {
    use ed25519_dalek::{Signer, SigningKey};

    if private_key_bytes.len() != 32 {
        return Err(anyhow!("Invalid private key length"));
    }

    let signing_key = SigningKey::from_bytes(&private_key_bytes.try_into().unwrap());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    let claims = Claims {
        sub: public_key_hex.to_string(),
        app_instance_id: app_instance_id.to_string(),
        exp: now + duration_secs,
        iat: now,
    };

    // Create header
    let header = r#"{"alg":"EdDSA","typ":"JWT"}"#;
    let header_b64 = base64_url_encode(header.as_bytes());

    // Create payload
    let payload = serde_json::to_string(&claims)?;
    let payload_b64 = base64_url_encode(payload.as_bytes());

    // Create message to sign
    let message = format!("{}.{}", header_b64, payload_b64);

    // Sign the message
    let signature = signing_key.sign(message.as_bytes());
    let signature_b64 = base64_url_encode(&signature.to_bytes());

    Ok(format!("{}.{}", message, signature_b64))
}

/// Base64URL encode (without padding)
fn base64_url_encode(data: &[u8]) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        data
    )
}

/// Base64URL decode (without padding)
fn base64_url_decode(data: &str) -> Result<Vec<u8>> {
    Ok(base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        data
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_jwt_create_and_verify() {
        // Generate a keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let private_key_bytes = signing_key.to_bytes();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let app_instance_id = "test_app_123";

        // Create a JWT
        let token = create_jwt(
            &private_key_bytes,
            &public_key_hex,
            app_instance_id,
            3600,  // 1 hour
        ).unwrap();

        // Verify the JWT
        let auth_info = verify_jwt(&token).unwrap();

        assert_eq!(auth_info.public_key, public_key_hex);
        assert_eq!(auth_info.app_instance_id, app_instance_id);
    }

    #[test]
    fn test_expired_token() {
        // Generate a keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let private_key_bytes = signing_key.to_bytes();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let app_instance_id = "test_app_123";

        // Create an expired JWT
        let token = create_jwt(
            &private_key_bytes,
            &public_key_hex,
            app_instance_id,
            0,  // Already expired
        ).unwrap();

        // Verification should fail
        let result = verify_jwt(&token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expired"));
    }
}