use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedKeypair {
    pub private_key: String,
    pub address: String,
}

/// Generate a new Ethereum keypair
pub fn generate_ethereum_keypair() -> Result<GeneratedKeypair> {
    // Generate a new random signer
    let signer = PrivateKeySigner::random();
    
    // Get the private key as hex string (with 0x prefix)
    let private_key = format!("0x{}", hex::encode(signer.to_bytes()));
    
    // Get the address
    let address = signer.address();
    
    Ok(GeneratedKeypair {
        private_key,
        address: format!("{:?}", address), // This formats with 0x prefix and checksum
    })
}

/// Create a keypair from an existing private key
pub fn keypair_from_private_key(private_key: &str) -> Result<GeneratedKeypair> {
    // Remove 0x prefix if present
    let key_hex = if private_key.starts_with("0x") || private_key.starts_with("0X") {
        &private_key[2..]
    } else {
        private_key
    };
    
    // Parse the private key
    let key_bytes = hex::decode(key_hex)
        .map_err(|e| anyhow!("Invalid hex private key: {}", e))?;
    
    if key_bytes.len() != 32 {
        return Err(anyhow!("Private key must be 32 bytes, got {}", key_bytes.len()));
    }
    
    // Create signer from private key
    let key_array: [u8; 32] = key_bytes.try_into()
        .map_err(|_| anyhow!("Private key must be exactly 32 bytes"))?;
    let signer = PrivateKeySigner::from_bytes(&key_array.into())
        .map_err(|e| anyhow!("Failed to create signer from private key: {}", e))?;
    
    // Get the address
    let address = signer.address();
    
    Ok(GeneratedKeypair {
        private_key: format!("0x{}", key_hex),
        address: format!("{:?}", address),
    })
}

/// Verify a keypair by checking the private key derives to the expected address
pub fn verify_keypair(private_key: &str, expected_address: &str) -> Result<bool> {
    let keypair = keypair_from_private_key(private_key)?;
    
    // Normalize addresses for comparison (both with 0x prefix, lowercase)
    let derived = keypair.address.to_lowercase();
    let expected = if expected_address.starts_with("0x") || expected_address.starts_with("0X") {
        expected_address.to_lowercase()
    } else {
        format!("0x{}", expected_address).to_lowercase()
    };
    
    Ok(derived == expected)
}

/// Parse an Ethereum address from a string
pub fn parse_address(address: &str) -> Result<Address> {
    address.parse::<Address>()
        .map_err(|e| anyhow!("Invalid Ethereum address: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keypair() {
        let keypair = generate_ethereum_keypair().unwrap();
        
        // Check private key format (should be 0x + 64 hex chars)
        assert!(keypair.private_key.starts_with("0x"));
        assert_eq!(keypair.private_key.len(), 66); // 0x + 64 hex chars
        
        // Check address format (should be 0x + 40 hex chars)
        assert!(keypair.address.starts_with("0x"));
        assert_eq!(keypair.address.len(), 42); // 0x + 40 hex chars
        
        println!("Generated keypair:");
        println!("  Private key: {}", keypair.private_key);
        println!("  Address: {}", keypair.address);
    }
    
    #[test]
    fn test_keypair_from_private_key() {
        // Test with a known private key
        let private_key = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let keypair = keypair_from_private_key(private_key).unwrap();
        
        // Should preserve the private key
        assert_eq!(keypair.private_key, private_key);
        
        // Should derive a valid address
        assert!(keypair.address.starts_with("0x"));
        assert_eq!(keypair.address.len(), 42);
    }
    
    #[test]
    fn test_keypair_from_private_key_without_prefix() {
        // Test without 0x prefix
        let private_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let keypair = keypair_from_private_key(private_key).unwrap();
        
        // Should add 0x prefix
        assert_eq!(keypair.private_key, format!("0x{}", private_key));
    }
    
    #[test]
    fn test_verify_keypair() {
        let keypair = generate_ethereum_keypair().unwrap();
        
        // Should verify correctly
        assert!(verify_keypair(&keypair.private_key, &keypair.address).unwrap());
        
        // Should fail with wrong address
        assert!(!verify_keypair(&keypair.private_key, "0x0000000000000000000000000000000000000000").unwrap());
    }
    
    #[test]
    fn test_deterministic_keypair() {
        // Same private key should always produce same address
        let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        
        let keypair1 = keypair_from_private_key(private_key).unwrap();
        let keypair2 = keypair_from_private_key(private_key).unwrap();
        
        assert_eq!(keypair1.address, keypair2.address);
    }
}