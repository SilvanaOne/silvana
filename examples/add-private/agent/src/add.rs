//! Add command - Submit add job to Private layer

use anyhow::{Context, Result};
use chrono::Duration;
use ed25519_dalek::SigningKey;
use log::info;
use state_proto::state_service_client::StateServiceClient;
use state_proto::{JwtAuth, SubmitUserActionRequest};
use std::env;

use crate::jwt::{create_claims, create_signed_jwt};

pub async fn run() -> Result<()> {
    // Load .env file if exists
    let _ = dotenvy::dotenv();

    println!("➕ Submitting Add Job to Private Layer");
    println!();

    // Step 1: Read environment variables
    println!("1. Reading configuration from .env...");
    let private_key_hex = env::var("SILVANA_PRIVATE_KEY")
        .context("SILVANA_PRIVATE_KEY not set in .env. Run 'cargo run -- setup' first.")?;
    let public_key_hex = env::var("SILVANA_PUBLIC_KEY")
        .context("SILVANA_PUBLIC_KEY not set in .env")?;
    let app_instance_id =
        env::var("APP_INSTANCE_ID").context("APP_INSTANCE_ID not set in .env")?;
    let state_service_url = env::var("STATE_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:50052".to_string());
    let aud = env::var("SILVANA_AUD").unwrap_or_else(|_| "private".to_string());

    println!("   App Instance ID: {}", app_instance_id);
    println!("   State Service URL: {}", state_service_url);
    println!();

    // Step 2: Load signing key
    println!("2. Loading signing key...");
    let private_key_bytes = hex::decode(private_key_hex)
        .context("Failed to decode SILVANA_PRIVATE_KEY from hex")?;
    let signing_key = SigningKey::from_bytes(
        &private_key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid private key length"))?,
    );
    println!("   ✓ Signing key loaded");
    println!();

    // Step 3: Create JWT (24 hours)
    println!("3. Creating JWT token (24 hour expiry)...");
    let claims = create_claims(
        &public_key_hex,
        &app_instance_id,
        &aud,
        Duration::hours(24),
    );

    let jwt = create_signed_jwt(&signing_key, &claims)?;
    println!("   ✓ JWT created");
    println!();

    // Step 4: Connect to State Service
    println!("4. Connecting to State Service...");
    let mut client = StateServiceClient::connect(state_service_url.clone())
        .await
        .context("Failed to connect to State Service")?;
    println!("   ✓ Connected");
    println!();

    // Step 5: Submit user action
    println!("5. Submitting add job...");

    // Simple action data: just two bytes [1, 1] representing add(1, 1)
    let action_data = vec![1, 1];

    let request = SubmitUserActionRequest {
        auth: Some(JwtAuth { token: jwt }),
        app_instance_id: app_instance_id.clone(),
        action_type: "add".to_string(),
        action_data: action_data.clone(),
        action_da: None,
        metadata: None,
    };

    let response = client
        .submit_user_action(request)
        .await
        .context("Failed to submit user action")?
        .into_inner();

    println!("   ✓ SubmitUserAction Response:");
    println!("     - Success: {}", response.success);
    println!("     - Message: {}", response.message);
    println!("     - Action Sequence: {}", response.action_sequence);
    println!();

    // Step 6: Success
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Add Job Submitted!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Sequence: {}", response.action_sequence);
    println!();
    println!("The job will be processed by agents running:");
    println!("  cargo run -- agent");
    println!();

    Ok(())
}
