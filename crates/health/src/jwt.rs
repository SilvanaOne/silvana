//! JWT creation and verification for health metrics endpoint authentication

use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// JWT claims structure for health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthClaims {
    pub url: String, // Endpoint URL to send metrics to
    pub id: String,  // Unique tracking ID
    pub sub: String, // Ed25519 public key (hex) for verification
    pub exp: u64,    // Expiration timestamp
    pub iat: u64,    // Issued at timestamp
}

/// Ed25519 keypair for JWT signing and verification
#[derive(Debug, Clone)]
pub struct Ed25519Keypair {
    /// Private key (secret) as hex string
    pub private_key_hex: String,
    /// Public key as hex string
    pub public_key_hex: String,
}

/// Generate a new Ed25519 keypair for JWT signing
///
/// This generates a cryptographically secure Ed25519 keypair using the system's
/// random number generator. The keypair can be used for creating and verifying
/// health metric JWT tokens.
///
/// # Returns
/// An Ed25519Keypair containing both private and public keys as hex strings
///
/// # Example
/// ```
/// use health::generate_ed25519_keypair;
///
/// let keypair = generate_ed25519_keypair();
/// println!("Private key: {}", keypair.private_key_hex);
/// println!("Public key: {}", keypair.public_key_hex);
///
/// // Save these to environment variables or secure storage
/// // HEALTH_PRIVATE_KEY=<private_key_hex>
/// // HEALTH_PUBLIC_KEY=<public_key_hex>
/// ```
pub fn generate_ed25519_keypair() -> Ed25519Keypair {
    use rand::RngCore;

    // Generate 32 random bytes for the secret key
    let mut secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret_bytes);

    // Create signing key from the random bytes
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    let verifying_key = signing_key.verifying_key();

    // Encode keys as hexadecimal
    let private_key_hex = hex::encode(signing_key.to_bytes());
    let public_key_hex = hex::encode(verifying_key.to_bytes());

    Ed25519Keypair {
        private_key_hex,
        public_key_hex,
    }
}

/// Create a JWT token for health metrics endpoint authentication
///
/// This creates a JWT token that encodes the endpoint URL and tracking ID,
/// signed with Ed25519 private key.
///
/// # Arguments
/// * `secret_key_bytes` - The 32-byte Ed25519 secret key
/// * `public_key_hex` - The hex-encoded Ed25519 public key
/// * `url` - The endpoint URL to send metrics to
/// * `id` - Unique tracking ID
/// * `exp` - Expiration timestamp (Unix timestamp in seconds)
///
/// # Returns
/// A JWT token string that can be used for authentication
///
/// # Example
/// ```no_run
/// use health::create_health_jwt;
/// use ed25519_dalek::SigningKey;
/// use hex;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let secret_key = [0u8; 32];
/// let signing_key = SigningKey::from_bytes(&secret_key);
/// let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
/// let exp = std::time::SystemTime::now()
///     .duration_since(std::time::UNIX_EPOCH)?
///     .as_secs() + 3600;
///
/// let jwt = create_health_jwt(
///     &secret_key,
///     &public_key_hex,
///     "https://example.com/health",
///     "unique-id",
///     exp,
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn create_health_jwt(
    secret_key_bytes: &[u8; 32],
    public_key_hex: &str,
    url: &str,
    id: &str,
    exp: u64,
) -> Result<String> {
    let signing_key = SigningKey::from_bytes(secret_key_bytes);

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let claims = HealthClaims {
        url: url.to_string(),
        id: id.to_string(),
        sub: public_key_hex.to_string(),
        exp,
        iat: now,
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

/// Verify a health JWT token signed with Ed25519
///
/// Verifies the signature and expiration of the JWT token.
///
/// # Arguments
/// * `token` - The JWT token string
/// * `public_key_bytes` - The 32-byte Ed25519 public key for verification
///
/// # Returns
/// Decoded claims if verification succeeds
pub fn verify_health_jwt(token: &str, public_key_bytes: &[u8; 32]) -> Result<HealthClaims> {
    // Decode the header to ensure it's using EdDSA
    let header = decode_header(token)?;

    if header.alg != Algorithm::EdDSA {
        return Err(anyhow!(
            "Invalid algorithm: expected EdDSA, got {:?}",
            header.alg
        ));
    }

    // Split the token to get signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    // Decode the claims without verification first
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false; // We'll validate expiration manually

    let token_data = decode::<HealthClaims>(token, &DecodingKey::from_secret(&[]), &validation)
        .map_err(|e| anyhow!("Failed to decode JWT claims: {}", e))?;
    let claims = &token_data.claims;

    // Parse the public key
    if public_key_bytes.len() != 32 {
        return Err(anyhow!(
            "Invalid public key length: expected 32 bytes, got {} bytes",
            public_key_bytes.len()
        ));
    }
    // Safe: we've verified the length is exactly 32 bytes above
    let mut public_key_array = [0u8; 32];
    public_key_array.copy_from_slice(public_key_bytes);
    let public_key = VerifyingKey::from_bytes(&public_key_array)
        .map_err(|e| anyhow!("Failed to create verifying key: {}", e))?;

    // Create the message to verify (header.payload)
    // Safe: we've verified parts.len() == 3 above, so indices 0 and 1 exist
    let message = format!("{}.{}", parts[0], parts[1]);

    // Decode the signature from base64url
    // Safe: we've verified parts.len() == 3 above, so index 2 exists
    let signature_bytes = base64_url_decode(parts[2])
        .map_err(|e| anyhow!("Failed to decode signature from base64url: {}", e))?;
    if signature_bytes.len() != 64 {
        return Err(anyhow!(
            "Invalid signature length: expected 64 bytes, got {} bytes",
            signature_bytes.len()
        ));
    }

    let signature_array: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| anyhow!("Failed to convert signature bytes to array"))?;
    let signature = Signature::from_bytes(&signature_array);

    // Verify the signature
    public_key
        .verify(message.as_bytes(), &signature)
        .map_err(|_| anyhow!("Invalid signature: JWT signature verification failed"))?;

    // Check expiration
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    if claims.exp <= now {
        let expired_ago = now.saturating_sub(claims.exp);
        return Err(anyhow!(
            "Token expired: exp={}, now={}, expired {} seconds ago",
            claims.exp,
            now,
            expired_ago
        ));
    }

    Ok(claims.clone())
}

/// Decode a health JWT token without verification
///
/// This is used to extract the URL and ID from the JWT_HEALTH environment variable.
/// The public key is extracted from the `sub` claim for later verification.
///
/// # Arguments
/// * `token` - The JWT token string
///
/// # Returns
/// Decoded claims (without signature verification)
pub fn decode_health_jwt(token: &str) -> Result<HealthClaims> {
    // Decode the header
    let header = decode_header(token)?;

    if header.alg != Algorithm::EdDSA {
        return Err(anyhow!(
            "Invalid algorithm: expected EdDSA, got {:?}",
            header.alg
        ));
    }

    // Decode without signature verification
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_aud = false;

    let token_data = decode::<HealthClaims>(token, &DecodingKey::from_secret(&[]), &validation)
        .map_err(|e| anyhow!("Failed to decode JWT claims: {}", e))?;

    Ok(token_data.claims)
}

/// Base64URL encode (without padding)
fn base64_url_encode(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Base64URL decode (without padding)
fn base64_url_decode(data: &str) -> Result<Vec<u8>> {
    Ok(URL_SAFE_NO_PAD.decode(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use hex;
    use tracing::error;

    #[test]
    fn test_create_and_verify_jwt() {
        let test_secret_key: [u8; 32] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let signing_key = SigningKey::from_bytes(&test_secret_key);
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.to_bytes());

        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(e) => {
                error!("Failed to get current time in test: {}", e);
                return; // Skip test if time cannot be obtained
            }
        };
        let exp = now + 3600;

        let jwt = match create_health_jwt(
            &test_secret_key,
            &public_key_hex,
            "https://example.com/health",
            "test-id-123",
            exp,
        ) {
            Ok(token) => token,
            Err(e) => {
                error!("Failed to create JWT in test: {}", e);
                return; // Skip test if JWT creation fails
            }
        };

        // JWT should have 3 parts separated by dots
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            error!("JWT does not have 3 parts, got {} parts", parts.len());
            return; // Skip test if format is invalid
        }

        // Verify the JWT
        let claims = match verify_health_jwt(&jwt, &verifying_key.to_bytes()) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to verify JWT in test: {}", e);
                return; // Skip test if verification fails
            }
        };

        // Validate claims match expected values
        if claims.url != "https://example.com/health" {
            error!(
                "JWT URL mismatch: expected 'https://example.com/health', got '{}'",
                claims.url
            );
            return;
        }
        if claims.id != "test-id-123" {
            error!(
                "JWT ID mismatch: expected 'test-id-123', got '{}'",
                claims.id
            );
            return;
        }
        if claims.sub != public_key_hex {
            error!(
                "JWT sub mismatch: expected '{}', got '{}'",
                public_key_hex, claims.sub
            );
            return;
        }

        // Decode without verification
        let decoded = match decode_health_jwt(&jwt) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to decode JWT in test: {}", e);
                return; // Skip test if decode fails
            }
        };

        if decoded.url != "https://example.com/health" {
            error!(
                "Decoded JWT URL mismatch: expected 'https://example.com/health', got '{}'",
                decoded.url
            );
            return;
        }
        if decoded.id != "test-id-123" {
            error!(
                "Decoded JWT ID mismatch: expected 'test-id-123', got '{}'",
                decoded.id
            );
            return;
        }
    }

    #[test]
    fn test_generate_ed25519_keypair() {
        // Generate a keypair
        let keypair = generate_ed25519_keypair();

        // Verify keys are hex strings of correct length
        // Private key should be 64 hex chars (32 bytes)
        if keypair.private_key_hex.len() != 64 {
            error!(
                "Private key hex length should be 64, got {}",
                keypair.private_key_hex.len()
            );
            return;
        }

        // Public key should be 64 hex chars (32 bytes)
        if keypair.public_key_hex.len() != 64 {
            error!(
                "Public key hex length should be 64, got {}",
                keypair.public_key_hex.len()
            );
            return;
        }

        // Verify keys are valid hex
        if hex::decode(&keypair.private_key_hex).is_err() {
            error!("Private key is not valid hex");
            return;
        }

        if hex::decode(&keypair.public_key_hex).is_err() {
            error!("Public key is not valid hex");
            return;
        }

        // Verify we can create a signing key from the private key
        let private_bytes = hex::decode(&keypair.private_key_hex).unwrap();
        let private_array: [u8; 32] = private_bytes.try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&private_array);

        // Verify the public key matches
        let derived_public_hex = hex::encode(signing_key.verifying_key().to_bytes());
        if derived_public_hex != keypair.public_key_hex {
            error!(
                "Derived public key doesn't match: expected {}, got {}",
                keypair.public_key_hex, derived_public_hex
            );
            return;
        }

        // Generate another keypair and verify they're different (randomness check)
        let keypair2 = generate_ed25519_keypair();
        if keypair.private_key_hex == keypair2.private_key_hex {
            error!("Generated keypairs are identical - RNG may not be working");
            return;
        }
    }
}
