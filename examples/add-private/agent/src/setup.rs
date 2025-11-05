//! Setup command - Generate Ed25519 keypair and create app instance

use anyhow::{Context, Result};
use chrono::Duration;
use ed25519_dalek::{SigningKey, VerifyingKey};
use log::info;
use rand::rngs::OsRng;
use state_proto::state_service_client::StateServiceClient;
use state_proto::{CreateAppInstanceRequest, JwtAuth};
use std::env;

use crate::jwt::{create_claims, create_signed_jwt};

pub async fn run() -> Result<()> {
    println!("🔐 Add Private Agent Setup");
    println!();

    // Step 1: Generate Ed25519 keypair
    println!("1. Generating Ed25519 keypair...");
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key: VerifyingKey = signing_key.verifying_key();

    let private_key_hex = hex::encode(signing_key.to_bytes());
    let public_key_hex = hex::encode(verifying_key.to_bytes());
    let app_instance_id = public_key_hex.clone();

    println!("   ✓ Keypair generated");
    println!("   ✓ App Instance ID: {}", app_instance_id);
    println!();

    // Step 2: Display environment variables to add
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Add these to your .env file:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("SILVANA_PRIVATE_KEY={}", private_key_hex);
    println!("SILVANA_PUBLIC_KEY={}", public_key_hex);
    println!("APP_INSTANCE_ID={}", app_instance_id);
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Step 3: Read configuration from environment
    println!("2. Reading configuration...");
    let state_service_url = env::var("STATE_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:50052".to_string());
    let aud = env::var("SILVANA_AUD").unwrap_or_else(|_| "private".to_string());
    let app_name = env::var("APP_NAME").unwrap_or_else(|_| "add_private_app".to_string());
    let app_description = env::var("APP_DESCRIPTION")
        .unwrap_or_else(|_| "Silvana Add Private App".to_string());

    println!("   State Service URL: {}", state_service_url);
    println!("   Audience: {}", aud);
    println!("   App Name: {}", app_name);
    println!();

    // Step 4: Create JWT (1 year expiry)
    println!("3. Creating JWT token...");
    let claims = create_claims(
        &public_key_hex,
        &app_instance_id,
        &aud,
        Duration::days(365), // 1 year
    );

    let jwt = create_signed_jwt(&signing_key, &claims)?;
    println!("   ✓ JWT created (expires in 1 year)");
    println!();

    // Step 5: Connect to State Service
    println!("4. Connecting to State Service...");
    println!("   URL: {}", state_service_url);

    let mut client = StateServiceClient::connect(state_service_url.clone())
        .await
        .context("Failed to connect to State Service")?;

    println!("   ✓ Connected");
    println!();

    // Step 6: Create app instance
    println!("5. Creating app instance...");

    let request = CreateAppInstanceRequest {
        auth: Some(JwtAuth { token: jwt }),
        app_instance_id: app_instance_id.clone(),
        name: app_name.clone(),
        description: Some(app_description),
        metadata: None,
    };

    let response = client
        .create_app_instance(request)
        .await
        .context("Failed to create app instance")?
        .into_inner();

    println!("   ✓ CreateAppInstance Response:");
    println!("     - Success: {}", response.success);
    println!("     - Message: {}", response.message);

    if let Some(app_instance) = &response.app_instance {
        println!("     - App Instance ID: {}", app_instance.app_instance_id);
        println!("     - Owner: {}", app_instance.owner);
        println!("     - Name: {}", app_instance.name);
    }
    println!();

    // Step 7: Success
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Setup Complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Next steps:");
    println!("  1. Add the environment variables above to your .env file");
    println!("  2. Submit a job: cargo run -- add");
    println!("  3. Run agent: cargo run -- agent");
    println!();

    Ok(())
}
