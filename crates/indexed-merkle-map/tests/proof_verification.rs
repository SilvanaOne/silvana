use indexed_merkle_map::{Field, Hash, IndexedMerkleMap, Leaf, MembershipProof, MerkleProof};

#[test]
fn test_valid_membership_proof() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let proof = map.get_membership_proof(&key).unwrap();

    // Verify with correct parameters
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_membership_proof_wrong_key() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let proof = map.get_membership_proof(&key).unwrap();

    // Verify with wrong key
    let wrong_key = Field::from_u32(101);
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &wrong_key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_membership_proof_wrong_value() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let proof = map.get_membership_proof(&key).unwrap();

    // Verify with wrong value
    let wrong_value = Field::from_u32(201);
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &wrong_value,
        map.length()
    ));
}

#[test]
fn test_invalid_membership_proof_wrong_root() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let proof = map.get_membership_proof(&key).unwrap();

    // Use wrong root
    let wrong_root = Hash::zero();
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &wrong_root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_valid_non_membership_proof() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert some keys
    map.insert(Field::from_u32(10), Field::from_u32(100))
        .unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300))
        .unwrap();

    let root = map.root();

    // Get non-membership proof for key between existing keys
    let non_existent = Field::from_u32(20);
    let proof = map.get_non_membership_proof(&non_existent).unwrap();

    // Verify proof
    assert!(IndexedMerkleMap::verify_non_membership_proof(
        &root,
        &proof,
        &non_existent,
        map.length()
    ));
}

#[test]
fn test_invalid_non_membership_proof_existing_key() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(20);
    map.insert(key, Field::from_u32(200)).unwrap();

    let root = map.root();

    // Try to create a fake non-membership proof for existing key
    // Use the proof for a different non-existent key
    let non_existent = Field::from_u32(10);
    let proof = map.get_non_membership_proof(&non_existent).unwrap();

    // This should fail because key 20 exists
    assert!(!IndexedMerkleMap::verify_non_membership_proof(
        &root,
        &proof,
        &key,
        map.length()
    ));
}

#[test]
fn test_non_membership_proof_boundary_check() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert keys with gaps
    map.insert(Field::from_u32(10), Field::from_u32(100))
        .unwrap();
    map.insert(Field::from_u32(50), Field::from_u32(500))
        .unwrap();

    let root = map.root();

    // Test non-membership for various positions
    let test_cases = vec![
        (Field::from_u32(5), Field::zero()),         // Before first key
        (Field::from_u32(30), Field::from_u32(10)),  // Between keys
        (Field::from_u32(100), Field::from_u32(50)), // After last key
    ];

    for (non_existent, expected_low_key) in test_cases {
        let proof = map.get_non_membership_proof(&non_existent).unwrap();
        assert_eq!(proof.low_leaf.key, expected_low_key);
        assert!(IndexedMerkleMap::verify_non_membership_proof(
            &root,
            &proof,
            &non_existent,
            map.length()
        ));
    }
}

#[test]
fn test_proof_after_update() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value1 = Field::from_u32(200);
    let value2 = Field::from_u32(300);

    map.insert(key, value1).unwrap();

    // Get proof before update
    let proof_before = map.get_membership_proof(&key).unwrap();
    let root_before = map.root();

    // Update value
    map.update(key, value2).unwrap();

    // Get proof after update
    let proof_after = map.get_membership_proof(&key).unwrap();
    let root_after = map.root();

    // Old proof should be invalid with new root
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root_after,
        &proof_before,
        &key,
        &value1,
        map.length()
    ));

    // New proof should be valid with new root
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root_after,
        &proof_after,
        &key,
        &value2,
        map.length()
    ));

    // Old proof should still be valid with old root
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root_before,
        &proof_before,
        &key,
        &value1,
        map.length()
    ));
}

#[test]
fn test_tampered_merkle_proof() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();

    // Tamper with the merkle proof siblings
    if !proof.merkle_proof.siblings.is_empty() {
        proof.merkle_proof.siblings[0] = Hash::zero();
    }

    // Verification should fail
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_tampered_leaf_data() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();

    // Tamper with leaf data
    proof.leaf.next_key = Field::from_u32(999);

    // Verification should fail because leaf hash will be different
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_proof_path_indices() {
    let mut map = IndexedMerkleMap::new(4); // Small tree for testing

    // Insert multiple keys
    for i in 1..=4 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).unwrap();
    }

    // Get proof and verify path indices
    let key = Field::from_u32(30);
    let proof = map.get_membership_proof(&key).unwrap();

    // Path indices should match tree height - 1
    assert_eq!(proof.merkle_proof.path_indices.len(), 3);
    assert_eq!(proof.merkle_proof.siblings.len(), 3);
}

#[test]
fn test_cross_tree_proof_invalid() {
    let mut map1 = IndexedMerkleMap::new(10);
    let mut map2 = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    // Insert same key-value in both trees
    map1.insert(key, value).unwrap();
    map2.insert(key, value).unwrap();

    // Also insert additional key in map2 to make roots different
    map2.insert(Field::from_u32(50), Field::from_u32(500))
        .unwrap();

    let root1 = map1.root();
    let root2 = map2.root();
    assert_ne!(root1, root2);

    // Get proof from map1
    let proof1 = map1.get_membership_proof(&key).unwrap();

    // Proof from map1 should not verify against map2's root
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root2,
        &proof1,
        &key,
        &value,
        map1.length()
    ));
}

#[test]
fn test_multiple_proofs_same_root() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert multiple keys
    let keys = vec![
        (Field::from_u32(10), Field::from_u32(100)),
        (Field::from_u32(20), Field::from_u32(200)),
        (Field::from_u32(30), Field::from_u32(300)),
    ];

    for (key, value) in &keys {
        map.insert(*key, *value).unwrap();
    }

    let root = map.root();

    // Get all proofs
    let proofs: Vec<_> = keys
        .iter()
        .map(|(key, _)| map.get_membership_proof(key).unwrap())
        .collect();

    // All proofs should verify against the same root
    for ((key, value), proof) in keys.iter().zip(&proofs) {
        assert!(IndexedMerkleMap::verify_membership_proof(
            &root,
            proof,
            key,
            value,
            map.length()
        ));
    }
}

#[test]
fn test_proof_serialization_round_trip() {
    let mut map = IndexedMerkleMap::new(10);

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    map.insert(key, value).unwrap();

    let root = map.root();
    let proof = map.get_membership_proof(&key).unwrap();

    // Serialize and deserialize proof using borsh
    let serialized = borsh::to_vec(&proof).unwrap();
    let deserialized: MembershipProof = borsh::from_slice(&serialized).unwrap();

    // Deserialized proof should still verify
    assert!(IndexedMerkleMap::verify_membership_proof(
        &root,
        &deserialized,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_proof_size_too_small() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert some data
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();

    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();

    // Truncate the proof to make it too small
    // A valid proof should have siblings matching the tree height
    if proof.merkle_proof.siblings.len() > 1 {
        proof.merkle_proof.siblings.truncate(1);
        proof.merkle_proof.path_indices.truncate(1);
    }

    // This proof with insufficient siblings should fail verification
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_proof_size_too_large() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert some data
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();

    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();

    // Add too many siblings to exceed the maximum tree height of 32
    while proof.merkle_proof.siblings.len() <= 33 {
        proof.merkle_proof.siblings.push(Hash::zero());
        proof.merkle_proof.path_indices.push(false);
    }

    // This proof with too many siblings should fail verification
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_proof_size_mismatched_arrays() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert some data
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();

    let root = map.root();
    let mut proof = map.get_membership_proof(&key).unwrap();

    // Create mismatch between siblings and path_indices arrays
    proof.merkle_proof.siblings.push(Hash::zero());
    // Don't add corresponding path_index

    // This proof with mismatched array sizes should fail verification
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &proof,
        &key,
        &value,
        map.length()
    ));
}

#[test]
fn test_invalid_non_membership_proof_size() {
    let mut map = IndexedMerkleMap::new(10);

    // Insert some data to create a non-trivial tree
    map.insert(Field::from_u32(10), Field::from_u32(100))
        .unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300))
        .unwrap();

    let root = map.root();
    let key = Field::from_u32(20); // Non-existent key between 10 and 30
    let mut proof = map.get_non_membership_proof(&key).unwrap();

    // Truncate the proof to make it invalid
    if proof.merkle_proof.siblings.len() > 1 {
        proof.merkle_proof.siblings.truncate(1);
        proof.merkle_proof.path_indices.truncate(1);
    }

    // This proof with insufficient siblings should fail verification
    assert!(!IndexedMerkleMap::verify_non_membership_proof(
        &root,
        &proof,
        &key,
        map.length()
    ));
}

#[test]
fn test_empty_proof_rejected() {
    let map = IndexedMerkleMap::new(10);
    let root = map.root();

    // Create an empty proof
    let empty_proof = MembershipProof {
        leaf: Leaf::empty(),
        merkle_proof: MerkleProof {
            siblings: vec![],
            path_indices: vec![],
        },
    };

    let key = Field::from_u32(100);
    let value = Field::from_u32(200);

    // Empty proof should fail verification
    assert!(!IndexedMerkleMap::verify_membership_proof(
        &root,
        &empty_proof,
        &key,
        &value,
        map.length()
    ));
}
