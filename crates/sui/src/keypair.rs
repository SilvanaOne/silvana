use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::Serialize;
use std::str::FromStr;
use sui_sdk_types as sui;

pub struct GeneratedKeypair {
    pub secret_key: [u8; 32],
    pub public_key: [u8; 32],
    pub address: sui::Address,
    pub sui_private_key: String,
}

pub fn generate_ed25519() -> Result<GeneratedKeypair, String> {
    let mut csprng = OsRng;
    let mut sk_bytes = [0u8; 32];
    csprng.fill_bytes(&mut sk_bytes);

    let signing_key = SigningKey::from_bytes(&sk_bytes);
    let verifying_key = signing_key.verifying_key();
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(verifying_key.as_bytes());

    // Compute Sui address using SDK
    let sui_public_key = sui::Ed25519PublicKey::new(pk_bytes);
    let address = sui_public_key.derive_address();

    // suiprivkey bech32 encoding: [flag || 32-byte secret]
    let mut payload = Vec::with_capacity(33);
    payload.push(0x00);
    payload.extend_from_slice(&sk_bytes);
    let sui_private_key = bech32::encode(
        "suiprivkey",
        bech32::ToBase32::to_base32(&payload),
        bech32::Variant::Bech32,
    )
    .map_err(|e| format!("Failed to encode bech32 private key: {}", e))?;

    Ok(GeneratedKeypair {
        secret_key: sk_bytes,
        public_key: pk_bytes,
        address,
        sui_private_key,
    })
}

pub fn sign_message(secret_key: &[u8; 32], message: &[u8]) -> Vec<u8> {
    let signing_key = SigningKey::from_bytes(secret_key);
    let verifying_key = signing_key.verifying_key();
    let sig: Signature = signing_key.sign(message);

    let mut sui_sig = Vec::with_capacity(97);
    sui_sig.push(0x00); // ed25519 flag
    sui_sig.extend_from_slice(&sig.to_bytes());
    sui_sig.extend_from_slice(verifying_key.as_bytes());
    sui_sig
}

pub fn verify_with_address(address: &sui::Address, message: &[u8], sui_sig: &[u8]) -> bool {
    if sui_sig.len() != 97 {
        return false;
    }
    if sui_sig[0] != 0x00 {
        return false;
    }
    let sig_bytes: [u8; 64] = match sui_sig[1..65].try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk_bytes: [u8; 32] = match sui_sig[65..97].try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };

    // Recompute address using SDK
    let sui_public_key = sui::Ed25519PublicKey::new(pk_bytes);
    let derived = sui_public_key.derive_address();
    if &derived != address {
        return false;
    }

    // Verify signature
    let vk = match VerifyingKey::from_bytes(&pk_bytes) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let sig = Signature::from_bytes(&sig_bytes);
    vk.verify(message, &sig).is_ok()
}

pub fn bcs_serialize<T: Serialize>(payload: &T) -> Result<Vec<u8>, bcs::Error> {
    bcs::to_bytes(payload)
}

pub fn parse_sui_private_key(sui_private_key: &str) -> Result<[u8; 32], String> {
    // Decode the private key from bech32
    let (hrp, data, _variant) = bech32::decode(sui_private_key)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;
    
    if hrp != "suiprivkey" {
        return Err("Invalid private key format".to_string());
    }
    
    let key_bytes: Vec<u8> = bech32::FromBase32::from_base32(&data)
        .map_err(|e| format!("Failed to convert private key: {}", e))?;
    
    // The format is [flag || 32-byte secret]
    if key_bytes.len() != 33 || key_bytes[0] != 0x00 {
        return Err("Invalid Ed25519 private key".to_string());
    }
    
    let mut secret_key = [0u8; 32];
    secret_key.copy_from_slice(&key_bytes[1..33]);
    Ok(secret_key)
}

pub fn parse_address(address_str: &str) -> Result<sui::Address, String> {
    sui::Address::from_str(address_str)
        .map_err(|e| format!("Invalid address: {}", e))
}