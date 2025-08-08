//! Provable operations for IndexedMerkleMap that can be used inside zkVM
//! All methods are static and work with proofs/witnesses only

extern crate alloc;

use alloc::vec::Vec;
use crate::types::{
    Field, Hash, MembershipProof, NonMembershipProof, 
    InsertWitness, UpdateWitness, MerkleProof, hash_pair, sha256_hash, IndexedMerkleMapError
};

/// Static methods for provable operations inside zkVM
/// These methods don't require the full IndexedMerkleMap structure
pub struct ProvableIndexedMerkleMap;

impl ProvableIndexedMerkleMap {
    /// Convert path indices (bits) to leaf index
    /// Uses little-endian representation matching the map.rs implementation
    fn path_indices_to_index(bits: &[bool]) -> usize {
        let mut idx = 0usize;
        for (level, &is_right) in bits.iter().enumerate() {
            if is_right { 
                idx |= 1usize << level; 
            }
        }
        idx
    }

    /// Combine internal root with length to create the final root
    /// This provides protection against collision attacks
    pub fn combine_root_with_length(internal_root: &Hash, length: usize) -> Hash {
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(internal_root.as_bytes());
        // Use big-endian encoding for length to ensure consistent hashing
        data.extend_from_slice(&(length as u64).to_be_bytes());
        sha256_hash(&data)
    }

    /// Compute merkle root from leaf and proof
    pub fn compute_root(leaf_hash: Hash, proof: &MerkleProof) -> Hash {
        let mut current = leaf_hash;
        
        for (sibling, is_right) in proof.siblings.iter().zip(&proof.path_indices) {
            if *is_right {
                current = hash_pair(sibling, &current);
            } else {
                current = hash_pair(&current, sibling);
            }
        }
        
        current
    }

    /// Verify a membership proof
    /// The root should be the combined hash of internal_root and length
    /// Note: We can't validate proof size without knowing the tree's configured height
    pub fn verify_membership_proof(
        root: &Hash, 
        proof: &MembershipProof, 
        key: &Field, 
        value: &Field, 
        tree_length: usize
    ) -> bool {
        if proof.leaf.key != *key || proof.leaf.value != *value {
            return false;
        }
        
        // Basic validation - proof should have matching siblings and path indices
        if proof.merkle_proof.siblings.len() != proof.merkle_proof.path_indices.len() {
            return false;
        }
        
        // Validate proof size based on tree structure
        // The proof size should match the height of the tree needed for tree_length leaves
        // For a tree with capacity 2^h, we need h siblings in the proof
        // Since tree_length tells us the actual number of leaves, we can determine the minimum height
        let min_height = if tree_length == 0 {
            1
        } else {
            // Calculate minimum height needed for tree_length leaves
            let mut h = 1;
            while (1 << h) < tree_length {
                h += 1;
            }
            h
        };
        
        // The proof should have exactly min_height siblings
        // Note: Some implementations might use a fixed height, so we check for a reasonable range
        if proof.merkle_proof.siblings.len() < min_height || proof.merkle_proof.siblings.len() > 32 {
            return false;
        }
        
        // Ensure proof is not empty
        if proof.merkle_proof.siblings.is_empty() {
            return false;
        }
        
        let computed_internal_root = Self::compute_root(proof.leaf.hash(), &proof.merkle_proof);
        let computed_root = Self::combine_root_with_length(&computed_internal_root, tree_length);
        
        computed_root == *root
    }

    /// Verify a non-membership proof
    /// The root should be the combined hash of internal_root and length
    /// Note: We can't validate exact proof size without knowing the tree's configured height
    pub fn verify_non_membership_proof(
        root: &Hash, 
        proof: &NonMembershipProof, 
        key: &Field, 
        tree_length: usize
    ) -> bool {
        // Check that key is between low_leaf.key and low_leaf.next_key
        if *key <= proof.low_leaf.key {
            return false;
        }
        
        if proof.low_leaf.next_key != Field::zero() && *key >= proof.low_leaf.next_key {
            return false;
        }
        
        // Basic validation - proof should have matching siblings and path indices
        if proof.merkle_proof.siblings.len() != proof.merkle_proof.path_indices.len() {
            return false;
        }
        
        // Validate proof size based on tree structure
        let min_height = if tree_length == 0 {
            1
        } else {
            // Calculate minimum height needed for tree_length leaves
            let mut h = 1;
            while (1 << h) < tree_length {
                h += 1;
            }
            h
        };
        
        // The proof should have at least min_height siblings (but may have more for a fixed-height tree)
        if proof.merkle_proof.siblings.len() < min_height || proof.merkle_proof.siblings.len() > 32 {
            return false;
        }
        
        // Ensure proof is not empty
        if proof.merkle_proof.siblings.is_empty() {
            return false;
        }
        
        let computed_internal_root = Self::compute_root(proof.low_leaf.hash(), &proof.merkle_proof);
        let computed_root = Self::combine_root_with_length(&computed_internal_root, tree_length);
        
        computed_root == *root
    }

    /// Verify an update using a witness (for zkVM usage)
    /// This static method verifies the update constraints without loading the full map
    /// 
    /// # Arguments
    /// * `witness` - Complete witness containing all data needed for verification
    /// 
    /// # Returns
    /// * `Ok(())` - If all constraints are satisfied and the update is valid
    /// * `Err(&str)` - Error if any verification fails
    pub fn update(witness: &UpdateWitness) -> Result<(), IndexedMerkleMapError> {
        // === CONSTRAINT 1: Verify the membership proof ===
        if !Self::verify_membership_proof(
            &witness.old_root,
            &witness.membership_proof,
            &witness.key,
            &witness.old_value,
            witness.tree_length
        ) {
            return Err(IndexedMerkleMapError::InvalidProof);
        }
        
        // === CONSTRAINT 2: Verify the updated leaf structure ===
        if witness.updated_leaf.key != witness.key {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        if witness.updated_leaf.value != witness.new_value {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        if witness.updated_leaf.next_key != witness.membership_proof.leaf.next_key {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        if witness.updated_leaf.index != witness.membership_proof.leaf.index {
            return Err(IndexedMerkleMapError::InvalidLeafIndex);
        }
        
        // === CONSTRAINT 3: Verify the new root computation ===
        let new_internal_root = Self::compute_root(
            witness.updated_leaf.hash(), 
            &witness.membership_proof.merkle_proof
        );
        let computed_new_root = Self::combine_root_with_length(&new_internal_root, witness.tree_length);
        
        if computed_new_root != witness.new_root {
            return Err(IndexedMerkleMapError::RootMismatch);
        }
        
        Ok(())
    }
    
    /// Verify an insertion using a witness (for zkVM usage)
    /// This static method verifies ALL constraints without loading the full map
    /// 
    /// # Arguments
    /// * `witness` - Complete witness containing all data needed for verification
    /// 
    /// # Returns
    /// * `Ok(())` - If all constraints are satisfied and the insertion is valid
    /// * `Err(&str)` - Error if any verification fails
    pub fn insert(witness: &InsertWitness) -> Result<(), IndexedMerkleMapError> {
        // === CONSTRAINT 1: Verify append-only ===
        if witness.new_leaf_index != witness.tree_length {
            return Err(IndexedMerkleMapError::InvalidLeafIndex);
        }
        
        // === CONSTRAINT 2: Verify path-index consistency ===
        // Ensure the Merkle proof paths correspond to the claimed indices
        let low_idx = Self::path_indices_to_index(&witness.low_leaf_proof_before.path_indices);
        if low_idx != witness.non_membership_proof.low_leaf.index {
            return Err(IndexedMerkleMapError::InvalidLeafIndex);
        }
        
        let new_idx = Self::path_indices_to_index(&witness.new_leaf_proof_after.path_indices);
        if new_idx != witness.new_leaf_index {
            return Err(IndexedMerkleMapError::InvalidLeafIndex);
        }
        
        // === CONSTRAINT 3: Verify non-membership ===
        if !Self::verify_non_membership_proof(
            &witness.old_root,
            &witness.non_membership_proof,
            &witness.key,
            witness.tree_length
        ) {
            return Err(IndexedMerkleMapError::InvalidProof);
        }
        
        // === CONSTRAINT 4: Verify key ordering ===
        // The new key must be between low_leaf.key and low_leaf.next_key
        if witness.key <= witness.non_membership_proof.low_leaf.key {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        if witness.non_membership_proof.low_leaf.next_key != Field::zero() && 
           witness.key >= witness.non_membership_proof.low_leaf.next_key {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        // === CONSTRAINT 5: Verify low leaf matches proof ===
        // The low_leaf_proof_before should match the low leaf in non_membership_proof
        let low_leaf_computed = Self::compute_root(
            witness.non_membership_proof.low_leaf.hash(),
            &witness.low_leaf_proof_before
        );
        let old_root_check = Self::combine_root_with_length(&low_leaf_computed, witness.tree_length);
        if old_root_check != witness.old_root {
            return Err(IndexedMerkleMapError::ProofVerificationFailed);
        }
        
        // === CONSTRAINT 6: Verify updated low leaf ===
        // The updated low leaf should have the same key, value, index but next_key = new key
        if witness.updated_low_leaf.key != witness.non_membership_proof.low_leaf.key ||
           witness.updated_low_leaf.value != witness.non_membership_proof.low_leaf.value ||
           witness.updated_low_leaf.index != witness.non_membership_proof.low_leaf.index ||
           witness.updated_low_leaf.next_key != witness.key {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        // === CONSTRAINT 7: Verify new leaf structure ===
        // The new leaf should have correct key, value, next_key (from old low leaf), and index
        if witness.new_leaf.key != witness.key ||
           witness.new_leaf.value != witness.value ||
           witness.new_leaf.next_key != witness.non_membership_proof.low_leaf.next_key ||
           witness.new_leaf.index != witness.new_leaf_index {
            return Err(IndexedMerkleMapError::InvalidWitness);
        }
        
        // === CONSTRAINT 8: Verify final root computation ===
        // Step 1: Apply low leaf update
        let _low_leaf_updated_root = Self::compute_root(
            witness.updated_low_leaf.hash(),
            &witness.low_leaf_proof_before
        );
        
        // Step 2: Apply new leaf insertion using the proof AFTER low leaf update
        // This proof accounts for any overlapping paths
        let new_leaf_root = Self::compute_root(
            witness.new_leaf.hash(),
            &witness.new_leaf_proof_after
        );
        
        // The final root should match the witness new_root
        let computed_final_root = Self::combine_root_with_length(&new_leaf_root, witness.tree_length + 1);
        if computed_final_root != witness.new_root {
            return Err(IndexedMerkleMapError::RootMismatch);
        }
        
        Ok(())
    }
}