//! IndexedMerkleMap - A Rust implementation optimized for SP1 zkVM
//! 
//! This library provides two APIs:
//! - `IndexedMerkleMap`: Full implementation for use outside zkVM (feature: default)
//! - `ProvableIndexedMerkleMap`: Static methods for use inside zkVM (feature: zkvm)
//! 
//! When compiling for zkVM, use `--features zkvm --no-default-features` to only
//! include the provable code and types.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

// Common types module - always available
pub mod types;
pub use types::{
    Field, Hash, Leaf, MembershipProof, NonMembershipProof, 
    InsertWitness, UpdateWitness, MerkleProof, IndexedMerkleMapError
};

// Provable module - always available for zkVM operations
pub mod provable;
pub use provable::ProvableIndexedMerkleMap;

// Full map implementation - only available when not in zkvm mode
#[cfg(not(feature = "zkvm"))]
pub mod map;

#[cfg(not(feature = "zkvm"))]
pub use map::IndexedMerkleMap;
