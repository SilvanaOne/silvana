use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "coordinator")]
#[command(about = "Silvana CLI", long_about = None)]
pub struct Cli {
    /// Override the blockchain network (devnet, testnet, or mainnet)
    #[arg(long, global = true, env = "SUI_CHAIN")]
    pub chain: Option<String>,
    
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the Silvana node
    Start {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,

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
        
        /// Filter to only process jobs from a specific app instance
        #[arg(long, env = "APP_INSTANCE_FILTER")]
        instance: Option<String>,
        
        /// Run as a dedicated settlement node (only process settlement jobs)
        #[arg(long, env = "SETTLE_ONLY", default_value = "false")]
        settle: bool,
    },
    
    /// Fetch and display an app instance by ID
    Instance {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// The app instance ID to fetch
        instance: String,
    },
    
    /// Fetch and display a raw Sui object by ID
    Object {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// The object ID to fetch
        object: String,
    },
    
    /// Fetch and display a block by number
    Block {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// The app instance ID to fetch from
        instance: String,
        
        /// The block number to fetch
        block: u64,
    },
    
    /// Fetch and display proof calculations for a block
    Proofs {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// The app instance ID to fetch from
        instance: String,
        
        /// The block number to fetch proofs for
        block: u64,
    },
    
    /// Fetch and display a job by sequence number
    Job {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
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
        rpc_url: Option<String>,
        
        /// The app instance ID to fetch jobs from
        instance: String,
        
        /// Fetch failed jobs instead of active jobs
        #[arg(long, default_value = "false")]
        failed: bool,
    },
    
    /// Execute blockchain transactions
    Transaction {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// Private key for signing transactions (optional, defaults to SUI_SECRET_KEY env var)
        #[arg(long, env = "SUI_PRIVATE_KEY")]
        private_key: Option<String>,
        
        #[command(subcommand)]
        tx_type: TransactionType,
    },
    
    /// Check balance
    Balance {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
        
        /// Address to check balance for (defaults to SUI_ADDRESS env var)
        address: Option<String>,
    },
    
    /// Split coins to maintain gas coin pool
    Split {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
    },
    
    /// Display network information
    Network {
        #[arg(long, env = "SUI_RPC_URL")]
        rpc_url: Option<String>,
    },
    
    /// Request tokens from the faucet
    Faucet {
        /// Address to fund (defaults to SUI_ADDRESS env var)
        #[arg(long, env = "SUI_ADDRESS")]
        address: Option<String>,
        
        /// Amount of SUI to request (e.g., 10.0 for 10 SUI)
        #[arg(long, default_value = "10.0")]
        amount: f64,
    },
}

#[derive(Subcommand)]
pub enum TransactionType {
    /// Terminate a job on the blockchain
    TerminateJob {
        /// The app instance ID
        instance: String,
        
        /// The job sequence number to terminate
        job: u64,
        
        /// Gas budget in SUI (e.g., 0.2 for 0.2 SUI)
        #[arg(long, default_value = "0.1")]
        gas: f64,
    },
    
    /// Restart a specific failed job on the blockchain
    RestartFailedJob {
        /// The app instance ID
        instance: String,
        
        /// The job sequence number to restart
        job: u64,
        
        /// Gas budget in SUI (e.g., 0.2 for 0.2 SUI)
        #[arg(long, default_value = "0.1")]
        gas: f64,
    },
    
    /// Restart all failed jobs on the blockchain
    RestartFailedJobs {
        /// The app instance ID
        instance: String,
        
        /// Gas budget in SUI (e.g., 1.0 for 1 SUI, default higher for this heavy operation)
        #[arg(long, default_value = "1.0")]
        gas: f64,
    },
}

