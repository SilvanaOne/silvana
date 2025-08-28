// Module declarations
pub mod chain;
pub mod coin;
pub mod error;
pub mod fetch;
pub mod interface;
pub mod keypair;
pub mod parse;
pub mod state;
pub mod transactions;

// Re-export commonly used types
pub use chain::{get_reference_gas_price, load_sender_from_env, pick_gas_object};
pub use coin::{
    CoinInfo, CoinLockGuard, CoinLockManager, fetch_coin, get_coin_lock_manager, list_coins,
};
pub use interface::SilvanaSuiInterface;
pub use keypair::{
    bcs_serialize, generate_ed25519, parse_address, parse_sui_private_key, sign_message,
    verify_with_address,
};
pub use state::SharedSuiState;
pub use transactions::{
    complete_job_tx, fail_job_tx, start_job_tx, submit_proof_tx, update_state_for_sequence_tx,
};
// Re-export selected fetch utilities for convenient access at crate root
pub use fetch::fetch_agent_method;
