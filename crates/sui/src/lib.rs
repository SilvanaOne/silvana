// Module declarations
pub mod chain;
pub mod coin;
pub mod coin_management;
pub mod error;
pub mod events;
pub mod fetch;
pub mod interface;
pub mod keypair;
pub mod object_lock;
pub mod parse;
pub mod state;
pub mod transactions;

// Re-export commonly used types
pub use chain::{get_reference_gas_price, load_sender_from_env, pick_gas_object};
pub use coin::{
    CoinInfo, CoinLockGuard, CoinLockManager, fetch_coin, get_coin_lock_manager, list_coins,
};
pub use coin_management::{
    ensure_gas_coin_pool, initialize_gas_coin_pool, split_gas_coins, get_gas_coins_info,
    CoinPoolConfig, GasCoinsInfo,
};
pub use object_lock::{ObjectLockGuard, ObjectLockManager, get_object_lock_manager};
pub use interface::SilvanaSuiInterface;
pub use keypair::{
    bcs_serialize, generate_ed25519, parse_address, parse_sui_private_key, sign_message,
    verify_with_address,
};
pub use state::SharedSuiState;
pub use transactions::{
    add_metadata_tx, complete_job_tx, delete_kv_tx, fail_job_tx, set_kv_tx, start_job_tx,
    submit_proof_tx, update_state_for_sequence_tx,
};
// Re-export selected fetch utilities for convenient access at crate root
pub use fetch::fetch_agent_method;
