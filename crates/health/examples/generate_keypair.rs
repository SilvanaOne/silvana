//! Example: Generate Ed25519 keypair for health JWT authentication
//!
//! This example shows how to generate a new Ed25519 keypair that can be used
//! for creating and verifying health metric JWT tokens.
//!
//! Run with:
//! ```bash
//! cargo run -p health --example generate_keypair
//! ```

use health::generate_ed25519_keypair;

fn main() {
    println!();
    println!("🔐 Health Metrics JWT Keypair Generator");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Generating Ed25519 keypair...");
    println!();

    // Generate the keypair
    let keypair = generate_ed25519_keypair();

    println!("✅ Keypair generated successfully!");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔑 PRIVATE KEY (Secret - Keep Secure!)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("{}", keypair.private_key_hex);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔓 PUBLIC KEY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("{}", keypair.public_key_hex);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 Environment Variables");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Add these to your .env file or environment:");
    println!();
    println!("# Private key (for JWT creation - server side)");
    println!("HEALTH_PRIVATE_KEY={}", keypair.private_key_hex);
    println!();
    println!("# Public key (for JWT verification - can be shared)");
    println!("HEALTH_PUBLIC_KEY={}", keypair.public_key_hex);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("⚠️  Security Notes");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("1. Keep the PRIVATE KEY secret - never commit to git!");
    println!("2. The private key is used to CREATE JWT tokens (server side)");
    println!("3. The public key is used to VERIFY JWT tokens (client side)");
    println!("4. Store private key in secure storage (e.g., AWS Secrets Manager)");
    println!("5. Include public key in JWT tokens (sub claim) for verification");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
}
