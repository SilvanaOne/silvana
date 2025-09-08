use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinaNetwork {
    /// The Mina GraphQL endpoints
    pub mina: Vec<String>,
    
    /// The archive endpoints
    pub archive: Vec<String>,
    
    /// The chain ID
    pub chain_id: String,
    
    /// The name of the network
    pub name: String,
    
    /// The account manager for Lightnet (optional)
    pub account_manager: Option<String>,
    
    /// The explorer account URL (optional)
    pub explorer_account_url: Option<String>,
    
    /// The explorer transaction URL (optional)
    pub explorer_transaction_url: Option<String>,
    
    /// The faucet URL (optional)
    pub faucet: Option<String>,
    
    /// The faucet API endpoint (optional)
    pub faucet_endpoint: Option<String>,
}

impl MinaNetwork {
    pub fn mainnet() -> Self {
        MinaNetwork {
            mina: vec![
                "https://api.minascan.io/node/mainnet/v1/graphql".to_string(),
            ],
            archive: vec![
                "https://api.minascan.io/archive/mainnet/v1/graphql".to_string(),
            ],
            explorer_account_url: Some("https://minascan.io/mainnet/account/".to_string()),
            explorer_transaction_url: Some("https://minascan.io/mainnet/tx/".to_string()),
            chain_id: "mina:mainnet".to_string(),
            name: "Mainnet".to_string(),
            account_manager: None,
            faucet: None,
            faucet_endpoint: None,
        }
    }
    
    pub fn devnet() -> Self {
        MinaNetwork {
            mina: vec![
                "https://api.minascan.io/node/devnet/v1/graphql".to_string(),
            ],
            archive: vec![
                "https://api.minascan.io/archive/devnet/v1/graphql".to_string(),
            ],
            explorer_account_url: Some("https://minascan.io/devnet/account/".to_string()),
            explorer_transaction_url: Some("https://minascan.io/devnet/tx/".to_string()),
            chain_id: "mina:devnet".to_string(),
            name: "Devnet".to_string(),
            account_manager: None,
            faucet: Some("https://faucet.minaprotocol.com".to_string()),
            faucet_endpoint: Some("https://faucet.minaprotocol.com/api/v1/faucet".to_string()),
        }
    }
    
    pub fn zeko() -> Self {
        MinaNetwork {
            mina: vec![
                "https://devnet.zeko.io/graphql".to_string(),
            ],
            archive: vec![
                "https://devnet.zeko.io/graphql".to_string(),
            ],
            explorer_account_url: Some("https://zekoscan.io/testnet/account/".to_string()),
            explorer_transaction_url: Some("https://zekoscan.io/testnet/tx/".to_string()),
            chain_id: "zeko:testnet".to_string(),
            name: "Zeko".to_string(),
            account_manager: None,
            faucet: Some("https://zeko.io/faucet".to_string()),
            faucet_endpoint: Some("https://zeko.io/api/faucet".to_string()),
        }
    }
    
    pub fn zeko_alphanet() -> Self {
        MinaNetwork {
            mina: vec![
                "http://m1.zeko.io/graphql".to_string(),
            ],
            archive: vec![
                "http://m1.zeko.io/graphql".to_string(),
            ],
            explorer_account_url: None,
            explorer_transaction_url: None,
            chain_id: "zeko:alphanet".to_string(),
            name: "Zeko AlphaNet".to_string(),
            account_manager: None,
            faucet: None,
            faucet_endpoint: None,
        }
    }
    
    pub fn lightnet() -> Self {
        MinaNetwork {
            mina: vec![
                "http://localhost:8080/graphql".to_string(),
            ],
            archive: vec![
                "http://localhost:8282".to_string(),
            ],
            explorer_account_url: None,
            explorer_transaction_url: None,
            chain_id: "mina:lightnet".to_string(),
            name: "Lightnet".to_string(),
            account_manager: Some("http://localhost:8181".to_string()),
            faucet: None,
            faucet_endpoint: None,
        }
    }
    
    pub fn local() -> Self {
        MinaNetwork {
            mina: vec![],
            archive: vec![],
            explorer_account_url: None,
            explorer_transaction_url: None,
            chain_id: "mina:local".to_string(),
            name: "Local".to_string(),
            account_manager: None,
            faucet: None,
            faucet_endpoint: None,
        }
    }
    
    /// Get a network by name
    pub fn get_network(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "mina:mainnet" | "mainnet" => Some(Self::mainnet()),
            "mina:devnet" | "devnet" => Some(Self::devnet()),
            "zeko:testnet" | "zeko" => Some(Self::zeko()),
            "zeko:alphanet" | "alphanet" => Some(Self::zeko_alphanet()),
            "mina:lightnet" | "lightnet" => Some(Self::lightnet()),
            "mina:local" | "local" => Some(Self::local()),
            _ => None,
        }
    }
    
    /// Get all available networks
    pub fn all_networks() -> Vec<MinaNetwork> {
        vec![
            Self::mainnet(),
            Self::devnet(),
            Self::zeko(),
            Self::zeko_alphanet(),
            Self::lightnet(),
            Self::local(),
        ]
    }
    
    /// Get networks with faucet support
    pub fn networks_with_faucet() -> Vec<MinaNetwork> {
        Self::all_networks()
            .into_iter()
            .filter(|n| n.faucet_endpoint.is_some())
            .collect()
    }
}