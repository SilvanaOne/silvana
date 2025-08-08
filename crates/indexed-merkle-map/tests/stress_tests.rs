//! Stress tests for IndexedMerkleMap covering edge cases and limits

use indexed_merkle_map::{Field, Hash, IndexedMerkleMap, IndexedMerkleMapError, MembershipProof, MerkleProof, Leaf};
use crypto_bigint::U256;

// ============= Maximum Tree Capacity Tests =============

#[test]
fn test_maximum_capacity_small_tree() {
    // Tree of height 2 can hold 2^(2-1) = 2 leaves (including the zero leaf)
    let mut map = IndexedMerkleMap::new(2);
    
    // First leaf is always (0,0,0), so we can only insert 1 more
    let key1 = Field::from_u32(10);
    let value1 = Field::from_u32(100);
    
    // First insert should succeed
    assert!(map.insert(key1, value1).is_ok());
    
    // Tree is now at capacity, next insert should fail
    let key2 = Field::from_u32(20);
    let value2 = Field::from_u32(200);
    assert_eq!(map.insert(key2, value2).unwrap_err(), IndexedMerkleMapError::TreeFull);
    
    // But updating existing key should still work
    let new_value = Field::from_u32(150);
    assert!(map.update(key1, new_value).is_ok());
}

#[test]
fn test_maximum_capacity_medium_tree() {
    // Tree of height 3 can hold 2^(3-1) = 4 leaves
    let mut map = IndexedMerkleMap::new(3);
    
    // Can insert 3 keys (plus the zero leaf = 4 total)
    for i in 1..=3 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        assert!(map.insert(key, value).is_ok(), "Insert {} should succeed", i);
    }
    
    // Fourth insert should fail - tree full
    let key = Field::from_u32(40);
    let value = Field::from_u32(400);
    assert_eq!(map.insert(key, value).unwrap_err(), IndexedMerkleMapError::TreeFull);
}

#[test]
fn test_fill_tree_to_capacity() {
    // Tree of height 4 can hold 2^(4-1) = 8 leaves
    let mut map = IndexedMerkleMap::new(4);
    let capacity = (1 << (4 - 1)) - 1; // -1 for the zero leaf
    
    // Fill the tree to capacity
    for i in 1..=capacity {
        let key = Field::from_u32(i as u32 * 10);
        let value = Field::from_u32(i as u32 * 100);
        assert!(map.insert(key, value).is_ok(), "Insert {} should succeed", i);
    }
    
    // Verify we're at capacity
    assert_eq!(map.length(), capacity + 1); // +1 for zero leaf
    
    // Next insert should fail
    let overflow_key = Field::from_u32(9999);
    let overflow_value = Field::from_u32(99999);
    assert_eq!(
        map.insert(overflow_key, overflow_value).unwrap_err(),
        IndexedMerkleMapError::TreeFull
    );
    
    // All existing keys should still be retrievable
    for i in 1..=capacity {
        let key = Field::from_u32(i as u32 * 10);
        let expected_value = Field::from_u32(i as u32 * 100);
        assert_eq!(map.get(&key), expected_value);
    }
}

#[test]
fn test_set_at_maximum_capacity() {
    // Tree of height 2 can hold 2 leaves
    let mut map = IndexedMerkleMap::new(2);
    
    // Fill to capacity
    let key1 = Field::from_u32(10);
    let value1 = Field::from_u32(100);
    map.set(key1, value1).unwrap();
    
    // Try to set a new key when at capacity
    let key2 = Field::from_u32(20);
    let value2 = Field::from_u32(200);
    let result = map.set(key2, value2);
    
    // set() should return Err(TreeFull) when it can't insert
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), IndexedMerkleMapError::TreeFull);
    
    // But updating existing key with set() should work
    let new_value = Field::from_u32(150);
    let prev = map.set(key1, new_value).unwrap();
    assert_eq!(prev, Some(value1));
}

// ============= Malformed Proof Rejection Tests =============

#[test]
fn test_malformed_proof_wrong_leaf_hash() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();
    
    // Tamper with the leaf to create wrong hash
    proof.leaf.next_key = Field::from_u32(999);
    
    // This should fail verification because leaf hash won't match
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &value, map.length()));
}

#[test]
fn test_malformed_proof_corrupted_sibling() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert multiple keys to create a non-trivial tree
    for i in 1..=5 {
        map.insert(Field::from_u32(i * 10), Field::from_u32(i * 100)).unwrap();
    }
    
    let key = Field::from_u32(30);
    let value = Field::from_u32(300);
    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();
    
    // Corrupt a sibling hash
    if !proof.merkle_proof.siblings.is_empty() {
        proof.merkle_proof.siblings[0] = Hash::new([0xFF; 32]);
    }
    
    // Verification should fail with corrupted sibling
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &value, map.length()));
}

#[test]
fn test_malformed_proof_flipped_path_bit() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert multiple keys
    for i in 1..=5 {
        map.insert(Field::from_u32(i * 10), Field::from_u32(i * 100)).unwrap();
    }
    
    let key = Field::from_u32(30);
    let value = Field::from_u32(300);
    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();
    
    // Flip a path bit
    if !proof.merkle_proof.path_indices.is_empty() {
        proof.merkle_proof.path_indices[0] = !proof.merkle_proof.path_indices[0];
    }
    
    // Verification should fail with wrong path
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &value, map.length()));
}

#[test]
fn test_malformed_proof_wrong_tree_length() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    let root = map.root();
    let proof = map.get_membership_proof(&key).unwrap();
    
    // Verify with wrong tree length
    let wrong_length = map.length() + 10;
    
    // This should fail because the root is computed with tree length
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &value, wrong_length));
}

#[test]
fn test_malformed_non_membership_proof() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys to create gaps
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    let root = map.root();
    let key = Field::from_u32(20); // Non-existent key between 10 and 30
    let mut proof = map.get_non_membership_proof(&key).unwrap();
    
    // Tamper with low_leaf to make it invalid
    proof.low_leaf.next_key = Field::from_u32(15); // Should be 30
    
    // Verification should fail
    assert!(!IndexedMerkleMap::verify_non_membership_proof(&root, &proof, &key, map.length()));
}

#[test]
fn test_proof_with_all_zero_siblings() {
    let map = IndexedMerkleMap::new(10);
    let root = map.root();
    
    // Create a proof with all zero siblings (invalid for non-zero root)
    let fake_proof = MembershipProof {
        leaf: Leaf {
            key: Field::from_u32(100),
            value: Field::from_u32(200),
            next_key: Field::zero(),
            index: 1,
        },
        merkle_proof: MerkleProof {
            siblings: vec![Hash::zero(); 9],
            path_indices: vec![false; 9],
        },
    };
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // This should fail because the computed root won't match
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &fake_proof, &key, &value, map.length()));
}

// ============= Field Overflow Condition Tests =============

#[test]
fn test_field_max_value() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Test with maximum U256 value
    let max_u256 = U256::MAX;
    let max_key = Field::from_u256(max_u256);
    let max_value = Field::from_u256(max_u256);
    
    // Should be able to insert max values
    assert!(map.insert(max_key, max_value).is_ok());
    
    // Should be able to retrieve
    assert_eq!(map.get(&max_key), max_value);
    
    // Should be able to generate and verify proof
    let proof = map.get_membership_proof(&max_key).unwrap();
    let root = map.root();
    assert!(IndexedMerkleMap::verify_membership_proof(&root, &proof, &max_key, &max_value, map.length()));
}

#[test]
fn test_field_ordering_with_large_values() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Create fields with values that test the full range
    // Note: U256::from_be_slice(&[0xFF; 32]) == U256::MAX, so we'll use a different value
    let fields = vec![
        Field::from_u256(U256::from_u64(0)),
        Field::from_u256(U256::from_u64(1)),
        Field::from_u256(U256::from_u64(u64::MAX)),
        Field::from_u256(U256::from_u128(u128::MAX)),
        Field::from_u256(U256::from_be_slice(&[0xFE; 32])), // Near max value (not quite max)
        Field::from_u256(U256::MAX),
    ];
    
    // Insert in random order (skip zero which already exists)
    for (i, &field) in fields.iter().enumerate() {
        if field != Field::zero() { // Skip zero which is already in tree
            let value = Field::from_u32(i as u32);
            assert!(map.insert(field, value).is_ok(), "Failed to insert field at index {}", i);
        }
    }
    
    // Verify sorted order is maintained
    let leaves = map.sorted_leaves();
    for i in 1..leaves.len() {
        assert!(leaves[i-1].key <= leaves[i].key, 
            "Leaves not in sorted order at index {}", i);
    }
}

#[test]
fn test_field_arithmetic_consistency() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Test that field conversions are consistent
    let test_values = vec![
        0u32,
        1,
        42,
        u32::MAX,
    ];
    
    for val in test_values {
        let key = Field::from_u32(val);
        let value = Field::from_u32(val);
        
        // Insert
        if val > 0 { // Skip zero
            assert!(map.insert(key, value).is_ok());
        }
        
        // Verify round-trip conversion
        let retrieved = map.get(&key);
        assert_eq!(retrieved, value);
        
        // Verify the internal representation
        let u256_val = retrieved.to_u256();
        assert_eq!(u256_val, U256::from_u32(val));
    }
}

#[test]
fn test_hash_collision_resistance() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys that might produce similar hashes
    let keys = vec![
        Field::from_u256(U256::from_u64(0x0000_0000_0000_0001)),
        Field::from_u256(U256::from_u64(0x0000_0000_0000_0100)),
        Field::from_u256(U256::from_u64(0x0000_0000_0001_0000)),
        Field::from_u256(U256::from_u64(0x0000_0001_0000_0000)),
        Field::from_u256(U256::from_u64(0x0001_0000_0000_0000)),
        Field::from_u256(U256::from_u64(0x0100_0000_0000_0000)),
    ];
    
    let mut roots = Vec::new();
    
    for (i, &key) in keys.iter().enumerate() {
        if i > 0 { // Skip zero
            let value = Field::from_u32(i as u32);
            assert!(map.insert(key, value).is_ok());
            roots.push(map.root());
        }
    }
    
    // All roots should be different (no collisions)
    for i in 1..roots.len() {
        for j in 0..i {
            assert_ne!(roots[i], roots[j], 
                "Root collision detected between insertions {} and {}", j, i);
        }
    }
}

#[test]
fn test_proof_verification_with_overflow_values() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Use values near the overflow boundary
    let large_bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00];
    let large_val = U256::from_be_slice(&large_bytes);
    let key = Field::from_u256(large_val);
    let value = Field::from_u256(U256::from_u128(u128::MAX));
    
    map.insert(key, value).unwrap();
    
    // Generate and verify proof
    let proof = map.get_membership_proof(&key).unwrap();
    let root = map.root();
    
    // Proof should verify correctly even with large values
    assert!(IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &value, map.length()));
    
    // Try to verify with slightly different large values (should fail)
    let wrong_bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                       0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x01];
    let wrong_key = Field::from_u256(U256::from_be_slice(&wrong_bytes));
    assert!(!IndexedMerkleMap::verify_membership_proof(&root, &proof, &wrong_key, &value, map.length()));
}

#[test]
fn test_boundary_values_in_tree() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Test boundary values
    let boundaries = vec![
        Field::from_u256(U256::from_u64(0)),  // Already in tree as zero leaf
        Field::from_u256(U256::from_u64(1)),  // Smallest non-zero
        Field::from_u256(U256::from_u128(u128::MAX)), // 2^128 - 1 (max u128)
        Field::from_u256(U256::from_be_slice(&[0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                                              0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                                              0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                                              0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])), // 2^255 - 1
        Field::from_u256(U256::MAX), // Maximum possible
    ];
    
    for (i, &boundary) in boundaries.iter().enumerate() {
        if i > 0 { // Skip zero
            let value = Field::from_u32(i as u32);
            assert!(map.insert(boundary, value).is_ok(), 
                "Failed to insert boundary value {}", i);
        }
    }
    
    // Verify all values are correctly stored and retrievable
    for (i, &boundary) in boundaries.iter().enumerate() {
        if i == 0 {
            assert_eq!(map.get(&boundary), Field::zero());
        } else {
            assert_eq!(map.get(&boundary), Field::from_u32(i as u32));
        }
    }
}