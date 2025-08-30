use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "coordinator")]
#[command(about = "Silvana Coordinator CLI", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the Silvana coordinator (default behavior)
    Start {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,

        #[arg(long, env = "SILVANA_REGISTRY_PACKAGE")]
        package_id: String,

        #[arg(long, env = "DOCKER_USE_TEE", default_value = "false")]
        use_tee: bool,

        #[arg(long, env = "CONTAINER_TIMEOUT_SECS", default_value = "900")]
        container_timeout: u64,

        #[arg(long, env = "LOG_LEVEL", default_value = "info")]
        log_level: String,

        #[arg(long, env = "GRPC_SOCKET_PATH", default_value = "/tmp/coordinator.sock")]
        grpc_socket_path: String,
    },
    
    /// Fetch and display an app instance by ID
    Instance {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,
        
        /// The app instance ID to fetch
        instance: String,
    },
    
    /// Fetch and display a block by number
    Block {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,
        
        /// The app instance ID to fetch from
        instance: String,
        
        /// The block number to fetch
        block: u64,
    },
    
    /// Fetch and display proof calculations for a block
    Proofs {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,
        
        /// The app instance ID to fetch from
        instance: String,
        
        /// The block number to fetch proofs for
        block: u64,
    },
    
    /// Fetch and display a job by sequence number
    Job {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,
        
        /// The app instance ID to fetch from
        instance: String,
        
        /// The job sequence number to fetch
        job: u64,
        
        /// Fetch from failed jobs table instead of active jobs
        #[arg(long, default_value = "false")]
        failed: bool,
    },
    
    /// Fetch and display all jobs from an app instance
    Jobs {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: String,
        
        /// The app instance ID to fetch jobs from
        instance: String,
        
        /// Fetch failed jobs instead of active jobs
        #[arg(long, default_value = "false")]
        failed: bool,
    },
}