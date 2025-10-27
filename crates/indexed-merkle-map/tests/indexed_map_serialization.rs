use indexed_merkle_map::{Field, IndexedMerkleMap};
use rand::Rng;

const NUMBER_OF_ITERATIONS: usize = 10;

#[test]
fn test_serialize_and_deserialize_indexed_map_with_random_data() {
    let mut rng = rand::rng();

    for iteration in 0..NUMBER_OF_ITERATIONS {
        // Generate a random IndexedMerkleMap with random data
        let original_map = random_indexed_map(&mut rng, 20, 10, 50);

        // Serialize the map
        let serialized = borsh::to_vec(&original_map).unwrap();

        // Verify serialization succeeded
        assert!(
            !serialized.is_empty(),
            "Serialized data should not be empty for iteration {}",
            iteration + 1
        );

        // Deserialize the map
        let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

        // Verify the deserialized map matches the original
        assert_eq!(
            deserialized_map.root(),
            original_map.root(),
            "Root should match for iteration {}",
            iteration + 1
        );
        assert_eq!(
            deserialized_map.length(),
            original_map.length(),
            "Length should match for iteration {}",
            iteration + 1
        );
        assert_eq!(
            deserialized_map.height(),
            original_map.height(),
            "Height should match for iteration {}",
            iteration + 1
        );

        // Verify that all keys from original map exist and have correct values in deserialized map
        for leaf in original_map.sorted_leaves() {
            let value = deserialized_map.get_option(&leaf.key);
            assert!(
                value.is_some(),
                "Key should exist in deserialized map for iteration {}",
                iteration + 1
            );
            assert_eq!(
                value.unwrap(),
                leaf.value,
                "Value should match for key in iteration {}",
                iteration + 1
            );
        }
    }
}

#[test]
fn test_serialize_and_deserialize_empty_indexed_map() {
    // Create an empty map (only has the zero leaf)
    let empty_map = IndexedMerkleMap::new(20);

    // Serialize the map
    let serialized = borsh::to_vec(&empty_map).unwrap();

    // Deserialize the map
    let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

    // Verify deserialized map matches empty map
    assert_eq!(
        deserialized_map.root(),
        empty_map.root(),
        "Empty map root should match"
    );
    assert_eq!(
        deserialized_map.length(),
        empty_map.length(),
        "Empty map length should match"
    );
    assert_eq!(
        deserialized_map.height(),
        empty_map.height(),
        "Empty map height should match"
    );
}

#[test]
fn test_deserialized_map_operations() {
    let mut rng = rand::rng();
    let mut original_map = IndexedMerkleMap::new(15);

    // Insert some data
    let keys = vec![
        Field::from_u32(100),
        Field::from_u32(200),
        Field::from_u32(300),
        Field::from_u32(400),
        Field::from_u32(500),
    ];

    for &key in &keys {
        let value = Field::from_u32(rng.random::<u32>());
        original_map.insert(key, value).unwrap();
    }

    // Serialize and deserialize
    let serialized = borsh::to_vec(&original_map).unwrap();
    let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

    // Test get operations on deserialized map
    for &key in &keys {
        let original_value = original_map.get_option(&key).unwrap();
        let deserialized_value = deserialized_map.get_option(&key).unwrap();
        assert_eq!(original_value, deserialized_value);
    }

    // Test membership proofs on deserialized map
    for &key in &keys {
        let value = deserialized_map.get_option(&key).unwrap();
        let proof = deserialized_map.get_membership_proof(&key).unwrap();
        let root = deserialized_map.root();
        assert!(
            IndexedMerkleMap::verify_membership_proof(
                &root,
                &proof,
                &key,
                &value,
                deserialized_map.length()
            ),
            "Membership proof should verify on deserialized map"
        );
    }

    // Test non-membership proofs on deserialized map
    let non_existent_key = Field::from_u32(150);
    let proof = deserialized_map.get_non_membership_proof(&non_existent_key).unwrap();
    let root = deserialized_map.root();
    assert!(
        IndexedMerkleMap::verify_non_membership_proof(
            &root,
            &proof,
            &non_existent_key,
            deserialized_map.length()
        ),
        "Non-membership proof should verify on deserialized map"
    );
}

#[test]
fn test_serialization_consistency() {
    let mut rng = rand::rng();
    let original_map = random_indexed_map(&mut rng, 10, 5, 20);

    // Serialize and deserialize multiple times
    let serialized1 = borsh::to_vec(&original_map).unwrap();
    let deserialized1: IndexedMerkleMap = borsh::from_slice(&serialized1).unwrap();
    let serialized2 = borsh::to_vec(&deserialized1).unwrap();
    let deserialized2: IndexedMerkleMap = borsh::from_slice(&serialized2).unwrap();

    // All serializations should be identical
    assert_eq!(serialized1, serialized2, "Serializations should be identical");

    // All roots should match
    assert_eq!(
        original_map.root(),
        deserialized1.root(),
        "First deserialization root should match"
    );
    assert_eq!(
        deserialized1.root(),
        deserialized2.root(),
        "Second deserialization root should match"
    );
}

#[test]
fn test_large_tree_serialization() {
    let mut map = IndexedMerkleMap::new(20);

    // Insert many keys to create a large tree
    for i in 1..=100 {
        let key = Field::from_u32(i * 10);
        let value = Field::from_u32(i * 100);
        map.insert(key, value).unwrap();
    }

    let original_root = map.root();
    let original_length = map.length();

    // Serialize
    let serialized = borsh::to_vec(&map).unwrap();

    // Check size is reasonable
    println!(
        "Large tree (100 entries, height 20) serialized to {} bytes",
        serialized.len()
    );

    // Deserialize
    let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

    // Verify
    assert_eq!(deserialized_map.root(), original_root);
    assert_eq!(deserialized_map.length(), original_length);

    // Spot check some values
    for i in [1, 25, 50, 75, 100] {
        let key = Field::from_u32(i * 10);
        let value = deserialized_map.get_option(&key).unwrap();
        assert_eq!(value, Field::from_u32(i * 100));
    }
}

#[test]
fn test_serialization_size_scaling() {
    println!("\nSerialization size scaling test:");

    for num_entries in [1, 10, 50, 100] {
        let mut map = IndexedMerkleMap::new(15);

        // Insert entries
        for i in 1..=num_entries {
            let key = Field::from_u32(i * 10);
            let value = Field::from_u32(i * 100);
            map.insert(key, value).unwrap();
        }

        // Serialize
        let serialized = borsh::to_vec(&map).unwrap();

        println!(
            "{:3} entries: {} bytes ({:.2} bytes/entry)",
            num_entries,
            serialized.len(),
            serialized.len() as f64 / num_entries as f64
        );
    }
}

#[test]
fn test_corrupted_data_deserialization() {
    let map = IndexedMerkleMap::new(10);
    let mut serialized = borsh::to_vec(&map).unwrap();

    // Corrupt the data by truncating it
    serialized.truncate(serialized.len() / 2);

    // Deserialization should fail
    let result: Result<IndexedMerkleMap, _> = borsh::from_slice(&serialized);
    assert!(result.is_err(), "Corrupted data should fail deserialization");
}

#[test]
fn test_map_with_max_field_values() {
    use crypto_bigint::U256;

    let mut map = IndexedMerkleMap::new(10);

    // Insert with large field values
    let max_key = Field::from_u256(U256::MAX);
    let max_value = Field::from_u256(U256::from_u128(u128::MAX));

    map.insert(max_key, max_value).unwrap();

    // Serialize and deserialize
    let serialized = borsh::to_vec(&map).unwrap();
    let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

    // Verify max values are preserved
    let retrieved_value = deserialized_map.get_option(&max_key).unwrap();
    assert_eq!(retrieved_value, max_value, "Max field values should be preserved");

    // Verify root matches
    assert_eq!(deserialized_map.root(), map.root());
}

#[test]
fn test_various_tree_heights() {
    for height in [5, 10, 15, 20, 25] {
        let mut map = IndexedMerkleMap::new(height);

        // Insert a few entries
        for i in 1..=5 {
            map.insert(Field::from_u32(i * 100), Field::from_u32(i * 1000)).unwrap();
        }

        let original_root = map.root();

        // Serialize and deserialize
        let serialized = borsh::to_vec(&map).unwrap();
        let deserialized_map: IndexedMerkleMap = borsh::from_slice(&serialized).unwrap();

        // Verify
        assert_eq!(deserialized_map.height(), height);
        assert_eq!(deserialized_map.root(), original_root);
    }
}

// ============ Helper Functions ============

/// Generates a random IndexedMerkleMap with random key-value pairs.
/// Similar to the TypeScript randomIndexedMap helper.
fn random_indexed_map<R: Rng>(
    rng: &mut R,
    height: usize,
    min_entries: usize,
    max_entries: usize,
) -> IndexedMerkleMap {
    let mut map = IndexedMerkleMap::new(height);

    let num_entries = rng.random_range(min_entries..=max_entries);

    // Generate random key-value pairs and add them to the map
    // We use incrementing keys to avoid collisions
    for i in 0..num_entries {
        let key = Field::from_u32(rng.random::<u32>() / (num_entries as u32 + 1) + i as u32 * 1000);
        let value = Field::from_u32(rng.random::<u32>());

        // Use set to handle any potential collisions gracefully
        let _ = map.set(key, value);
    }

    map
}
