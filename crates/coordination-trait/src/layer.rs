//! Coordination layer types and operation modes

use serde::{Deserialize, Serialize};
use std::fmt;

/// Enum representing different coordination layers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoordinationLayer {
    /// Sui blockchain
    Sui,
    /// Ethereum mainnet
    Ethereum,
    /// Polygon (MATIC)
    Polygon,
    /// Base L2
    Base,
    /// Solana blockchain
    Solana,
    /// Private coordination layer (TiDB-backed)
    Private,
}

impl fmt::Display for CoordinationLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sui => write!(f, "Sui"),
            Self::Ethereum => write!(f, "Ethereum"),
            Self::Polygon => write!(f, "Polygon"),
            Self::Base => write!(f, "Base"),
            Self::Solana => write!(f, "Solana"),
            Self::Private => write!(f, "Private"),
        }
    }
}

impl CoordinationLayer {
    /// Check if this is an EVM-compatible chain
    pub fn is_evm(&self) -> bool {
        matches!(self, Self::Ethereum | Self::Polygon | Self::Base)
    }

    /// Check if this is a blockchain (vs private layer)
    pub fn is_blockchain(&self) -> bool {
        !matches!(self, Self::Private)
    }

    /// Get the default operation mode for this layer
    pub fn default_operation_mode(&self) -> CoordinationLayerOperationMode {
        match self {
            Self::Private => CoordinationLayerOperationMode::Direct,
            _ => CoordinationLayerOperationMode::Multicall,
        }
    }
}

/// Operation mode for a coordination layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationLayerOperationMode {
    /// Direct execution - operations are executed immediately without batching
    /// Used for cheap/fast layers like Private
    Direct,

    /// Multicall batching - operations are batched to share transaction fees
    /// Used for blockchain layers like Sui, Ethereum, etc.
    Multicall,
}

impl fmt::Display for CoordinationLayerOperationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => write!(f, "Direct"),
            Self::Multicall => write!(f, "Multicall"),
        }
    }
}