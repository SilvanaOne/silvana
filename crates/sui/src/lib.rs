//! Sui blockchain integration library for Silvana
//!
//! This library provides functions for interacting with the Sui blockchain,
//! including registry management, app instance operations, and transaction handling.
//!
//! # Environment Variables
//!
//! The library expects the following environment variables to be set:
//! - `SUI_ADDRESS`: Your Sui address
//! - `SUI_SECRET_KEY`: Your Sui private key
//! - `SUI_CHAIN`: The Sui network to use (devnet, testnet, or mainnet)
//! - `SILVANA_REGISTRY_PACKAGE`: The registry package ID (optional)
//!
//! It's recommended to use a `.env` file with the `dotenvy` crate:
//! ```rust,no_run
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Load environment variables from .env file
//! dotenvy::dotenv().ok();
//!
//! // Initialize the shared state
//! let rpc_url = sui::chain::resolve_rpc_url(None, None)?;
//! sui::SharedSuiState::initialize(&rpc_url).await?;
//!
//! // Now you can use the library functions
//! # Ok(())
//! # }
//! ```

// Module declarations
pub mod app_instance;
pub mod balance;
pub mod chain;
pub mod coin;
pub mod coin_management;
pub mod constants;
pub mod coordination;
pub mod error;
pub mod events;
pub mod faucet;
pub mod fetch;
pub mod interface;
pub mod keypair;
pub mod network_info;
pub mod object_lock;
pub mod parse;
pub mod publish;
pub mod registry;
pub mod state;
pub mod transactions;
pub mod types;

// Re-export commonly used types
pub use app_instance::{add_metadata_tx, delete_kv_tx, set_kv_tx};
pub use coordination::SuiCoordination;
pub use balance::{
    BalanceInfo, get_balance_in_sui, get_balance_info, get_current_address, get_network_name,
    get_total_balance_sui, print_balance_info,
};
pub use chain::{get_reference_gas_price, load_sender_from_env, pick_gas_object, resolve_rpc_url};
pub use coin::{
    CoinInfo, CoinLockGuard, CoinLockManager, fetch_coin, get_coin_lock_manager, list_coins,
};
pub use coin_management::{
    CoinPoolConfig, GasCoinsInfo, ensure_gas_coin_pool, get_gas_coins_info,
    initialize_gas_coin_pool, merge_gas_coins, split_gas_coins,
};
pub use faucet::{
    FaucetNetwork, ensure_sufficient_balance, ensure_sufficient_balance_network, initialize_faucet,
    request_tokens_for_default_address, request_tokens_for_default_address_network,
    request_tokens_from_faucet, request_tokens_from_faucet_network,
};
pub use interface::SilvanaSuiInterface;
pub use keypair::{
    bcs_serialize, generate_ed25519, parse_address, parse_sui_private_key, sign_message,
    verify_with_address,
};
pub use network_info::{
    NetworkInfo, ServiceInfo, get_network_info, get_network_summary, get_service_info,
    get_service_info_full, print_network_info, verify_chain_config,
};
pub use object_lock::{ObjectLockGuard, ObjectLockManager, get_object_lock_manager};
pub use publish::{
    BuildResult, PublishResult, build_move_package, publish_move_package,
    update_package_dependencies,
};
pub use state::SharedSuiState;
pub use transactions::{fetch_transaction_events, fetch_transaction_events_as_json};
// Re-export selected fetch utilities for convenient access at crate root
pub use fetch::fetch_agent_method;
// Re-export multicall constants
pub use constants::get_max_operations_per_multicall;
// Re-export types
pub use types::{MulticallOperations, MulticallResult};
