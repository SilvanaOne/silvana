use crate::cli::KeypairCommands;
use crate::error::Result;
use anyhow::anyhow;
use tracing_subscriber::prelude::*;

pub async fn handle_keypair_command(subcommand: KeypairCommands) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match subcommand {
        KeypairCommands::Sui => {
            println!("🔑 Generating new Sui Ed25519 keypair...\n");

            match sui::keypair::generate_ed25519() {
                Ok(keypair) => {
                    println!("✅ Sui Keypair Generated Successfully!\n");
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!("🔐 PRIVATE KEY (Keep this secret!):");
                    println!("   {}", keypair.sui_private_key);
                    println!();
                    println!("📍 ADDRESS:");
                    println!("   {}", keypair.address);
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!();
                    println!("⚠️  IMPORTANT:");
                    println!("   • Save your private key in a secure location");
                    println!("   • Never share your private key with anyone");
                    println!("   • You will need this key to sign transactions");
                    println!();
                    println!("💡 To use this keypair:");
                    println!("   export SUI_SECRET_KEY={}", keypair.sui_private_key);
                    println!("   export SUI_ADDRESS={}", keypair.address);
                }
                Err(e) => {
                    eprintln!("❌ Failed to generate keypair: {}", e);
                    return Err(anyhow!("Keypair generation failed: {}", e).into());
                }
            }
        }

        KeypairCommands::Mina => {
            println!("🔑 Generating new Mina keypair...");
            println!();

            match mina::generate_mina_keypair() {
                Ok(keypair) => {
                    println!("✅ Mina Keypair Generated Successfully!");
                    println!();
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!("🔐 PRIVATE KEY (Keep this secret!):");
                    println!("   {}", keypair.private_key);
                    println!();
                    println!("📍 PUBLIC KEY (Address):");
                    println!("   {}", keypair.public_key);
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!();
                    println!("⚠️  IMPORTANT:");
                    println!("   • Save your private key in a secure location");
                    println!("   • Never share your private key with anyone");
                    println!("   • You will need this key to sign transactions");
                    println!();
                    println!("💡 To use this keypair:");
                    println!("   export MINA_PRIVATE_KEY={}", keypair.private_key);
                    println!("   export MINA_PUBLIC_KEY={}", keypair.public_key);
                }
                Err(e) => {
                    eprintln!("❌ Failed to generate Mina keypair: {}", e);
                    return Err(anyhow!("Mina keypair generation failed: {}", e).into());
                }
            }
        }

        KeypairCommands::Ethereum => {
            println!("🔑 Generating new Ethereum keypair...");
            println!();

            match ethereum::generate_ethereum_keypair() {
                Ok(keypair) => {
                    println!("✅ Ethereum Keypair Generated Successfully!");
                    println!();
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!("🔐 PRIVATE KEY (Keep this secret!):");
                    println!("   {}", keypair.private_key);
                    println!();
                    println!("📍 ADDRESS:");
                    println!("   {}", keypair.address);
                    println!(
                        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
                    );
                    println!();
                    println!("⚠️  IMPORTANT:");
                    println!("   • Save your private key in a secure location");
                    println!("   • Never share your private key with anyone");
                    println!("   • You will need this key to sign transactions");
                    println!();
                    println!("💡 To use this keypair:");
                    println!("   export ETH_PRIVATE_KEY={}", keypair.private_key);
                    println!("   export ETH_ADDRESS={}", keypair.address);
                }
                Err(e) => {
                    eprintln!("❌ Failed to generate Ethereum keypair: {}", e);
                    return Err(anyhow!("Ethereum keypair generation failed: {}", e).into());
                }
            }
        }
    }

    Ok(())
}