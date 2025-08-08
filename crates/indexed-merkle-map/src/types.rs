//! Common types used by both provable and non-provable code

extern crate alloc;

use alloc::vec::Vec;
use borsh::{BorshDeserialize, BorshSerialize};
use crypto_bigint::{Encoding, U256};
use sha2::{Digest, Sha256};
use core::fmt;

/// Error types for IndexedMerkleMap operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedMerkleMapError {
    /// Key already exists in the map
    KeyAlreadyExists,
    /// Key does not exist in the map
    KeyDoesNotExist,
    /// Invalid proof provided
    InvalidProof,
    /// Tree is full and cannot accept more entries
    TreeFull,
    /// Invalid tree height (must be between 1 and 32)
    InvalidHeight,
    /// Invalid witness data
    InvalidWitness,
    /// Merkle proof verification failed
    ProofVerificationFailed,
    /// Root mismatch during verification
    RootMismatch,
    /// Invalid leaf index
    InvalidLeafIndex,
    /// Invalid sibling path length
    InvalidSiblingPath,
}

impl fmt::Display for IndexedMerkleMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyAlreadyExists => write!(f, "Key already exists"),
            Self::KeyDoesNotExist => write!(f, "Key does not exist"),
            Self::InvalidProof => write!(f, "Invalid proof"),
            Self::TreeFull => write!(f, "Tree is full"),
            Self::InvalidHeight => write!(f, "Height must be between 1 and 32"),
            Self::InvalidWitness => write!(f, "Invalid witness data"),
            Self::ProofVerificationFailed => write!(f, "Proof verification failed"),
            Self::RootMismatch => write!(f, "Root mismatch"),
            Self::InvalidLeafIndex => write!(f, "Invalid leaf index"),
            Self::InvalidSiblingPath => write!(f, "Invalid sibling path length"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for IndexedMerkleMapError {}

/// Hash type - 32 bytes SHA256 output
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn new(bytes: [u8; 32]) -> Self {
        Hash(bytes)
    }

    pub fn zero() -> Self {
        Hash([0u8; 32])
    }

    /// Create a Hash from a byte array
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Hash(bytes)
    }

    /// Try to create a Hash from a byte slice
    /// Returns None if the slice is not exactly 32 bytes
    pub fn try_from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(bytes);
        Some(Hash(hash))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Get the inner byte array
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    pub fn to_u256(&self) -> U256 {
        U256::from_be_bytes(self.0)
    }
}

/// SHA256 hash function
pub fn sha256_hash(data: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&result);
    Hash::from_bytes(bytes)
}

/// Hash two hashes together (for merkle tree nodes)
pub fn hash_pair(left: &Hash, right: &Hash) -> Hash {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(left.as_bytes());
    data.extend_from_slice(right.as_bytes());
    sha256_hash(&data)
}

/// Wrapper for U256 to implement serialization traits
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Field([u8; 32]);

impl Field {
    pub fn zero() -> Self {
        Field([0u8; 32])
    }

    pub fn from_u256(val: U256) -> Self {
        Field(val.to_be_bytes())
    }

    pub fn to_u256(&self) -> U256 {
        U256::from_be_bytes(self.0)
    }

    pub fn from_u32(val: u32) -> Self {
        Self::from_u256(U256::from_u32(val))
    }

    /// Create a Field from a byte array
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Field(bytes)
    }

    /// Try to create a Field from a byte slice
    /// Returns None if the slice is not exactly 32 bytes
    pub fn try_from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut field_bytes = [0u8; 32];
        field_bytes.copy_from_slice(bytes);
        Some(Field(field_bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Get the inner byte array
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl BorshSerialize for Field {
    fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        writer.write_all(&self.0)?;
        Ok(())
    }
}

impl BorshDeserialize for Field {
    fn deserialize_reader<R: borsh::io::Read>(reader: &mut R) -> borsh::io::Result<Self> {
        let mut bytes = [0u8; 32];
        reader.read_exact(&mut bytes)?;
        Ok(Field(bytes))
    }
}

/// A leaf in the indexed merkle tree
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Leaf {
    pub key: Field,
    pub value: Field,
    pub next_key: Field,
    pub index: usize,
}

impl Leaf {
    pub fn new(key: Field, value: Field, next_key: Field, index: usize) -> Self {
        Leaf { key, value, next_key, index }
    }

    pub fn empty() -> Self {
        Leaf {
            key: Field::zero(),
            value: Field::zero(),
            next_key: Field::zero(),
            index: 0,
        }
    }

    /// Hash the leaf node for inclusion in the merkle tree
    pub fn hash(&self) -> Hash {
        let mut data = Vec::with_capacity(96);
        data.extend_from_slice(self.key.as_bytes());
        data.extend_from_slice(self.value.as_bytes());
        data.extend_from_slice(self.next_key.as_bytes());
        sha256_hash(&data)
    }
}

/// Merkle proof path
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct MerkleProof {
    pub siblings: Vec<Hash>,
    pub path_indices: Vec<bool>, // true = right, false = left
}

/// Proof of membership in the indexed merkle tree
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct MembershipProof {
    pub leaf: Leaf,
    pub merkle_proof: MerkleProof,
}

/// Proof of non-membership in the indexed merkle tree
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct NonMembershipProof {
    pub low_leaf: Leaf,
    pub merkle_proof: MerkleProof,
}

/// Complete witness for inserting a new key-value pair
/// Contains all data needed to verify the insertion in zkVM
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct InsertWitness {
    /// The root before insertion
    pub old_root: Hash,
    /// The root after insertion  
    pub new_root: Hash,
    /// The key being inserted
    pub key: Field,
    /// The value being inserted
    pub value: Field,
    /// The index where the new leaf is inserted
    pub new_leaf_index: usize,
    /// The tree length before insertion
    pub tree_length: usize,
    /// Non-membership proof for the key
    pub non_membership_proof: NonMembershipProof,
    /// Membership proof for the low leaf (before update)
    pub low_leaf_proof_before: MerkleProof,
    /// The updated low leaf after insertion
    pub updated_low_leaf: Leaf,
    /// The new leaf being inserted
    pub new_leaf: Leaf,
    /// Merkle proof for the new leaf after low leaf update (accounts for overlapping paths)
    pub new_leaf_proof_after: MerkleProof,
}

/// Complete witness for updating an existing key-value pair
/// Contains all data needed to verify the update in zkVM
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct UpdateWitness {
    /// The root before update
    pub old_root: Hash,
    /// The root after update
    pub new_root: Hash,
    /// The key being updated
    pub key: Field,
    /// The old value being replaced
    pub old_value: Field,
    /// The new value being set
    pub new_value: Field,
    /// The tree length (remains unchanged during update)
    pub tree_length: usize,
    /// Membership proof for the key (before update)
    pub membership_proof: MembershipProof,
    /// The updated leaf after the update
    pub updated_leaf: Leaf,
}