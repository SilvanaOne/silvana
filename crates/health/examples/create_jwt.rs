//! Example: Create a health JWT token
//!
//! This example shows how to create a JWT token for health metrics authentication.
//! The token includes the endpoint URL, node ID, and public key for verification.
//!
//! Run with:
//! ```bash
//! cargo run -p health --example create_jwt
//! ```

use health::{create_health_jwt, generate_ed25519_keypair};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("🔐 Health JWT Token Creator");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Step 1: Generate a new keypair (or use existing keys)
    println!("Step 1: Generating Ed25519 keypair...");
    let keypair = generate_ed25519_keypair();
    println!("✅ Keypair generated");
    println!();

    // Step 2: Set JWT parameters
    let endpoint_url = "https://api.example.com/health";
    let node_id = "node-001";
    let expiration_hours = 24 * 30; // 30 days

    println!("Step 2: JWT Configuration");
    println!("  Endpoint URL: {}", endpoint_url);
    println!("  Node ID: {}", node_id);
    println!("  Expiration: {} hours ({} days)", expiration_hours, expiration_hours / 24);
    println!();

    // Step 3: Calculate expiration timestamp
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs();
    let exp = now + (expiration_hours as u64 * 3600);

    println!("Step 3: Creating JWT token...");

    // Step 4: Decode private key from hex
    let private_key_bytes = hex::decode(&keypair.private_key_hex)?;
    let private_key_array: [u8; 32] = private_key_bytes
        .try_into()
        .map_err(|_| "Invalid private key length")?;

    // Step 5: Create the JWT
    let jwt = create_health_jwt(
        &private_key_array,
        &keypair.public_key_hex,
        endpoint_url,
        node_id,
        exp,
    )?;

    println!("✅ JWT token created successfully!");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🎫 JWT TOKEN");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("{}", jwt);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 Token Details");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Endpoint URL: {}", endpoint_url);
    println!("Node ID: {}", node_id);
    println!("Public Key: {}", keypair.public_key_hex);
    println!("Issued At: {}", chrono::DateTime::from_timestamp(now as i64, 0).unwrap());
    println!("Expires At: {}", chrono::DateTime::from_timestamp(exp as i64, 0).unwrap());
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💡 Usage");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Set this token as the JWT_HEALTH environment variable:");
    println!();
    println!("export JWT_HEALTH=\"{}\"", jwt);
    println!();
    println!("Then run the health exporter:");
    println!();
    println!("cargo run -p health --features binary");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    Ok(())
}
