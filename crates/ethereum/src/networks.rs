use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use once_cell::sync::Lazy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumNetwork {
    pub name: String,
    pub chain_id: u64,
    pub rpc_endpoints: Vec<String>,
    pub explorer: Option<String>,
    pub native_currency: NativeCurrency,
    pub testnet: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeCurrency {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

impl EthereumNetwork {
    pub fn get_network(name: &str) -> Option<&'static EthereumNetwork> {
        NETWORKS.get(name)
    }
    
    pub fn list_networks() -> Vec<&'static str> {
        NETWORKS.keys().map(|s| s.as_str()).collect()
    }
}

static NETWORKS: Lazy<HashMap<String, EthereumNetwork>> = Lazy::new(|| {
    let mut networks = HashMap::new();
    
    // Ethereum Mainnet
    networks.insert("mainnet".to_string(), EthereumNetwork {
        name: "Ethereum Mainnet".to_string(),
        chain_id: 1,
        rpc_endpoints: vec![
            "https://eth-mainnet.g.alchemy.com/v2/demo".to_string(),
            "https://eth.llamarpc.com".to_string(),
            "https://rpc.ankr.com/eth".to_string(),
            "https://eth-mainnet.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://etherscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Sepolia Testnet
    networks.insert("sepolia".to_string(), EthereumNetwork {
        name: "Sepolia Testnet".to_string(),
        chain_id: 11155111,
        rpc_endpoints: vec![
            "https://eth-sepolia.g.alchemy.com/v2/demo".to_string(),
            "https://rpc.sepolia.org".to_string(),
            "https://rpc.ankr.com/eth_sepolia".to_string(),
            "https://sepolia.drpc.org".to_string(),
        ],
        explorer: Some("https://sepolia.etherscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Sepolia Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Holesky Testnet
    networks.insert("holesky".to_string(), EthereumNetwork {
        name: "Holesky Testnet".to_string(),
        chain_id: 17000,
        rpc_endpoints: vec![
            "https://rpc.holesky.io".to_string(),
            "https://holesky.drpc.org".to_string(),
            "https://rpc.ankr.com/eth_holesky".to_string(),
        ],
        explorer: Some("https://holesky.etherscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Holesky Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Arbitrum One
    networks.insert("arbitrum".to_string(), EthereumNetwork {
        name: "Arbitrum One".to_string(),
        chain_id: 42161,
        rpc_endpoints: vec![
            "https://arb1.arbitrum.io/rpc".to_string(),
            "https://rpc.ankr.com/arbitrum".to_string(),
            "https://arbitrum-one.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://arbiscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Arbitrum Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Arbitrum Sepolia
    networks.insert("arbitrum-sepolia".to_string(), EthereumNetwork {
        name: "Arbitrum Sepolia".to_string(),
        chain_id: 421614,
        rpc_endpoints: vec![
            "https://sepolia-rollup.arbitrum.io/rpc".to_string(),
            "https://arbitrum-sepolia.blockpi.network/v1/rpc/public".to_string(),
        ],
        explorer: Some("https://sepolia.arbiscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Arbitrum Sepolia Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Optimism
    networks.insert("optimism".to_string(), EthereumNetwork {
        name: "Optimism".to_string(),
        chain_id: 10,
        rpc_endpoints: vec![
            "https://mainnet.optimism.io".to_string(),
            "https://rpc.ankr.com/optimism".to_string(),
            "https://optimism-mainnet.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://optimistic.etherscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Optimism Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Optimism Sepolia
    networks.insert("optimism-sepolia".to_string(), EthereumNetwork {
        name: "Optimism Sepolia".to_string(),
        chain_id: 11155420,
        rpc_endpoints: vec![
            "https://sepolia.optimism.io".to_string(),
            "https://optimism-sepolia.blockpi.network/v1/rpc/public".to_string(),
        ],
        explorer: Some("https://sepolia-optimism.etherscan.io".to_string()),
        native_currency: NativeCurrency {
            name: "Optimism Sepolia Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Base
    networks.insert("base".to_string(), EthereumNetwork {
        name: "Base".to_string(),
        chain_id: 8453,
        rpc_endpoints: vec![
            "https://mainnet.base.org".to_string(),
            "https://base.llamarpc.com".to_string(),
            "https://rpc.ankr.com/base".to_string(),
        ],
        explorer: Some("https://basescan.org".to_string()),
        native_currency: NativeCurrency {
            name: "Base Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Base Sepolia
    networks.insert("base-sepolia".to_string(), EthereumNetwork {
        name: "Base Sepolia".to_string(),
        chain_id: 84532,
        rpc_endpoints: vec![
            "https://sepolia.base.org".to_string(),
            "https://base-sepolia-rpc.publicnode.com".to_string(),
        ],
        explorer: Some("https://sepolia.basescan.org".to_string()),
        native_currency: NativeCurrency {
            name: "Base Sepolia Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Polygon
    networks.insert("polygon".to_string(), EthereumNetwork {
        name: "Polygon".to_string(),
        chain_id: 137,
        rpc_endpoints: vec![
            "https://polygon-rpc.com".to_string(),
            "https://rpc.ankr.com/polygon".to_string(),
            "https://polygon-mainnet.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://polygonscan.com".to_string()),
        native_currency: NativeCurrency {
            name: "MATIC".to_string(),
            symbol: "MATIC".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Polygon Amoy Testnet
    networks.insert("polygon-amoy".to_string(), EthereumNetwork {
        name: "Polygon Amoy".to_string(),
        chain_id: 80002,
        rpc_endpoints: vec![
            "https://rpc-amoy.polygon.technology".to_string(),
            "https://polygon-amoy-bor-rpc.publicnode.com".to_string(),
        ],
        explorer: Some("https://amoy.polygonscan.com".to_string()),
        native_currency: NativeCurrency {
            name: "MATIC".to_string(),
            symbol: "MATIC".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Binance Smart Chain
    networks.insert("bsc".to_string(), EthereumNetwork {
        name: "BNB Smart Chain".to_string(),
        chain_id: 56,
        rpc_endpoints: vec![
            "https://bsc-dataseed.binance.org".to_string(),
            "https://rpc.ankr.com/bsc".to_string(),
            "https://bsc-mainnet.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://bscscan.com".to_string()),
        native_currency: NativeCurrency {
            name: "BNB".to_string(),
            symbol: "BNB".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // BSC Testnet
    networks.insert("bsc-testnet".to_string(), EthereumNetwork {
        name: "BNB Smart Chain Testnet".to_string(),
        chain_id: 97,
        rpc_endpoints: vec![
            "https://data-seed-prebsc-1-s1.binance.org:8545".to_string(),
            "https://bsc-testnet.public.blastapi.io".to_string(),
        ],
        explorer: Some("https://testnet.bscscan.com".to_string()),
        native_currency: NativeCurrency {
            name: "Test BNB".to_string(),
            symbol: "tBNB".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Avalanche C-Chain
    networks.insert("avalanche".to_string(), EthereumNetwork {
        name: "Avalanche C-Chain".to_string(),
        chain_id: 43114,
        rpc_endpoints: vec![
            "https://api.avax.network/ext/bc/C/rpc".to_string(),
            "https://rpc.ankr.com/avalanche".to_string(),
            "https://avalanche-c-chain.publicnode.com".to_string(),
        ],
        explorer: Some("https://snowtrace.io".to_string()),
        native_currency: NativeCurrency {
            name: "Avalanche".to_string(),
            symbol: "AVAX".to_string(),
            decimals: 18,
        },
        testnet: false,
    });
    
    // Avalanche Fuji Testnet
    networks.insert("avalanche-fuji".to_string(), EthereumNetwork {
        name: "Avalanche Fuji".to_string(),
        chain_id: 43113,
        rpc_endpoints: vec![
            "https://api.avax-test.network/ext/bc/C/rpc".to_string(),
            "https://rpc.ankr.com/avalanche_fuji".to_string(),
        ],
        explorer: Some("https://testnet.snowtrace.io".to_string()),
        native_currency: NativeCurrency {
            name: "Avalanche".to_string(),
            symbol: "AVAX".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    // Local development
    networks.insert("localhost".to_string(), EthereumNetwork {
        name: "Localhost".to_string(),
        chain_id: 31337,
        rpc_endpoints: vec![
            "http://127.0.0.1:8545".to_string(),
            "http://localhost:8545".to_string(),
        ],
        explorer: None,
        native_currency: NativeCurrency {
            name: "Ether".to_string(),
            symbol: "ETH".to_string(),
            decimals: 18,
        },
        testnet: true,
    });
    
    networks
});