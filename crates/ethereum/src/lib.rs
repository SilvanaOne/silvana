pub mod keypair;
pub mod balance;
pub mod networks;
pub mod avs_operator;
pub mod avs_contracts;

pub use keypair::{
    generate_ethereum_keypair,
    keypair_from_private_key,
    verify_keypair,
    parse_address,
    GeneratedKeypair,
};

pub use balance::{
    get_balance,
    get_balance_wei,
    get_account_info,
    account_exists,
    wei_to_ether,
    ether_to_wei,
    AccountInfo,
};

pub use networks::{
    EthereumNetwork,
    NativeCurrency,
};

pub use avs_operator::{
    AvsConfig,
    AvsContracts,
    AvsOperatorClient,
    SignatureWithSaltAndExpiry,
    Task,
    DecodedTaskEvent,
    generate_operator_signature,
    generate_random_salt,
    parse_address as avs_parse_address,
    encode_task_response,
};

// Re-export alloy types that are used by avs-operator
pub use alloy::primitives::U256;