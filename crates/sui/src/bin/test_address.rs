use ed25519_dalek::SigningKey;
use sui_sdk_types as sui;
use tracing::info;

fn main() {
    tracing_subscriber::fmt::init();
    
    // The secret key bytes we extracted
    let secret_key_hex = "e2773acf074e1ab9be6a3ae8c9810b54f91f3e6169d7edc6a8dc5026c10c48bb";
    let secret_key_bytes = hex::decode(secret_key_hex).unwrap();
    let mut sk_array = [0u8; 32];
    sk_array.copy_from_slice(&secret_key_bytes);
    
    // Use ed25519_dalek to get the public key (like the reference implementation)
    let signing_key = SigningKey::from_bytes(&sk_array);
    let verifying_key = signing_key.verifying_key();
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(verifying_key.as_bytes());
    
    info!("Secret key: {}", hex::encode(&sk_array));
    info!("Public key (ed25519_dalek): {}", hex::encode(&pk_bytes));
    
    // Use SDK's Ed25519PublicKey to derive address - exact reference algorithm
    let sui_public_key = sui::Ed25519PublicKey::new(pk_bytes);
    let address = sui_public_key.derive_address();
    
    info!("Address (SDK): {}", address);
    
    // Expected: 0x8af5716ef88bd3f050b5f349b6eac298dca1c41da2d0e304353c0bc08a360efc
}