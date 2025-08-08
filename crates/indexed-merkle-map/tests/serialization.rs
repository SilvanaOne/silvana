use indexed_merkle_map::{
    Field, Hash, IndexedMerkleMap, Leaf, MembershipProof, NonMembershipProof,
    MerkleProof, InsertWitness, UpdateWitness
};

#[test]
fn test_field_serialization() {
    let field = Field::from_u32(12345);
    
    // Serialize
    let serialized = borsh::to_vec(&field).unwrap();
    assert_eq!(serialized.len(), 32); // Field is 32 bytes
    
    // Deserialize
    let deserialized: Field = borsh::from_slice(&serialized).unwrap();
    assert_eq!(field, deserialized);
}

#[test]
fn test_hash_serialization() {
    let hash = Hash::new([42u8; 32]);
    
    // Serialize
    let serialized = borsh::to_vec(&hash).unwrap();
    assert_eq!(serialized.len(), 32);
    
    // Deserialize
    let deserialized: Hash = borsh::from_slice(&serialized).unwrap();
    assert_eq!(hash, deserialized);
}

#[test]
fn test_leaf_serialization() {
    let leaf = Leaf::new(
        Field::from_u32(100),
        Field::from_u32(200),
        Field::from_u32(300),
        5,
    );
    
    // Serialize
    let serialized = borsh::to_vec(&leaf).unwrap();
    
    // Deserialize
    let deserialized: Leaf = borsh::from_slice(&serialized).unwrap();
    assert_eq!(leaf, deserialized);
    assert_eq!(deserialized.key, Field::from_u32(100));
    assert_eq!(deserialized.value, Field::from_u32(200));
    assert_eq!(deserialized.next_key, Field::from_u32(300));
    assert_eq!(deserialized.index, 5);
}

#[test]
fn test_membership_proof_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some data
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    // Get membership proof
    let proof = map.get_membership_proof(&key).unwrap();
    
    // Serialize
    let serialized = borsh::to_vec(&proof).unwrap();
    
    // Deserialize
    let deserialized: MembershipProof = borsh::from_slice(&serialized).unwrap();
    
    // Verify fields match
    assert_eq!(proof.leaf, deserialized.leaf);
    assert_eq!(proof.merkle_proof.siblings.len(), deserialized.merkle_proof.siblings.len());
    assert_eq!(proof.merkle_proof.path_indices, deserialized.merkle_proof.path_indices);
    
    // Verify deserialized proof still works
    let root = map.root();
    assert!(IndexedMerkleMap::verify_membership_proof(&root, &deserialized, &key, &value, map.length()));
}

#[test]
fn test_non_membership_proof_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some data with gap
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Get non-membership proof for key in gap
    let non_existent = Field::from_u32(20);
    let proof = map.get_non_membership_proof(&non_existent).unwrap();
    
    // Serialize
    let serialized = borsh::to_vec(&proof).unwrap();
    
    // Deserialize
    let deserialized: NonMembershipProof = borsh::from_slice(&serialized).unwrap();
    
    // Verify fields match
    assert_eq!(proof.low_leaf, deserialized.low_leaf);
    assert_eq!(proof.merkle_proof.siblings.len(), deserialized.merkle_proof.siblings.len());
    
    // Verify deserialized proof still works
    let root = map.root();
    assert!(IndexedMerkleMap::verify_non_membership_proof(&root, &deserialized, &non_existent, map.length()));
}

#[test]
fn test_large_proof_serialization() {
    let mut map = IndexedMerkleMap::new(20); // Large tree
    
    // Insert many keys
    for i in 1..=100 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).unwrap();
    }
    
    // Get proof for middle key
    let key = Field::from_u32(500);
    let value = Field::from_u32(5000);
    let proof = map.get_membership_proof(&key).unwrap();
    
    // Serialize
    let serialized = borsh::to_vec(&proof).unwrap();
    
    // Check size is reasonable
    assert!(serialized.len() < 10000); // Should be much smaller
    
    // Deserialize and verify
    let deserialized: MembershipProof = borsh::from_slice(&serialized).unwrap();
    let root = map.root();
    assert!(IndexedMerkleMap::verify_membership_proof(&root, &deserialized, &key, &value, map.length()));
}

#[test]
fn test_zero_field_serialization() {
    let zero = Field::zero();
    
    // Serialize
    let serialized = borsh::to_vec(&zero).unwrap();
    assert_eq!(serialized, vec![0u8; 32]);
    
    // Deserialize
    let deserialized: Field = borsh::from_slice(&serialized).unwrap();
    assert_eq!(zero, deserialized);
}

#[test]
fn test_max_field_serialization() {
    use crypto_bigint::U256;
    
    let max = Field::from_u256(U256::MAX);
    
    // Serialize
    let serialized = borsh::to_vec(&max).unwrap();
    assert_eq!(serialized.len(), 32);
    
    // Deserialize
    let deserialized: Field = borsh::from_slice(&serialized).unwrap();
    assert_eq!(max, deserialized);
}

#[test]
fn test_proof_size_scaling() {
    println!("\nProof size scaling test:");
    
    for height in [5, 10, 15, 20] {
        let mut map = IndexedMerkleMap::new(height);
        
        // Insert a key
        let key = Field::from_u32(100);
        let value = Field::from_u32(200);
        map.insert(key, value).unwrap();
        
        // Get proof
        let proof = map.get_membership_proof(&key).unwrap();
        
        // Serialize
        let serialized = borsh::to_vec(&proof).unwrap();
        
        println!(
            "Height {}: Proof size = {} bytes ({} siblings)",
            height,
            serialized.len(),
            proof.merkle_proof.siblings.len()
        );
        
        // Expected size: leaf (32*3 + 8) + siblings (32 * (height-1)) + path_indices
        let expected_min = 32 * 3 + 8 + 32 * (height - 1);
        assert!(serialized.len() >= expected_min);
    }
}

#[test]
fn test_corrupted_data_deserialization() {
    let field = Field::from_u32(12345);
    let mut serialized = borsh::to_vec(&field).unwrap();
    
    // Corrupt the data
    serialized.truncate(30); // Make it too short
    
    // Deserialization should fail
    let result: Result<Field, _> = borsh::from_slice(&serialized);
    assert!(result.is_err());
}

#[test]
fn test_empty_proof_serialization() {
    // Create a minimal proof
    let leaf = Leaf::empty();
    let merkle_proof = indexed_merkle_map::MerkleProof {
        siblings: vec![],
        path_indices: vec![],
    };
    let proof = MembershipProof {
        leaf,
        merkle_proof,
    };
    
    // Serialize
    let serialized = borsh::to_vec(&proof).unwrap();
    
    // Deserialize
    let deserialized: MembershipProof = borsh::from_slice(&serialized).unwrap();
    
    assert_eq!(proof.leaf, deserialized.leaf);
    assert!(deserialized.merkle_proof.siblings.is_empty());
    assert!(deserialized.merkle_proof.path_indices.is_empty());
}

#[test]
fn test_batch_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert multiple keys
    let keys = vec![10, 20, 30, 40, 50];
    for k in &keys {
        let key = Field::from_u32(*k);
        let value = Field::from_u32(*k * 10);
        map.insert(key, value).unwrap();
    }
    
    // Collect all proofs
    let proofs: Vec<MembershipProof> = keys
        .iter()
        .map(|k| {
            let key = Field::from_u32(*k);
            map.get_membership_proof(&key).unwrap()
        })
        .collect();
    
    // Serialize batch
    let serialized = borsh::to_vec(&proofs).unwrap();
    
    // Deserialize batch
    let deserialized: Vec<MembershipProof> = borsh::from_slice(&serialized).unwrap();
    
    assert_eq!(proofs.len(), deserialized.len());
    
    // Verify all deserialized proofs
    let root = map.root();
    for (k, proof) in keys.iter().zip(&deserialized) {
        let key = Field::from_u32(*k);
        let value = Field::from_u32(*k * 10);
        assert!(IndexedMerkleMap::verify_membership_proof(&root, proof, &key, &value, map.length()));
    }
}

// ============ Tests for Missing Structs ============

#[test]
fn test_merkle_proof_serialization() {
    // Create a MerkleProof directly
    let merkle_proof = MerkleProof {
        siblings: vec![
            Hash::new([1u8; 32]),
            Hash::new([2u8; 32]),
            Hash::new([3u8; 32]),
        ],
        path_indices: vec![true, false, true],
    };
    
    // Serialize
    let serialized = borsh::to_vec(&merkle_proof).unwrap();
    
    // Deserialize
    let deserialized: MerkleProof = borsh::from_slice(&serialized).unwrap();
    
    // Verify all fields match
    assert_eq!(merkle_proof.siblings.len(), deserialized.siblings.len());
    for (orig, deser) in merkle_proof.siblings.iter().zip(&deserialized.siblings) {
        assert_eq!(orig, deser);
    }
    assert_eq!(merkle_proof.path_indices, deserialized.path_indices);
}

#[test]
fn test_insert_witness_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some initial data
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Generate witness for new insertion
    let new_key = Field::from_u32(20);
    let new_value = Field::from_u32(200);
    let witness = map.insert_and_generate_witness(new_key, new_value, true)
        .unwrap()
        .expect("Witness generation should succeed");
    
    // Serialize
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize
    let deserialized: InsertWitness = borsh::from_slice(&serialized).unwrap();
    
    // Verify all fields match
    assert_eq!(witness.old_root, deserialized.old_root);
    assert_eq!(witness.new_root, deserialized.new_root);
    assert_eq!(witness.key, deserialized.key);
    assert_eq!(witness.value, deserialized.value);
    assert_eq!(witness.new_leaf_index, deserialized.new_leaf_index);
    assert_eq!(witness.tree_length, deserialized.tree_length);
    assert_eq!(witness.non_membership_proof.low_leaf, deserialized.non_membership_proof.low_leaf);
    assert_eq!(witness.low_leaf_proof_before.siblings.len(), deserialized.low_leaf_proof_before.siblings.len());
    assert_eq!(witness.updated_low_leaf, deserialized.updated_low_leaf);
    assert_eq!(witness.new_leaf, deserialized.new_leaf);
    assert_eq!(witness.new_leaf_proof_after.siblings.len(), deserialized.new_leaf_proof_after.siblings.len());
    
    // Verify deserialized witness still works for verification
    assert!(IndexedMerkleMap::verify_insert(&deserialized).is_ok());
}

#[test]
fn test_update_witness_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert data to update
    let key = Field::from_u32(100);
    let old_value = Field::from_u32(200);
    let new_value = Field::from_u32(300);
    
    map.insert(key, old_value).unwrap();
    
    // Generate update witness
    let witness = map.update_and_generate_witness(key, new_value, true)
        .unwrap()
        .expect("Update witness generation should succeed");
    
    // Serialize
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize
    let deserialized: UpdateWitness = borsh::from_slice(&serialized).unwrap();
    
    // Verify all fields match
    assert_eq!(witness.old_root, deserialized.old_root);
    assert_eq!(witness.new_root, deserialized.new_root);
    assert_eq!(witness.key, deserialized.key);
    assert_eq!(witness.old_value, deserialized.old_value);
    assert_eq!(witness.new_value, deserialized.new_value);
    assert_eq!(witness.updated_leaf, deserialized.updated_leaf);
    assert_eq!(witness.membership_proof.leaf, deserialized.membership_proof.leaf);
    
    // Verify deserialized witness still works for verification
    assert!(IndexedMerkleMap::verify_update(&deserialized).is_ok());
}

#[test]
fn test_large_insert_witness_serialization() {
    let mut map = IndexedMerkleMap::new(15); // Larger tree
    
    // Insert many keys to create a complex tree
    for i in 1..=50 {
        map.insert(Field::from_u32(i * 10), Field::from_u32(i * 100)).unwrap();
    }
    
    // Generate witness for insertion
    let new_key = Field::from_u32(255);
    let new_value = Field::from_u32(2550);
    let witness = map.insert_and_generate_witness(new_key, new_value, true)
        .unwrap()
        .expect("Witness generation should succeed");
    
    // Serialize
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Check size is reasonable (should be much less than the full tree)
    assert!(serialized.len() < 5000, "Witness size too large: {} bytes", serialized.len());
    
    // Deserialize and verify
    let deserialized: InsertWitness = borsh::from_slice(&serialized).unwrap();
    assert!(IndexedMerkleMap::verify_insert(&deserialized).is_ok());
}

#[test]
fn test_witness_batch_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Create multiple witnesses
    let mut witnesses = Vec::new();
    
    // Insert initial keys
    for i in 1..=3 {
        map.insert(Field::from_u32(i * 20), Field::from_u32(i * 200)).unwrap();
    }
    
    // Generate update witnesses
    for i in 1..=3 {
        let key = Field::from_u32(i * 20);
        let new_value = Field::from_u32(i * 200 + 1000);
        let witness = map.update_and_generate_witness(key, new_value, true)
            .unwrap()
            .expect("Update witness generation should succeed");
        witnesses.push(witness);
    }
    
    // Serialize the batch
    let serialized = borsh::to_vec(&witnesses).unwrap();
    
    // Deserialize the batch
    let deserialized: Vec<UpdateWitness> = borsh::from_slice(&serialized).unwrap();
    
    assert_eq!(witnesses.len(), deserialized.len());
    
    // Verify all deserialized witnesses
    for witness in &deserialized {
        assert!(IndexedMerkleMap::verify_update(witness).is_ok());
    }
}

#[test]
fn test_witness_serialization_consistency() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Setup
    let key = Field::from_u32(42);
    let value = Field::from_u32(420);
    map.insert(key, value).unwrap();
    
    // Generate witness
    let new_value = Field::from_u32(840);
    let witness = map.update_and_generate_witness(key, new_value, true)
        .unwrap()
        .expect("Witness generation should succeed");
    
    // Serialize and deserialize multiple times
    let serialized1 = borsh::to_vec(&witness).unwrap();
    let deserialized1: UpdateWitness = borsh::from_slice(&serialized1).unwrap();
    let serialized2 = borsh::to_vec(&deserialized1).unwrap();
    let deserialized2: UpdateWitness = borsh::from_slice(&serialized2).unwrap();
    
    // All serializations should be identical
    assert_eq!(serialized1, serialized2);
    
    // Both deserializations should work
    assert!(IndexedMerkleMap::verify_update(&deserialized1).is_ok());
    assert!(IndexedMerkleMap::verify_update(&deserialized2).is_ok());
}

#[test]
fn test_empty_tree_witness_serialization() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Generate witness for first insertion (into empty tree except zero leaf)
    let key = Field::from_u32(100);
    let value = Field::from_u32(1000);
    let witness = map.insert_and_generate_witness(key, value, true)
        .unwrap()
        .expect("Witness generation should succeed");
    
    // Serialize
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize
    let deserialized: InsertWitness = borsh::from_slice(&serialized).unwrap();
    
    // Verify witness for insertion into nearly empty tree
    assert!(IndexedMerkleMap::verify_insert(&deserialized).is_ok());
    assert_eq!(deserialized.tree_length, 1); // Only zero leaf before insertion
}

#[test]
fn test_max_values_witness_serialization() {
    use crypto_bigint::U256;
    
    let mut map = IndexedMerkleMap::new(10);
    
    // Use maximum field values
    let max_key = Field::from_u256(U256::MAX);
    let max_value = Field::from_u256(U256::from_u128(u128::MAX));
    
    map.insert(max_key, max_value).unwrap();
    
    // Generate update witness with max values
    let new_max_value = Field::from_u256(U256::from_u64(u64::MAX));
    let witness = map.update_and_generate_witness(max_key, new_max_value, true)
        .unwrap()
        .expect("Witness generation should succeed");
    
    // Serialize
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize
    let deserialized: UpdateWitness = borsh::from_slice(&serialized).unwrap();
    
    // Verify max values are preserved
    assert_eq!(deserialized.key, max_key);
    assert_eq!(deserialized.old_value, max_value);
    assert_eq!(deserialized.new_value, new_max_value);
    
    // Verify witness still works
    assert!(IndexedMerkleMap::verify_update(&deserialized).is_ok());
}