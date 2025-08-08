use indexed_merkle_map::{Field, Hash, IndexedMerkleMap, ProvableIndexedMerkleMap, UpdateWitness, InsertWitness};

#[test]
fn test_insert_witness_path_index_consistency() {
    // This test verifies that the path-index consistency checks are enforced
    // This addresses the HIGH severity issue from the audit
    let mut map = IndexedMerkleMap::new(4); // Small tree for easier testing
    
    // Insert initial keys
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Generate a valid witness for inserting key 20
    let witness = map.insert_and_generate_witness(Field::from_u32(20), Field::from_u32(200), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // First verify the witness is valid
    assert!(ProvableIndexedMerkleMap::insert(&witness).is_ok());
    
    // Now tamper with the path indices to create inconsistency
    // Create a witness where the new_leaf_proof_after path doesn't match new_leaf_index
    let mut bad_witness = witness.clone();
    
    // Flip a bit in the path to make it point to a different index
    if !bad_witness.new_leaf_proof_after.path_indices.is_empty() {
        bad_witness.new_leaf_proof_after.path_indices[0] = !bad_witness.new_leaf_proof_after.path_indices[0];
    }
    
    // This should now fail due to path-index inconsistency
    let result = ProvableIndexedMerkleMap::insert(&bad_witness);
    assert!(result.is_err(), "Should reject witness with inconsistent new leaf path indices");
    
    // Similarly test for low_leaf_proof_before path inconsistency
    let mut bad_witness2 = witness.clone();
    if !bad_witness2.low_leaf_proof_before.path_indices.is_empty() {
        bad_witness2.low_leaf_proof_before.path_indices[0] = !bad_witness2.low_leaf_proof_before.path_indices[0];
    }
    
    let result2 = ProvableIndexedMerkleMap::insert(&bad_witness2);
    assert!(result2.is_err(), "Should reject witness with inconsistent low leaf path indices");
}

#[test]
fn test_insert_witness_wrong_index_claim() {
    // Test that claiming a wrong index (but with correct path) is also rejected
    let mut map = IndexedMerkleMap::new(4);
    
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    
    let witness = map.insert_and_generate_witness(Field::from_u32(20), Field::from_u32(200), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Tamper with the claimed index (but keep path consistent with original)
    let mut bad_witness = witness.clone();
    bad_witness.new_leaf_index = witness.new_leaf_index + 1; // Wrong index claim
    
    // This should fail because new_leaf_index != tree_length
    let result = ProvableIndexedMerkleMap::insert(&bad_witness);
    assert!(result.is_err(), "Should reject witness with wrong new_leaf_index");
}

#[test]
fn test_update_with_witness() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert initial value
    let key = Field::from_u32(100);
    let old_value = Field::from_u32(200);
    map.insert(key, old_value).unwrap();
    
    // Generate update witness
    let new_value = Field::from_u32(300);
    let witness = map.update_and_generate_witness(key, new_value, true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Verify using the witness (simulating zkVM usage)
    let result = ProvableIndexedMerkleMap::update(&witness);
    assert!(result.is_ok());
    
    // Verify the new root matches
    assert_eq!(witness.new_root, map.root());
    
    // Verify the updated leaf
    assert_eq!(witness.updated_leaf.key, key);
    assert_eq!(witness.updated_leaf.value, new_value);
    assert_eq!(witness.old_value, old_value);
}

#[test]
fn test_update_witness_invalid_key() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    // Generate valid witness
    let witness = map.update_and_generate_witness(key, Field::from_u32(300), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Tamper with the key
    let mut bad_witness = witness.clone();
    bad_witness.key = Field::from_u32(101);
    
    // Verification should fail
    let result = ProvableIndexedMerkleMap::update(&bad_witness);
    assert!(result.is_err());
}

#[test]
fn test_update_witness_invalid_value() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    // Generate valid witness
    let witness = map.update_and_generate_witness(key, Field::from_u32(300), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Tamper with the old value
    let mut bad_witness = witness.clone();
    bad_witness.old_value = Field::from_u32(201);
    
    // Verification should fail
    let result = ProvableIndexedMerkleMap::update(&bad_witness);
    assert!(result.is_err());
}

#[test]
fn test_update_witness_invalid_root() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    
    // Generate valid witness
    let witness = map.update_and_generate_witness(key, Field::from_u32(300), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Tamper with the old root
    let mut bad_witness = witness.clone();
    bad_witness.old_root = Hash::zero();
    
    // Verification should fail
    let result = ProvableIndexedMerkleMap::update(&bad_witness);
    assert!(result.is_err());
}

#[test]
fn test_multiple_updates_with_witness() {
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
    
    // Update each key using witnesses
    for (i, (key, _old_value)) in keys.iter().enumerate() {
        let new_value = Field::from_u32((i as u32 + 1) * 100 + 1000);
        
        // Generate update witness
        let witness = map.update_and_generate_witness(*key, new_value, true)
            .unwrap()
            .expect("Witness should be generated");
        
        // Verify with witness
        let result = ProvableIndexedMerkleMap::update(&witness);
        assert!(result.is_ok());
        
        // Verify the new root matches
        assert_eq!(witness.new_root, map.root());
    }
}

#[test]
fn test_serialized_update_witness() {
    let mut map = IndexedMerkleMap::new(10);
    
    let key = Field::from_u32(100);
    let old_value = Field::from_u32(200);
    map.insert(key, old_value).unwrap();
    
    // Generate update witness
    let new_value = Field::from_u32(300);
    let witness = map.update_and_generate_witness(key, new_value, true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Serialize the witness (simulating sending to zkVM)
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize (simulating receiving in zkVM)
    let deserialized: UpdateWitness = borsh::from_slice(&serialized).unwrap();
    
    // Verify with deserialized witness
    let result = ProvableIndexedMerkleMap::update(&deserialized);
    assert!(result.is_ok());
    
    // Verify the new root matches
    assert_eq!(deserialized.new_root, map.root());
}

#[test]
fn test_insert_with_witness() {
    // Test the new witness-based insertion approach
    let mut map = IndexedMerkleMap::new(4); // Small tree for testing
    
    // Insert initial key to have a non-empty tree
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    
    let before_insert = map.root();
    
    // Prepare to insert key 20 and generate witness
    let new_key = Field::from_u32(20);
    let new_value = Field::from_u32(200);
    
    // Generate witness for the insertion
    let witness = map.insert_and_generate_witness(new_key, new_value, true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Verify the witness is valid
    let result = ProvableIndexedMerkleMap::insert(&witness);
    if let Err(e) = &result {
        println!("Witness verification failed: {}", e);
        println!("Witness details:");
        println!("  old_root: {:?}", witness.old_root);
        println!("  new_root: {:?}", witness.new_root);
        println!("  key: {:?}", witness.key);
        println!("  value: {:?}", witness.value);
        println!("  new_leaf_index: {}", witness.new_leaf_index);
        println!("  tree_length: {}", witness.tree_length);
    }
    assert!(result.is_ok(), "Witness verification should succeed");
    
    // Verify old and new roots
    assert_eq!(witness.old_root, before_insert);
    assert_eq!(witness.new_root, map.root());
    
    // Verify the witness contains correct data
    assert_eq!(witness.key, new_key);
    assert_eq!(witness.value, new_value);
    assert_eq!(witness.new_leaf_index, 2); // After index 0 for empty leaf, index 1 for key 10
    assert_eq!(witness.tree_length, 2); // Before insertion
}

#[test]
fn test_zkvm_workflow_simulation() {
    // This test simulates a complete zkVM workflow:
    // 1. Off-chain: prepare witnesses for insertions and updates
    // 2. zkVM: verify witnesses and update values using only proofs
    // 3. Off-chain: verify results
    
    let mut map = IndexedMerkleMap::new(10);
    
    // Setup: Insert some initial data
    let entries = vec![
        (Field::from_u32(10), Field::from_u32(100)),
        (Field::from_u32(30), Field::from_u32(300)),
        (Field::from_u32(50), Field::from_u32(500)),
    ];
    
    for (key, value) in &entries {
        map.insert(*key, *value).unwrap();
    }
    
    // === Test 1: Update operation ===
    let key_to_update = Field::from_u32(30);
    let new_value = Field::from_u32(999);
    
    // Generate update witness
    let update_witness = map.update_and_generate_witness(key_to_update, new_value, true)
        .unwrap()
        .expect("Update witness should be generated");
    
    // Serialize witness to send to zkVM
    let serialized_update = borsh::to_vec(&update_witness).unwrap();
    
    // zkVM - Deserialize and verify witness
    let zkvm_update_witness: UpdateWitness = borsh::from_slice(&serialized_update).unwrap();
    let zkvm_update_result = ProvableIndexedMerkleMap::update(&zkvm_update_witness);
    
    assert!(zkvm_update_result.is_ok(), "Update witness verification should succeed");
    assert_eq!(zkvm_update_witness.new_root, map.root());
    
    // === Test 2: Insert operation with witness ===
    let insert_key = Field::from_u32(40);
    let insert_value = Field::from_u32(400);
    
    // Generate witness for insertion
    let insert_witness = map.insert_and_generate_witness(insert_key, insert_value, true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Serialize witness to send to zkVM
    let serialized_witness = borsh::to_vec(&insert_witness).unwrap();
    
    // zkVM - Deserialize and verify witness
    let deserialized_witness = borsh::from_slice(&serialized_witness).unwrap();
    let zkvm_insert_result = ProvableIndexedMerkleMap::insert(&deserialized_witness);
    
    assert!(zkvm_insert_result.is_ok(), "Insert witness verification should succeed");
    
    // Verify the insertion produced the expected new root
    assert_eq!(insert_witness.new_root, map.root());
    
    println!("zkVM workflow simulation successful!");
    println!("Update - Old root: {:?}", update_witness.old_root);
    println!("Update - New root: {:?}", update_witness.new_root);
    println!("Insert - New root: {:?}", insert_witness.new_root);
}

#[test]
fn test_witness_verification_edge_cases() {
    let mut map = IndexedMerkleMap::new(4);
    
    // Insert initial keys
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    // Generate valid witness
    let witness = map.insert_and_generate_witness(Field::from_u32(20), Field::from_u32(200), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Test 1: Valid witness should pass
    assert!(ProvableIndexedMerkleMap::insert(&witness).is_ok());
    
    // Test 2: Wrong new_leaf_index should fail
    let mut bad_witness = witness.clone();
    bad_witness.new_leaf_index = 10;
    assert!(ProvableIndexedMerkleMap::insert(&bad_witness).is_err());
    
    // Test 3: Wrong tree_length should fail
    let mut bad_witness = witness.clone();
    bad_witness.tree_length = 100;
    assert!(ProvableIndexedMerkleMap::insert(&bad_witness).is_err());
    
    // Test 4: Wrong key ordering should fail
    let mut bad_witness = witness.clone();
    bad_witness.key = Field::from_u32(5); // Less than low_leaf key (10)
    assert!(ProvableIndexedMerkleMap::insert(&bad_witness).is_err());
    
    // Test 5: Wrong updated_low_leaf next_key should fail
    let mut bad_witness = witness.clone();
    bad_witness.updated_low_leaf.next_key = Field::from_u32(999);
    assert!(ProvableIndexedMerkleMap::insert(&bad_witness).is_err());
    
    // Test 6: Wrong new_leaf structure should fail
    let mut bad_witness = witness.clone();
    bad_witness.new_leaf.key = Field::from_u32(999);
    assert!(ProvableIndexedMerkleMap::insert(&bad_witness).is_err());
}

#[test]
fn test_insert_without_witness() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert without generating witness (more efficient)
    let result = map.insert_and_generate_witness(Field::from_u32(10), Field::from_u32(100), false);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none(), "No witness should be generated when flag is false");
    
    // Verify the insertion worked
    let proof = map.get_membership_proof(&Field::from_u32(10)).unwrap();
    assert_eq!(proof.leaf.value, Field::from_u32(100));
}

#[test]
fn test_serialized_witness_verification() {
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some data
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    
    // Generate witness
    let witness = map.insert_and_generate_witness(Field::from_u32(20), Field::from_u32(200), true)
        .unwrap()
        .expect("Witness should be generated");
    
    // Serialize witness
    let serialized = borsh::to_vec(&witness).unwrap();
    
    // Deserialize and verify
    let deserialized = borsh::from_slice(&serialized).unwrap();
    assert!(ProvableIndexedMerkleMap::insert(&deserialized).is_ok());
}

#[test]
fn test_random_1000_transactions_zkvm_emulation() {
    use rand::{Rng, SeedableRng};
    use rand::rngs::StdRng;
    use std::collections::HashMap;
    use std::time::Instant;
    
    println!("\n=== Random 1000 Transactions zkVM Emulation Test ===\n");
    
    // Use a fixed seed for reproducibility
    let mut rng = StdRng::seed_from_u64(42);
    
    // Track timing for different phases
    let total_start = Instant::now();
    
    // ===== PHASE 1: Generate Random Operations =====
    let phase1_start = Instant::now();
    println!("PHASE 1: Generating random operations...");
    
    // Generate random keys with some repetition to trigger updates
    let num_unique_keys = 500; // Use 500 unique keys to ensure some repetition
    let mut operations = Vec::with_capacity(1000);
    let mut key_tracker = HashMap::new();
    
    for i in 0..1000 {
        let key = Field::from_u32(rng.random_range(1..=num_unique_keys));
        let value = Field::from_u32(rng.random_range(1..=100000));
        
        // Track if this is an insert or update
        let is_update = key_tracker.contains_key(&key);
        key_tracker.insert(key, value);
        
        operations.push((i, key, value, is_update));
    }
    
    let num_inserts = operations.iter().filter(|(_, _, _, is_update)| !is_update).count();
    let num_updates = operations.iter().filter(|(_, _, _, is_update)| *is_update).count();
    
    println!("  Generated {} operations: {} inserts, {} updates", 
        operations.len(), num_inserts, num_updates);
    println!("  Phase 1 time: {:?}\n", phase1_start.elapsed());
    
    // ===== PHASE 2: Execute Operations & Generate Witnesses =====
    let phase2_start = Instant::now();
    println!("PHASE 2: Executing operations and generating witnesses...");
    
    let mut map = IndexedMerkleMap::new(14); // Height 14 for 16K entries, enough for our test
    let mut witnesses = Vec::with_capacity(1000);
    let mut current_keys = HashMap::new();
    
    let witness_gen_start = Instant::now();
    
    for (tx_id, key, value, _) in &operations {
        // Check if this is an update or insert
        if let Some(&_old_value) = current_keys.get(key) {
            // Update operation
            let witness = map.update_and_generate_witness(*key, *value, true)
                .unwrap()
                .expect("Update witness generation failed");
            
            // Serialize the witness as a transaction
            let serialized_tx = borsh::to_vec(&witness).unwrap();
            witnesses.push((*tx_id, serialized_tx, true)); // true = update
            
            current_keys.insert(*key, *value);
        } else {
            // Insert operation
            let witness = map.insert_and_generate_witness(*key, *value, true)
                .unwrap()
                .expect("Insert witness generation failed");
            
            // Serialize the witness as a transaction
            let serialized_tx = borsh::to_vec(&witness).unwrap();
            witnesses.push((*tx_id, serialized_tx, false)); // false = insert
            
            current_keys.insert(*key, *value);
        }
        
        // Progress reporting every 100 operations
        if (tx_id + 1) % 100 == 0 {
            println!("  Processed {} operations...", tx_id + 1);
        }
    }
    
    let witness_gen_time = witness_gen_start.elapsed();
    let avg_witness_size = witnesses.iter()
        .map(|(_, tx, _)| tx.len())
        .sum::<usize>() / witnesses.len();
    
    println!("  Generated {} witnesses", witnesses.len());
    println!("  Average witness size: {} bytes", avg_witness_size);
    println!("  Witness generation time: {:?}", witness_gen_time);
    println!("  Phase 2 total time: {:?}\n", phase2_start.elapsed());
    
    // Store final root for verification
    let final_root_expected = map.root();
    
    // ===== PHASE 3: zkVM Emulation - Verify All Transactions =====
    let phase3_start = Instant::now();
    println!("PHASE 3: zkVM emulation - verifying all transactions...");
    println!("  (Using only ProvableIndexedMerkleMap with serialized witnesses)");
    
    let verification_start = Instant::now();
    let mut verified_count = 0;
    let mut insert_count = 0;
    let mut update_count = 0;
    let mut last_root = None;
    
    for (tx_id, serialized_tx, is_update) in &witnesses {
        if *is_update {
            // Deserialize as UpdateWitness
            let witness: UpdateWitness = borsh::from_slice(serialized_tx)
                .expect("Failed to deserialize update witness");
            
            // Verify the chain of roots
            if let Some(expected_old_root) = last_root {
                assert_eq!(witness.old_root, expected_old_root, 
                    "Transaction {} root mismatch", tx_id);
            }
            
            // Verify using only ProvableIndexedMerkleMap (zkVM simulation)
            ProvableIndexedMerkleMap::update(&witness)
                .expect(&format!("Update verification failed for transaction {}", tx_id));
            
            last_root = Some(witness.new_root);
            update_count += 1;
        } else {
            // Deserialize as InsertWitness
            let witness: InsertWitness = borsh::from_slice(serialized_tx)
                .expect("Failed to deserialize insert witness");
            
            // Verify the chain of roots
            if let Some(expected_old_root) = last_root {
                assert_eq!(witness.old_root, expected_old_root, 
                    "Transaction {} root mismatch", tx_id);
            }
            
            // Verify using only ProvableIndexedMerkleMap (zkVM simulation)
            ProvableIndexedMerkleMap::insert(&witness)
                .expect(&format!("Insert verification failed for transaction {}", tx_id));
            
            last_root = Some(witness.new_root);
            insert_count += 1;
        }
        
        verified_count += 1;
        
        // Progress reporting every 100 verifications
        if verified_count % 100 == 0 {
            println!("  Verified {} transactions...", verified_count);
        }
    }
    
    let verification_time = verification_start.elapsed();
    
    println!("  Verified all {} transactions successfully!", verified_count);
    println!("  {} inserts, {} updates", insert_count, update_count);
    println!("  Verification time: {:?}", verification_time);
    println!("  Phase 3 total time: {:?}\n", phase3_start.elapsed());
    
    // ===== PHASE 4: Final Verification =====
    println!("PHASE 4: Final verification...");
    
    // Verify the final root matches
    assert_eq!(last_root.unwrap(), final_root_expected, 
        "Final root mismatch after all transactions");
    
    println!("  ✓ Final root matches expected value");
    println!("  ✓ All transactions verified successfully in zkVM emulation\n");
    
    // ===== Performance Summary =====
    let total_time = total_start.elapsed();
    
    println!("=== Performance Summary ===");
    println!("Total transactions: 1000");
    println!("  - Inserts: {}", num_inserts);
    println!("  - Updates: {}", num_updates);
    println!("\nTiming breakdown:");
    println!("  Phase 1 (Generation):     {:?}", phase1_start.elapsed());
    println!("  Phase 2 (Witness Gen):    {:?}", phase2_start.elapsed());  
    println!("  Phase 3 (Verification):   {:?}", phase3_start.elapsed());
    println!("  Total time:               {:?}", total_time);
    println!("\nAverage times per transaction:");
    println!("  Witness generation:       {:?}", witness_gen_time / 1000);
    println!("  zkVM verification:        {:?}", verification_time / 1000);
    println!("\nData sizes:");
    println!("  Average witness size:     {} bytes", avg_witness_size);
    println!("  Total serialized data:    {} KB", 
        witnesses.iter().map(|(_, tx, _)| tx.len()).sum::<usize>() / 1024);
}