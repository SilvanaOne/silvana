//! zkVM witness generation and verification example
//! 
//! This example demonstrates the witness-based API for zkVM:
//! - Generating witnesses outside zkVM
//! - Serializing witnesses for transmission
//! - Verifying witnesses inside zkVM using static methods
//! - Both insert and update operations with witnesses

use indexed_merkle_map::{IndexedMerkleMap, ProvableIndexedMerkleMap, Field, InsertWitness, UpdateWitness};

fn main() {
    println!("IndexedMerkleMap zkVM Witness Example\n");
    println!("======================================\n");
    
    // === OUTSIDE zkVM: Generate witnesses ===
    println!("OUTSIDE zkVM: Generating witnesses");
    println!("-----------------------------------\n");
    
    // Create and populate a map
    let mut map = IndexedMerkleMap::new(10);
    println!("✓ Created IndexedMerkleMap with height 10");
    
    // Insert some initial data
    let initial_entries = vec![
        (Field::from_u32(10), Field::from_u32(100)),
        (Field::from_u32(20), Field::from_u32(200)),
        (Field::from_u32(30), Field::from_u32(300)),
    ];
    
    for (key, value) in &initial_entries {
        map.insert(*key, *value).unwrap();
        println!("  Inserted key={}, value={}", key.to_u256(), value.to_u256());
    }
    
    println!("\nInitial root: {:?}", map.root());
    
    // Generate witness for inserting a new key
    println!("\n1. Generating INSERT witness:");
    let new_key = Field::from_u32(25);
    let new_value = Field::from_u32(250);
    
    let insert_witness = map.insert_and_generate_witness(new_key, new_value, true)
        .unwrap()
        .expect("Witness generation failed");
    
    println!("  ✓ Generated witness for inserting key=25, value=250");
    println!("  Old root: {:?}", insert_witness.old_root);
    println!("  New root: {:?}", insert_witness.new_root);
    println!("  New leaf index: {}", insert_witness.new_leaf_index);
    
    // Serialize the witness (simulating sending to zkVM)
    let serialized_insert = borsh::to_vec(&insert_witness).unwrap();
    println!("  Serialized witness size: {} bytes", serialized_insert.len());
    
    // Generate witness for updating an existing key
    println!("\n2. Generating UPDATE witness:");
    let update_key = Field::from_u32(20);
    let update_value = Field::from_u32(999);
    
    let update_witness = map.update_and_generate_witness(update_key, update_value, true)
        .unwrap()
        .expect("Update witness generation failed");
    
    println!("  ✓ Generated witness for updating key=20 to value=999");
    println!("  Old value: {}", update_witness.old_value.to_u256());
    println!("  New value: {}", update_witness.new_value.to_u256());
    println!("  Old root: {:?}", update_witness.old_root);
    println!("  New root: {:?}", update_witness.new_root);
    
    // Serialize the witness
    let serialized_update = borsh::to_vec(&update_witness).unwrap();
    println!("  Serialized witness size: {} bytes", serialized_update.len());
    
    // === INSIDE zkVM: Verify using static methods ===
    println!("\n\nINSIDE zkVM: Verifying witnesses");
    println!("---------------------------------\n");
    println!("(Using only ProvableIndexedMerkleMap static methods)\n");
    
    // Deserialize and verify insert witness
    println!("1. Verifying INSERT witness:");
    let zkvm_insert_witness: InsertWitness = borsh::from_slice(&serialized_insert).unwrap();
    
    match ProvableIndexedMerkleMap::insert(&zkvm_insert_witness) {
        Ok(()) => {
            println!("  ✓ Insert verified successfully!");
            println!("    Confirmed: key=25 was inserted at index {}", 
                zkvm_insert_witness.new_leaf_index);
            println!("    Old root: {:?}", zkvm_insert_witness.old_root);
            println!("    New root: {:?}", zkvm_insert_witness.new_root);
        }
        Err(e) => {
            println!("  ✗ Verification failed: {}", e);
        }
    }
    
    // Deserialize and verify update witness
    println!("\n2. Verifying UPDATE witness:");
    let zkvm_update_witness: UpdateWitness = borsh::from_slice(&serialized_update).unwrap();
    
    match ProvableIndexedMerkleMap::update(&zkvm_update_witness) {
        Ok(()) => {
            println!("  ✓ Update verified successfully!");
            println!("    Confirmed: key=20 updated from {} to {}", 
                zkvm_update_witness.old_value.to_u256(),
                zkvm_update_witness.new_value.to_u256());
            println!("    Old root: {:?}", zkvm_update_witness.old_root);
            println!("    New root: {:?}", zkvm_update_witness.new_root);
        }
        Err(e) => {
            println!("  ✗ Verification failed: {}", e);
        }
    }
    
    // Demonstrate the efficiency gains
    println!("\n\n=== Efficiency Analysis ===");
    println!("Traditional approach:");
    println!("  - Send entire merkle tree to zkVM");
    println!("  - Tree size: ~{} bytes for {} entries", 
        std::mem::size_of_val(&map), map.length());
    println!("\nWitness-based approach:");
    println!("  - Send only witness to zkVM");
    println!("  - Insert witness: {} bytes", serialized_insert.len());
    println!("  - Update witness: {} bytes", serialized_update.len());
    println!("  - Reduction: >{}x smaller!", 
        std::mem::size_of_val(&map) / serialized_insert.len());
    
    println!("\n✅ zkVM witness example completed successfully!");
    
    // Summary
    println!("\n=== Summary ===");
    println!("This pattern allows zkVM programs to:");
    println!("1. Receive only the witness (small data)");
    println!("2. Verify the operation is valid");
    println!("3. Return the new root");
    println!("Without ever loading the entire merkle map into the zkVM!");
}