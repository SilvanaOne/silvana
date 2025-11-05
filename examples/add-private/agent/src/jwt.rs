//! JWT utilities for Ed25519 signing

use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,      // Subject (public key)
    pub aud: String,      // Audience
    pub scope: String,    // Scope
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
}

/// Create and sign a JWT using Ed25519 private key
pub fn create_signed_jwt(signing_key: &SigningKey, claims: &Claims) -> Result<String> {
    // Create header
    let header = serde_json::json!({
        "alg": "EdDSA",
        "typ": "JWT"
    });

    // Encode header and claims
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);

    // Create the message to sign
    let message = format!("{}.{}", header_b64, claims_b64);

    // Sign the message
    let signature: Signature = signing_key.sign(message.as_bytes());

    // Encode the signature
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    // Combine to create the JWT
    let jwt = format!("{}.{}", message, signature_b64);

    Ok(jwt)
}

/// Create JWT claims for app instance with given expiry duration
pub fn create_claims(
    public_key_hex: &str,
    app_instance_id: &str,
    aud: &str,
    expiry: Duration,
) -> Claims {
    let now = Utc::now();
    let exp_time = now + expiry;

    Claims {
        sub: public_key_hex.to_string(),
        aud: aud.to_string(),
        scope: format!("app_instance {}", app_instance_id),
        exp: exp_time.timestamp(),
        iat: now.timestamp(),
    }
}
