use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, error};
use tracing_subscriber::FmtSubscriber;
use avs_operator::{
    config::load_config,
    register::register_operator,
    task::create_task,
    monitor::monitor_new_tasks,
    status::get_operator_status,
};

#[derive(Parser)]
#[command(
    name = "avs-operator",
    about = "AVS Operator CLI for managing EigenLayer AVS operations",
    version,
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register operator to EigenLayer and AVS
    Register {
        /// Override chain ID (default: from environment)
        #[arg(long)]
        chain_id: Option<u64>,
        
        /// Override RPC URL (default: from environment)
        #[arg(long)]
        rpc_url: Option<String>,
    },
    
    /// Monitor for new tasks and respond to them
    Monitor {
        /// Use polling instead of WebSocket
        #[arg(long, default_value = "false")]
        polling: bool,
        
        /// Override operator response percentage (0-100)
        #[arg(long)]
        response_percentage: Option<f64>,
    },
    
    /// Create a new task on the AVS
    Task {
        /// Task name to create
        #[arg(short, long)]
        name: String,
        
        /// Use aggregator key instead of operator key
        #[arg(long, default_value = "false")]
        use_aggregator: bool,
    },
    
    /// Get operator status and registration information
    Status {
        /// Operator address to check (default: from private key)
        #[arg(long)]
        address: Option<String>,
        
        /// Show detailed information
        #[arg(long, default_value = "false")]
        detailed: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file first
    dotenvy::dotenv().ok();
    
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    
    // Parse CLI arguments
    let cli = Cli::parse();
    
    // Load base configuration
    let mut config = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };
    
    // Execute command
    match cli.command {
        Commands::Register { chain_id, rpc_url } => {
            info!("AVS Operator Registration");
            
            // Override config if provided
            if let Some(chain_id) = chain_id {
                config.chain_id = chain_id;
            }
            if let Some(rpc_url) = rpc_url {
                // Update ws_url based on rpc_url
                config.ws_url = rpc_url.replace("https://", "wss://").replace("http://", "ws://");
                config.rpc_url = rpc_url;
            }
            
            info!("Configuration:");
            info!("  Chain ID: {}", config.chain_id);
            info!("  RPC URL: {}", config.rpc_url);
            info!("  Operator address will be derived from private key");
            
            match register_operator(&config).await {
                Ok(_) => {
                    info!("Operator registration completed successfully!");
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to register operator: {}", e);
                    Err(e.into())
                }
            }
        }
        
        Commands::Monitor { polling, response_percentage } => {
            info!("AVS Task Monitoring");
            
            // Override response percentage if provided
            if let Some(pct) = response_percentage {
                if pct < 0.0 || pct > 100.0 {
                    error!("Response percentage must be between 0 and 100");
                    return Err(anyhow::anyhow!("Invalid response percentage"));
                }
                config.operator_response_percentage = pct;
            }
            
            info!("Configuration:");
            info!("  Chain ID: {}", config.chain_id);
            info!("  RPC URL: {}", config.rpc_url);
            if !polling {
                info!("  WS URL: {}", config.ws_url);
            }
            info!("  Response percentage: {}%", config.operator_response_percentage);
            info!("  Mode: {}", if polling { "Polling" } else { "WebSocket" });
            
            // Set up Ctrl+C handler
            let (tx, rx) = tokio::sync::oneshot::channel();
            
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                tx.send(()).ok();
            });
            
            info!("Starting task monitoring... Press Ctrl+C to stop");
            
            // Run monitoring with cancellation
            tokio::select! {
                result = monitor_new_tasks(&config) => {
                    match result {
                        Ok(_) => info!("Monitoring completed"),
                        Err(e) => error!("Monitoring error: {}", e),
                    }
                }
                _ = rx => {
                    info!("Received shutdown signal, stopping...");
                }
            }
            
            info!("Monitoring stopped");
            Ok(())
        }
        
        Commands::Task { name, use_aggregator } => {
            info!("AVS Task Creation");
            
            info!("Configuration:");
            info!("  Chain ID: {}", config.chain_id);
            info!("  RPC URL: {}", config.rpc_url);
            info!("  Task name: {}", name);
            info!("  Using {} key", if use_aggregator { "aggregator" } else { "operator" });
            
            match create_task(&config, name, use_aggregator).await {
                Ok(_) => {
                    info!("Task created successfully!");
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to create task: {}", e);
                    Err(e.into())
                }
            }
        }
        
        Commands::Status { address, detailed } => {
            if detailed {
                info!("Fetching detailed operator status...");
                avs_operator::status::get_operator_details(&config, address).await?;
            } else {
                match get_operator_status(&config, address).await {
                    Ok(_) => {
                        // Status already printed by the function
                    }
                    Err(e) => {
                        error!("Failed to get operator status: {}", e);
                        return Err(e.into());
                    }
                }
            }
            Ok(())
        }
    }
}