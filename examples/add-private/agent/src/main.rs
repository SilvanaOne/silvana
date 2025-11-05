//! Add Private Agent
//!
//! CLI tool and agent for Private coordination layer.
//!
//! Commands:
//! - setup: Generate Ed25519 keypair and create app instance
//! - add: Submit add job to Private layer
//! - agent: Run as agent to process jobs

mod setup;
mod add;
mod agent;
mod jwt;

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "add-private-agent")]
#[command(about = "Add Private Agent - CLI and agent for Private coordination layer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate Ed25519 keypair and create app instance on Private layer
    Setup,

    /// Submit an add job to the Private layer
    Add,

    /// Run as agent to process jobs from coordinator
    Agent,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Setup => setup::run().await?,
        Command::Add => add::run().await?,
        Command::Agent => agent::run().await?,
    }

    Ok(())
}
