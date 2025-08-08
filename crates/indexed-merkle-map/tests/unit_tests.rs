use indexed_merkle_map::{Field, Hash, IndexedMerkleMap, IndexedMerkleMapError};

#[test]
fn test_new_map() {
    let map = IndexedMerkleMap::new(10);
    assert_eq!(map.sorted_leaves().len(), 1);
    assert_eq!(map.sorted_leaves()[0].key, Field::zero());
}

#[test]
fn test_insert() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // Get root before insertion
    let root_before = map.root();
    
    // Get non-membership proof before insertion
    let proof_before = map.get_non_membership_proof(&key).unwrap();
    assert!(IndexedMerkleMap::verify_non_membership_proof(&root_before, &proof_before, &key, map.length()));
    
    // Insert the key
    map.insert(key, value).unwrap();
    
    // Now verify membership after insertion
    let proof_after = map.get_membership_proof(&key).unwrap();
    assert!(IndexedMerkleMap::verify_membership_proof(&map.root(), &proof_after, &key, &value, map.length()));
}

#[test]
fn test_membership_proof() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    map.insert(key, value).unwrap();
    
    let proof = map.get_membership_proof(&key).unwrap();
    assert!(IndexedMerkleMap::verify_membership_proof(&map.root(), &proof, &key, &value, map.length()));
}

#[test]
fn test_update() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value1 = Field::from_u32(200);
    let value2 = Field::from_u32(300);
    
    map.insert(key, value1).unwrap();
    
    let old_value = map.update(key, value2).unwrap();
    assert_eq!(old_value, value1);
    
    let proof = map.get_membership_proof(&key).unwrap();
    assert!(IndexedMerkleMap::verify_membership_proof(&map.root(), &proof, &key, &value2, map.length()));
}

#[test]
fn test_multiple_inserts() {
    let mut map = IndexedMerkleMap::new(10);
    
    let keys = vec![
        Field::from_u32(100),
        Field::from_u32(50),
        Field::from_u32(150),
        Field::from_u32(75),
    ];
    
    for (i, key) in keys.iter().enumerate() {
        let value = Field::from_u32((i + 1) as u32 * 100);
        map.insert(*key, value).unwrap();
    }
    
    // Verify all keys exist
    for (i, key) in keys.iter().enumerate() {
        let value = Field::from_u32((i + 1) as u32 * 100);
        let proof = map.get_membership_proof(key).unwrap();
        assert!(IndexedMerkleMap::verify_membership_proof(&map.root(), &proof, key, &value, map.length()));
    }
    
    // Verify non-membership
    let non_existent = Field::from_u32(60);
    let proof = map.get_non_membership_proof(&non_existent).unwrap();
    assert!(IndexedMerkleMap::verify_non_membership_proof(&map.root(), &proof, &non_existent, map.length()));
}

// Tests moved from o1js_compatibility.rs

#[test]
fn test_set_insert_new_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // Set should return Ok(None) for a new key
    let previous = map.set(key, value).unwrap();
    assert_eq!(previous, None);
    
    // Verify the key now exists with correct value
    assert_eq!(map.get_option(&key), Some(value));
}

#[test]
fn test_set_update_existing_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value1 = Field::from_u32(200);
    let value2 = Field::from_u32(300);
    
    // First set
    map.set(key, value1).unwrap();
    
    // Second set should return the previous value
    let previous = map.set(key, value2).unwrap();
    assert_eq!(previous, Some(value1));
    
    // Verify the new value
    assert_eq!(map.get_option(&key), Some(value2));
}

#[test]
fn test_get_option_existing_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    map.insert(key, value).unwrap();
    
    // get_option should return Some(value)
    assert_eq!(map.get_option(&key), Some(value));
}

#[test]
fn test_get_option_nonexistent_key() {
    let map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    
    // get_option should return None for non-existent key
    assert_eq!(map.get_option(&key), None);
}

#[test]
fn test_get_existing_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    map.insert(key, value).unwrap();
    
    // get should return the value
    assert_eq!(map.get(&key), value);
}

#[test]
#[should_panic(expected = "Key does not exist in the map")]
fn test_get_nonexistent_key_panics() {
    let map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    
    // get should panic for non-existent key
    map.get(&key);
}

#[test]
fn test_set_multiple_keys() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Set multiple keys
    let keys_values = vec![
        (Field::from_u32(10), Field::from_u32(100)),
        (Field::from_u32(20), Field::from_u32(200)),
        (Field::from_u32(30), Field::from_u32(300)),
    ];
    
    // First set should return None for all
    for (key, value) in &keys_values {
        let previous = map.set(*key, *value).unwrap();
        assert_eq!(previous, None);
    }
    
    // Verify all exist
    for (key, value) in &keys_values {
        assert_eq!(map.get_option(key), Some(*value));
    }
    
    // Update all values
    for (i, (key, value)) in keys_values.iter().enumerate() {
        let new_value = Field::from_u32((i as u32 + 1) * 1000);
        let previous = map.set(*key, new_value).unwrap();
        assert_eq!(previous, Some(*value));
    }
}

#[test]
fn test_set_zero_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    // The zero key is initially present with zero value
    let zero_key = Field::zero();
    let initial_value = map.get_option(&zero_key);
    assert_eq!(initial_value, Some(Field::zero()));
    
    // Update zero key
    let new_value = Field::from_u32(100);
    let previous = map.set(zero_key, new_value).unwrap();
    assert_eq!(previous, Some(Field::zero()));
    
    // Verify update
    assert_eq!(map.get_option(&zero_key), Some(new_value));
}

#[test]
fn test_set_at_capacity() {
    let mut map = IndexedMerkleMap::new(3); // Small tree
    
    // Tree of height 3 can hold 2^(3-1) = 4 leaves
    // First leaf is always (0,0,0), so we can set 3 more
    for i in 1..4 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        let previous = map.set(key, value).unwrap();
        assert_eq!(previous, None);
    }
    
    // Tree is at capacity, next set for a new key should return Err(TreeFull)
    let key = Field::from_u32(100);
    let value = Field::from_u32(1000);
    let result = map.set(key, value);
    assert!(result.is_err()); // Should return Err(TreeFull)
    assert_eq!(result.unwrap_err(), IndexedMerkleMapError::TreeFull);
    
    // But we can still update existing keys
    let existing_key = Field::from_u32(10);
    let new_value = Field::from_u32(999);
    let previous = map.set(existing_key, new_value).unwrap();
    assert_eq!(previous, Some(Field::from_u32(100)));
}

#[test]
fn test_workflow_increment_counter() {
    // This test mimics the o1js example: incrementing a counter
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(42);
    
    // Get the value (or default to 0 if doesn't exist) and increment
    let current_value = map.get_option(&key).unwrap_or(Field::zero());
    // Convert to u32 for simple increment (assuming small values for this test)
    let current_u32 = if current_value == Field::zero() { 0u32 } else { 1u32 };
    let new_value = Field::from_u32(current_u32 + 1);
    
    // Set the new value
    let previous = map.set(key, new_value).unwrap();
    assert_eq!(previous, None); // First time setting this key
    
    // Increment again
    let _current_value = map.get_option(&key).unwrap();
    let new_value = Field::from_u32(2); // We know it's 1, so increment to 2
    let previous = map.set(key, new_value).unwrap();
    assert_eq!(previous, Some(Field::from_u32(1)));
    
    // Final value should be 2
    assert_eq!(map.get(&key), Field::from_u32(2));
}

#[test]
fn test_set_preserves_tree_properties() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Use set to build a tree
    let keys = vec![50, 20, 80, 10, 60, 90, 30, 70, 40];
    
    for k in keys {
        let key = Field::from_u32(k);
        let value = Field::from_u32(k * 10);
        map.set(key, value).unwrap();
    }
    
    // Verify sorted order is maintained
    let mut prev_key = Field::zero();
    for leaf in map.sorted_leaves() {
        assert!(leaf.key >= prev_key);
        prev_key = leaf.key;
    }
    
    // Verify linked list structure
    let sorted_keys = vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90];
    for i in 0..sorted_keys.len() - 1 {
        let key = Field::from_u32(sorted_keys[i]);
        let proof = map.get_membership_proof(&key).unwrap();
        if i < sorted_keys.len() - 1 {
            assert_eq!(proof.leaf.next_key, Field::from_u32(sorted_keys[i + 1]));
        }
    }
}

// Tests moved from basic_operations.rs

#[test]
fn test_initialization() {
    let map = IndexedMerkleMap::new(10);
    
    // Check initial state
    assert_eq!(map.sorted_leaves().len(), 1);
    assert_eq!(map.sorted_leaves()[0].key, Field::zero());
    assert_eq!(map.sorted_leaves()[0].value, Field::zero());
    assert_eq!(map.sorted_leaves()[0].next_key, Field::zero());
    
    // Root should be non-zero
    assert_ne!(map.root(), Hash::zero());
}

#[test]
fn test_single_insert() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // Insert should succeed
    let result = map.insert(key, value);
    assert!(result.is_ok());
    
    // Verify the key now exists
    let membership_proof = map.get_membership_proof(&key).unwrap();
    assert_eq!(membership_proof.leaf.key, key);
    assert_eq!(membership_proof.leaf.value, value);
}

#[test]
fn test_multiple_inserts_sequential() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys in sequential order
    for i in 1..=10 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    // Verify all keys exist
    for i in 1..=10 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.key, key);
        assert_eq!(proof.leaf.value, value);
    }
}

#[test]
fn test_multiple_inserts_random_order() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert keys in random order
    let keys = vec![50, 20, 80, 10, 60, 90, 30, 70, 40, 100];
    
    for (i, &k) in keys.iter().enumerate() {
        let key = Field::from_u32(k);
        let value = Field::from_u32((i + 1) as u32 * 100);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    // Verify all keys exist with correct values
    for (i, &k) in keys.iter().enumerate() {
        let key = Field::from_u32(k);
        let value = Field::from_u32((i + 1) as u32 * 100);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.key, key);
        assert_eq!(proof.leaf.value, value);
    }
    
    // Verify sorted order is maintained
    let mut prev_key = Field::zero();
    for leaf in map.sorted_leaves() {
        assert!(leaf.key >= prev_key);
        prev_key = leaf.key;
    }
}

#[test]
fn test_update_existing_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value1 = Field::from_u32(200);
    let value2 = Field::from_u32(300);
    
    // Insert initial value
    map.insert(key, value1).expect("Insert should succeed");
    
    // Update to new value
    let old_value = map.update(key, value2).expect("Update should succeed");
    assert_eq!(old_value, value1);
    
    // Verify new value
    let proof = map.get_membership_proof(&key).unwrap();
    assert_eq!(proof.leaf.value, value2);
}

#[test]
fn test_update_nonexistent_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // Update should fail for non-existent key
    let result = map.update(key, value);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), IndexedMerkleMapError::KeyDoesNotExist);
}

#[test]
fn test_insert_duplicate_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    // First insert should succeed
    map.insert(key, value).expect("First insert should succeed");
    
    // Second insert with same key should fail
    let result = map.insert(key, value);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), IndexedMerkleMapError::KeyAlreadyExists);
}

#[test]
fn test_get_membership_proof_existing() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    map.insert(key, value).expect("Insert should succeed");
    
    let proof = map.get_membership_proof(&key);
    assert!(proof.is_some());
    
    let proof = proof.unwrap();
    assert_eq!(proof.leaf.key, key);
    assert_eq!(proof.leaf.value, value);
}

#[test]
fn test_get_membership_proof_nonexistent() {
    let map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    
    let proof = map.get_membership_proof(&key);
    assert!(proof.is_none());
}

#[test]
fn test_get_non_membership_proof_nonexistent() {
    let map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    
    let proof = map.get_non_membership_proof(&key);
    assert!(proof.is_some());
    
    let proof = proof.unwrap();
    // For empty tree, low leaf should be the zero leaf
    assert_eq!(proof.low_leaf.key, Field::zero());
    assert!(key > proof.low_leaf.key);
    assert_eq!(proof.low_leaf.next_key, Field::zero());
}

#[test]
fn test_get_non_membership_proof_existing() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    
    map.insert(key, value).expect("Insert should succeed");
    
    let proof = map.get_non_membership_proof(&key);
    assert!(proof.is_none());
}

#[test]
fn test_root_changes_on_insert() {
    let mut map = IndexedMerkleMap::new(10);
    
    let root1 = map.root();
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).expect("Insert should succeed");
    
    let root2 = map.root();
    
    assert_ne!(root1, root2);
}

#[test]
fn test_root_changes_on_update() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value1 = Field::from_u32(200);
    let value2 = Field::from_u32(300);
    
    map.insert(key, value1).expect("Insert should succeed");
    let root1 = map.root();
    
    map.update(key, value2).expect("Update should succeed");
    let root2 = map.root();
    
    assert_ne!(root1, root2);
}

#[test]
fn test_tree_height_limits() {
    // Test minimum height
    let map = IndexedMerkleMap::new(1);
    assert_eq!(map.height(), 1);
    
    // Test maximum height
    let map = IndexedMerkleMap::new(32);
    assert_eq!(map.height(), 32);
}

#[test]
#[should_panic(expected = "Height must be between 1 and 32")]
fn test_invalid_height_zero() {
    IndexedMerkleMap::new(0);
}

#[test]
#[should_panic(expected = "Height must be between 1 and 32")]
fn test_invalid_height_too_large() {
    IndexedMerkleMap::new(33);
}

#[test]
fn test_hash_from_bytes() {
    // Test creating Hash from [u8; 32]
    let bytes: [u8; 32] = [1; 32];
    let hash = Hash::from_bytes(bytes);
    assert_eq!(hash.to_bytes(), bytes);
    
    // Test round-trip conversion
    let original = Hash::from_bytes(bytes);
    let converted = Hash::from_bytes(original.to_bytes());
    assert_eq!(original, converted);
}

#[test]
fn test_hash_try_from_slice() {
    // Valid 32-byte slice
    let bytes = vec![2u8; 32];
    let hash = Hash::try_from_slice(&bytes);
    assert!(hash.is_some());
    assert_eq!(hash.unwrap().to_bytes(), [2u8; 32]);
    
    // Invalid size - too short
    let short_bytes = vec![3u8; 16];
    let hash = Hash::try_from_slice(&short_bytes);
    assert!(hash.is_none());
    
    // Invalid size - too long
    let long_bytes = vec![4u8; 64];
    let hash = Hash::try_from_slice(&long_bytes);
    assert!(hash.is_none());
    
    // Exact 32 bytes from array
    let array: [u8; 32] = [5; 32];
    let hash = Hash::try_from_slice(&array);
    assert!(hash.is_some());
    assert_eq!(hash.unwrap().to_bytes(), array);
}

#[test]
fn test_field_from_bytes() {
    // Test creating Field from [u8; 32]
    let bytes: [u8; 32] = [7; 32];
    let field = Field::from_bytes(bytes);
    assert_eq!(field.to_bytes(), bytes);
    
    // Test round-trip conversion
    let original = Field::from_bytes(bytes);
    let converted = Field::from_bytes(original.to_bytes());
    assert_eq!(original, converted);
}

#[test]
fn test_field_try_from_slice() {
    // Valid 32-byte slice
    let bytes = vec![8u8; 32];
    let field = Field::try_from_slice(&bytes);
    assert!(field.is_some());
    assert_eq!(field.unwrap().to_bytes(), [8u8; 32]);
    
    // Invalid size - too short
    let short_bytes = vec![9u8; 16];
    let field = Field::try_from_slice(&short_bytes);
    assert!(field.is_none());
    
    // Invalid size - too long
    let long_bytes = vec![10u8; 64];
    let field = Field::try_from_slice(&long_bytes);
    assert!(field.is_none());
    
    // Exact 32 bytes from array
    let array: [u8; 32] = [11; 32];
    let field = Field::try_from_slice(&array);
    assert!(field.is_some());
    assert_eq!(field.unwrap().to_bytes(), array);
}

#[test]
fn test_hash_new_equals_from_bytes() {
    let bytes: [u8; 32] = [42; 32];
    let hash1 = Hash::new(bytes);
    let hash2 = Hash::from_bytes(bytes);
    assert_eq!(hash1, hash2);
}

#[test] 
fn test_field_and_hash_byte_compatibility() {
    // Test that Field and Hash can share byte representations
    let bytes: [u8; 32] = [99; 32];
    let field = Field::from_bytes(bytes);
    let hash = Hash::from_bytes(bytes);
    
    assert_eq!(field.to_bytes(), hash.to_bytes());
    assert_eq!(field.as_bytes(), &bytes);
    assert_eq!(hash.as_bytes(), &bytes);
}