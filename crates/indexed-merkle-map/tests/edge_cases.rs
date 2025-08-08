use crypto_bigint::U256;
use indexed_merkle_map::{Field, IndexedMerkleMap, IndexedMerkleMapError};

#[test]
fn test_zero_key_value() {
    let mut map = IndexedMerkleMap::new(10);
    
    // The zero key is reserved as the first leaf
    // Updating it should work
    let zero_key = Field::zero();
    let new_value = Field::from_u32(100);
    
    let result = map.update(zero_key, new_value);
    assert!(result.is_ok());
    
    let proof = map.get_membership_proof(&zero_key).unwrap();
    assert_eq!(proof.leaf.value, new_value);
}

#[test]
fn test_max_value_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Test with maximum U256 value
    let max_u256 = U256::MAX;
    let max_key = Field::from_u256(max_u256);
    let value = Field::from_u32(100);
    
    // Should be able to insert max value
    let result = map.insert(max_key, value);
    assert!(result.is_ok());
    
    // Verify it exists
    let proof = map.get_membership_proof(&max_key).unwrap();
    assert_eq!(proof.leaf.key, max_key);
}

#[test]
fn test_boundary_values() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Test with values near boundaries
    let keys = vec![
        Field::from_u32(1),          // Smallest non-zero
        Field::from_u32(u32::MAX),    // Max u32
        Field::from_u256(U256::from_u64(u64::MAX)), // Max u64
    ];
    
    for (i, key) in keys.iter().enumerate() {
        let value = Field::from_u32(i as u32);
        map.insert(*key, value).expect("Insert should succeed");
    }
    
    // Verify all exist
    for (i, key) in keys.iter().enumerate() {
        let proof = map.get_membership_proof(key).unwrap();
        assert_eq!(proof.leaf.value, Field::from_u32(i as u32));
    }
}

#[test]
fn test_adjacent_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert adjacent keys
    for i in 1..=5 {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    // Verify linked list structure
    for i in 1..5 {
        let key = Field::from_u32(i);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.next_key, Field::from_u32(i + 1));
    }
    
    // Last key should point to zero (max value)
    let last_proof = map.get_membership_proof(&Field::from_u32(5)).unwrap();
    assert_eq!(last_proof.leaf.next_key, Field::zero());
}

#[test]
fn test_sparse_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert very sparse keys
    let keys = vec![
        Field::from_u32(1),
        Field::from_u32(1000000),
        Field::from_u256(U256::from_u128(u128::MAX / 2)),
    ];
    
    for (i, key) in keys.iter().enumerate() {
        let value = Field::from_u32(i as u32);
        map.insert(*key, value).expect("Insert should succeed");
    }
    
    // Verify all exist and linked list is correct
    for i in 0..keys.len() - 1 {
        let proof = map.get_membership_proof(&keys[i]).unwrap();
        assert_eq!(proof.leaf.next_key, keys[i + 1]);
    }
}

#[test]
fn test_fill_tree_to_capacity() {
    // Use a small tree to test capacity
    let mut map = IndexedMerkleMap::new(3);
    
    // Tree of height 3 can hold 2^(3-1) = 4 leaves at the leaf level
    // First leaf is always (0,0,0), so we can insert 3 more
    for i in 1..4 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        let result = map.insert(key, value);
        assert!(result.is_ok(), "Insert {} should succeed", i);
    }
    
    // Tree should be at capacity
    assert_eq!(map.next_index(), 4);
    
    // Next insert should fail (tree is at capacity)
    let key = Field::from_u32(100);
    let value = Field::from_u32(1000);
    let result = map.insert(key, value);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), IndexedMerkleMapError::TreeFull);
}

#[test]
fn test_insert_in_reverse_order() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys in reverse order
    for i in (1..=10).rev() {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    // Verify sorted order is maintained
    let mut prev_key = Field::zero();
    for leaf in map.sorted_leaves() {
        assert!(leaf.key >= prev_key);
        prev_key = leaf.key;
    }
    
    // Verify linked list structure
    for i in 1..10 {
        let key = Field::from_u32(i);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.next_key, Field::from_u32(i + 1));
    }
}

#[test]
fn test_insert_between_existing_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys with gaps
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Insert key in between
    map.insert(Field::from_u32(20), Field::from_u32(200)).unwrap();
    
    // Verify linked list is correct
    let proof10 = map.get_membership_proof(&Field::from_u32(10)).unwrap();
    assert_eq!(proof10.leaf.next_key, Field::from_u32(20));
    
    let proof20 = map.get_membership_proof(&Field::from_u32(20)).unwrap();
    assert_eq!(proof20.leaf.next_key, Field::from_u32(30));
    
    let proof30 = map.get_membership_proof(&Field::from_u32(30)).unwrap();
    assert_eq!(proof30.leaf.next_key, Field::zero());
}

#[test]
fn test_non_membership_between_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some keys
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(20), Field::from_u32(200)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Test non-membership for key between existing keys
    let non_existent = Field::from_u32(15);
    let proof = map.get_non_membership_proof(&non_existent).unwrap();
    
    // Low leaf should be 10
    assert_eq!(proof.low_leaf.key, Field::from_u32(10));
    assert_eq!(proof.low_leaf.next_key, Field::from_u32(20));
    
    // Verify the proof
    let root = map.root();
    let tree_length = map.length();
    assert!(IndexedMerkleMap::verify_non_membership_proof(&root, &proof, &non_existent, tree_length));
}

#[test]
fn test_update_maintains_linked_list() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert multiple keys
    for i in 1..=5 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).unwrap();
    }
    
    // Update middle key
    let key = Field::from_u32(30);
    let new_value = Field::from_u32(999);
    map.update(key, new_value).unwrap();
    
    // Verify linked list structure is unchanged
    let proof20 = map.get_membership_proof(&Field::from_u32(20)).unwrap();
    assert_eq!(proof20.leaf.next_key, Field::from_u32(30));
    
    let proof30 = map.get_membership_proof(&Field::from_u32(30)).unwrap();
    assert_eq!(proof30.leaf.next_key, Field::from_u32(40));
    assert_eq!(proof30.leaf.value, new_value);
}

#[test]
fn test_identical_values_different_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    let value = Field::from_u32(100);
    
    // Insert same value with different keys
    for i in 1..=5 {
        let key = Field::from_u32(i * 10);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    // Verify all keys exist with same value
    for i in 1..=5 {
        let key = Field::from_u32(i * 10);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.value, value);
    }
}