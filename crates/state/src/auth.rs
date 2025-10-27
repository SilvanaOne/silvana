//! JWT authentication with Ed25519 signatures

use anyhow::{anyhow, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// JWT claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // Ed25519 public key (hex)
    pub exp: u64,  // Expiration timestamp
    pub iat: u64,  // Issued at timestamp
    pub aud: String,  // Audience (from SILVANA_AUD)
    pub scope: String,  // Space-separated permissions: "app_instance <id> object <id> ..."
}

/// Verification result containing authenticated information
#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub public_key: String,  // Ed25519 public key (hex)
    pub scope_permissions: Vec<ScopePermission>,
}

/// Represents a single permission in the scope
#[derive(Debug, Clone, PartialEq)]
pub enum ScopePermission {
    AppInstance(String),  // app_instance <id>
    Object(String),       // object <id>
}

/// Parse scope string into permissions
/// Format: "app_instance <id> object <id> app_instance <id2> ..."
fn parse_scope(scope: &str) -> Result<Vec<ScopePermission>> {
    let mut permissions = Vec::new();
    let parts: Vec<&str> = scope.split_whitespace().collect();

    if parts.is_empty() {
        return Err(anyhow!("Invalid scope: empty scope string (expected format: 'app_instance <id>' or 'object <id>')"));
    }

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "app_instance" => {
                if i + 1 >= parts.len() {
                    return Err(anyhow!(
                        "Invalid scope: 'app_instance' missing id at position {}. Full scope: '{}'",
                        i, scope
                    ));
                }
                permissions.push(ScopePermission::AppInstance(parts[i + 1].to_string()));
                i += 2;
            }
            "object" => {
                if i + 1 >= parts.len() {
                    return Err(anyhow!(
                        "Invalid scope: 'object' missing id at position {}. Full scope: '{}'",
                        i, scope
                    ));
                }
                permissions.push(ScopePermission::Object(parts[i + 1].to_string()));
                i += 2;
            }
            _ => {
                return Err(anyhow!(
                    "Invalid scope: unknown permission type '{}' at position {}. Expected 'app_instance' or 'object'. Full scope: '{}'",
                    parts[i], i, scope
                ));
            }
        }
    }

    if permissions.is_empty() {
        return Err(anyhow!("Invalid scope: no valid permissions found. Full scope: '{}'", scope));
    }

    Ok(permissions)
}

impl AuthInfo {
    /// Check if scope contains permission for specific app instance
    pub fn has_app_instance_permission(&self, app_instance_id: &str) -> bool {
        self.scope_permissions.iter().any(|p| {
            matches!(p, ScopePermission::AppInstance(id) if id == app_instance_id)
        })
    }

    /// Check if scope contains permission for specific object
    pub fn has_object_permission(&self, object_id: &str) -> bool {
        self.scope_permissions.iter().any(|p| {
            matches!(p, ScopePermission::Object(id) if id == object_id)
        })
    }

    /// Get the first app_instance from scope (for operations that need an app_instance context)
    pub fn get_primary_app_instance(&self) -> Option<String> {
        self.scope_permissions.iter().find_map(|p| {
            match p {
                ScopePermission::AppInstance(id) => Some(id.clone()),
                _ => None,
            }
        })
    }
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
    validation.validate_aud = false;  // We'll validate audience manually with better error messages
    validation.validate_exp = false;  // We'll validate expiration manually with better error messages

    let token_data = decode::<Claims>(token, &DecodingKey::from_secret(&[]), &validation)
        .map_err(|e| anyhow!("Failed to decode JWT claims: {}", e))?;
    let claims = &token_data.claims;

    // Parse the public key from hex
    let public_key_bytes = hex::decode(&claims.sub)
        .map_err(|e| anyhow!("Failed to decode public key from hex (sub={}): {}", claims.sub, e))?;
    if public_key_bytes.len() != 32 {
        return Err(anyhow!(
            "Invalid public key length: expected 32 bytes, got {} bytes (sub={})",
            public_key_bytes.len(),
            claims.sub
        ));
    }

    let public_key = VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap())
        .map_err(|e| anyhow!("Failed to create verifying key from public key (sub={}): {}", claims.sub, e))?;

    // Create the message to verify (header.payload)
    let message = format!("{}.{}", parts[0], parts[1]);

    // Decode the signature from base64url
    let signature_bytes = base64_url_decode(parts[2])
        .map_err(|e| anyhow!("Failed to decode signature from base64url: {}", e))?;
    if signature_bytes.len() != 64 {
        return Err(anyhow!(
            "Invalid signature length: expected 64 bytes, got {} bytes",
            signature_bytes.len()
        ));
    }

    let signature = Signature::from_bytes(&signature_bytes.try_into().unwrap());

    // Verify the signature
    public_key.verify(message.as_bytes(), &signature)
        .map_err(|_| anyhow!(
            "Invalid signature: JWT signature verification failed for public key {}",
            claims.sub
        ))?;

    // Check expiration
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();

    if claims.exp <= now {
        return Err(anyhow!(
            "Token expired: exp={} ({}), now={} ({}), expired {} seconds ago",
            claims.exp,
            chrono::DateTime::from_timestamp(claims.exp as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "invalid".to_string()),
            now,
            chrono::DateTime::from_timestamp(now as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "invalid".to_string()),
            now - claims.exp
        ));
    }

    // Validate audience
    let expected_aud = std::env::var("SILVANA_AUD")
        .map_err(|_| anyhow!("SILVANA_AUD environment variable not set"))?;

    if claims.aud != expected_aud {
        return Err(anyhow!(
            "Invalid audience: expected '{}', got '{}'",
            expected_aud,
            claims.aud
        ));
    }

    // Parse and validate scope
    let scope_permissions = parse_scope(&claims.scope)?;

    Ok(AuthInfo {
        public_key: claims.sub.clone(),
        scope_permissions,
    })
}

/// Create a JWT token (for testing purposes)
pub fn create_jwt(
    private_key_bytes: &[u8],
    public_key_hex: &str,
    duration_secs: u64,
    aud: &str,
    scope: &str,
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
        exp: now + duration_secs,
        iat: now,
        aud: aud.to_string(),
        scope: scope.to_string(),
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
        // Set required environment variable
        std::env::set_var("SILVANA_AUD", "test-service");

        // Generate a keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let private_key_bytes = signing_key.to_bytes();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let app_instance_id = "test_app_123";
        let scope = format!("app_instance {}", app_instance_id);

        // Create a JWT
        let token = create_jwt(
            &private_key_bytes,
            &public_key_hex,
            3600,  // 1 hour
            "test-service",
            &scope,
        ).unwrap();

        // Verify the JWT
        let auth_info = verify_jwt(&token).unwrap();

        assert_eq!(auth_info.public_key, public_key_hex);
        assert!(auth_info.has_app_instance_permission(app_instance_id));
        assert_eq!(auth_info.get_primary_app_instance().unwrap(), app_instance_id);
    }

    #[test]
    fn test_expired_token() {
        // Set required environment variable
        std::env::set_var("SILVANA_AUD", "test-service");

        // Generate a keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let private_key_bytes = signing_key.to_bytes();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let app_instance_id = "test_app_123";
        let scope = format!("app_instance {}", app_instance_id);

        // Create an expired JWT
        let token = create_jwt(
            &private_key_bytes,
            &public_key_hex,
            0,  // Already expired
            "test-service",
            &scope,
        ).unwrap();

        // Verification should fail
        let result = verify_jwt(&token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expired"));
    }

    #[test]
    fn test_scope_parsing() {
        let scope = "app_instance app1 object obj1 app_instance app2 object obj2";
        let permissions = parse_scope(scope).unwrap();

        assert_eq!(permissions.len(), 4);
        assert_eq!(permissions[0], ScopePermission::AppInstance("app1".to_string()));
        assert_eq!(permissions[1], ScopePermission::Object("obj1".to_string()));
        assert_eq!(permissions[2], ScopePermission::AppInstance("app2".to_string()));
        assert_eq!(permissions[3], ScopePermission::Object("obj2".to_string()));
    }

    #[test]
    fn test_invalid_audience() {
        // Set required environment variable
        std::env::set_var("SILVANA_AUD", "test-service");

        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let private_key_bytes = signing_key.to_bytes();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let app_instance_id = "test_app_123";
        let scope = format!("app_instance {}", app_instance_id);

        // Create a JWT with wrong audience
        let token = create_jwt(
            &private_key_bytes,
            &public_key_hex,
            3600,
            "wrong-audience",
            &scope,
        ).unwrap();

        // Verification should fail
        let result = verify_jwt(&token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid audience"));
    }
}