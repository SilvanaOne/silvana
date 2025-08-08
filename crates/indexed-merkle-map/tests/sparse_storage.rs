use indexed_merkle_map::{Field, IndexedMerkleMap};

#[test]
fn test_sparse_storage_efficiency() {
    // Create a large tree but insert only a few elements
    let mut map = IndexedMerkleMap::new(20); // Height 20 can support ~500k leaves
    
    // Insert only 100 elements
    for i in 1..=100 {
        let key = Field::from_u32(i * 1000);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    // The sparse storage should only have nodes along the paths to these 100 leaves
    // Approximate: 100 leaves * 20 levels * 2 (some siblings) = ~4000 nodes max
    // Much better than 2^20 = 1,048,576 nodes in a dense array
    
    // Verify the tree still works correctly
    for i in 1..=100 {
        let key = Field::from_u32(i * 1000);
        let value = Field::from_u32(i);
        assert_eq!(map.get_option(&key), Some(value));
    }
    
    // Verify proofs still work
    let key = Field::from_u32(50000);
    let proof = map.get_membership_proof(&key).unwrap();
    let root = map.root();
    let tree_length = map.length();
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &Field::from_u32(50),
        tree_length
    ));
}

#[test]
fn test_sparse_storage_with_gaps() {
    let mut map = IndexedMerkleMap::new(15);
    
    // Insert elements with large gaps
    let keys = vec![1, 1000, 10000, 100000, 1000000];
    
    for k in &keys {
        let key = Field::from_u32(*k);
        let value = Field::from_u32(*k * 2);
        map.insert(key, value).unwrap();
    }
    
    // Verify all keys exist with correct values
    for k in &keys {
        let key = Field::from_u32(*k);
        let value = Field::from_u32(*k * 2);
        assert_eq!(map.get(&key), value);
    }
    
    // Verify non-membership for keys in gaps
    let non_existent = Field::from_u32(500);
    assert_eq!(map.get_option(&non_existent), None);
    
    let non_proof = map.get_non_membership_proof(&non_existent).unwrap();
    let root = map.root();
    let tree_length = map.length();
    assert!(IndexedMerkleMap::verify_non_membership_proof(
        &root,
        &non_proof,
        &non_existent,
        tree_length
    ));
}

#[test]
fn test_sparse_storage_update_efficiency() {
    let mut map = IndexedMerkleMap::new(18);
    
    // Insert sparse elements
    for i in 0..10 {
        let key = Field::from_u32(1 << i); // Powers of 2: 1, 2, 4, 8, 16, ...
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    // Update values - should only affect paths to these specific leaves
    for i in 0..10 {
        let key = Field::from_u32(1 << i);
        let new_value = Field::from_u32(i + 100);
        map.update(key, new_value).unwrap();
    }
    
    // Verify updates
    for i in 0..10 {
        let key = Field::from_u32(1 << i);
        let expected = Field::from_u32(i + 100);
        assert_eq!(map.get(&key), expected);
    }
}

#[test]
fn test_empty_tree_is_sparse() {
    let map = IndexedMerkleMap::new(25); // Very tall tree
    
    // Should only have the initial zero leaf and its path to root
    // Approximately 25 nodes instead of 2^25 = 33 million
    
    // Verify the tree works despite being mostly empty
    let zero_key = Field::zero();
    assert_eq!(map.get_option(&zero_key), Some(Field::zero()));
    
    // Non-membership proof for any non-zero key should work
    let key = Field::from_u32(12345);
    let proof = map.get_non_membership_proof(&key).unwrap();
    let root = map.root();
    let tree_length = map.length();
    assert!(IndexedMerkleMap::verify_non_membership_proof(
        &root,
        &proof,
        &key,
        tree_length
    ));
}

#[test]
fn test_sparse_tree_growth() {
    let mut map = IndexedMerkleMap::new(12);
    let mut roots = Vec::new();
    
    // Track roots as we add elements
    roots.push(map.root());
    
    // Add elements one by one and verify tree grows correctly
    for i in 1..=50 {
        let key = Field::from_u32(i * 100);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
        roots.push(map.root());
        
        // Verify all previous elements still accessible
        for j in 1..=i {
            let k = Field::from_u32(j * 100);
            let v = Field::from_u32(j);
            assert_eq!(map.get_option(&k), Some(v));
        }
    }
    
    // Verify all roots are different (tree changed with each insertion)
    for i in 1..roots.len() {
        assert_ne!(roots[i], roots[i-1], "Root should change after insertion");
    }
}

#[test]
fn test_sparse_serialization() {
    // The sparse storage should serialize efficiently
    let mut map = IndexedMerkleMap::new(16);
    
    // Add a few elements
    for i in 1..=5 {
        let key = Field::from_u32(i * 10000);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    // Get proof and serialize it
    let key = Field::from_u32(30000);
    let proof = map.get_membership_proof(&key).unwrap();
    let serialized = borsh::to_vec(&proof).unwrap();
    
    // Proof size should be reasonable (roughly height * 32 bytes per sibling)
    // For height 16, expect around 16 * 32 = 512 bytes plus overhead
    assert!(serialized.len() < 1000, "Proof too large: {} bytes", serialized.len());
    
    // Verify deserialized proof works
    let deserialized: indexed_merkle_map::MembershipProof = 
        borsh::from_slice(&serialized).unwrap();
    let root = map.root();
    let tree_length = map.length();
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &deserialized,
        &key,
        &Field::from_u32(3),
        tree_length
    ));
}

#[test]
fn test_worst_case_sparse_storage() {
    // Test with maximum spread - elements at opposite ends
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert at minimum and maximum reasonable values
    let min_key = Field::from_u32(1);
    let max_key = Field::from_u32(u32::MAX - 1);
    
    map.insert(min_key, Field::from_u32(111)).unwrap();
    map.insert(max_key, Field::from_u32(999)).unwrap();
    
    // Even with maximum spread, sparse storage should handle it efficiently
    assert_eq!(map.get(&min_key), Field::from_u32(111));
    assert_eq!(map.get(&max_key), Field::from_u32(999));
    
    // Verify both proofs work
    let min_proof = map.get_membership_proof(&min_key).unwrap();
    let max_proof = map.get_membership_proof(&max_key).unwrap();
    
    let root = map.root();
    let tree_length = map.length();
    
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &min_proof,
        &min_key,
        &Field::from_u32(111),
        tree_length
    ));
    
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &max_proof,
        &max_key,
        &Field::from_u32(999),
        tree_length
    ));
}