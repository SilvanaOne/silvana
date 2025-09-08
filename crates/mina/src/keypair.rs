use anyhow::Result;
use mina_signer::{Keypair, SecKey};
use rand::rngs::OsRng;

pub struct GeneratedKeypair {
    pub private_key: String,  // Base58-encoded private key
    pub public_key: String,   // Mina address (Base58Check)
}

/// Generate a new Mina keypair
pub fn generate_mina_keypair() -> Result<GeneratedKeypair> {
    // Generate a random keypair using mina-signer
    let keypair = Keypair::rand(&mut OsRng)
        .map_err(|e| anyhow::anyhow!("Failed to generate random keypair: {:?}", e))?;
    
    // Get the private key in base58 format
    let private_key = keypair.secret.to_base58();
    
    // Get the public key address (Mina address)
    let public_key = keypair.public.into_address();
    
    Ok(GeneratedKeypair {
        private_key,
        public_key,
    })
}

/// Create a keypair from an existing private key
pub fn keypair_from_private_key(private_key_b58: &str) -> Result<Keypair> {
    let sec_key = SecKey::from_base58(private_key_b58)
        .map_err(|e| anyhow::anyhow!("Invalid base58 private key: {:?}", e))?;
    
    Keypair::from_secret_key(sec_key)
        .map_err(|e| anyhow::anyhow!("Failed to create keypair: {:?}", e))
}

/// Verify that a private key matches a public address
pub fn verify_keypair(private_key_b58: &str, expected_address: &str) -> Result<bool> {
    let keypair = keypair_from_private_key(private_key_b58)?;
    let address = keypair.public.into_address();
    Ok(address == expected_address)
}