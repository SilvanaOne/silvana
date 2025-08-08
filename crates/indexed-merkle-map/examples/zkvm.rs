//! Example showing how to use IndexedMerkleMap with zkVM
//! 
//! This demonstrates the separation between provable and non-provable code:
//! - Outside zkVM: Use IndexedMerkleMap to manage the tree and generate witnesses
//! - Inside zkVM: Use ProvableIndexedMerkleMap to verify operations

use indexed_merkle_map::{
    Field, IndexedMerkleMap, ProvableIndexedMerkleMap, InsertWitness, UpdateWitness
};

fn main() {
    println!("IndexedMerkleMap zkVM Usage Example\n");
    
    // ===== OUTSIDE zkVM: Tree Management =====
    println!("OUTSIDE zkVM: Managing the tree and generating witnesses");
    
    // Create and populate the tree
    let mut map = IndexedMerkleMap::new(10);
    
    // Insert some initial data
    map.insert(Field::from_u32(10), Field::from_u32(100)).unwrap();
    map.insert(Field::from_u32(20), Field::from_u32(200)).unwrap();
    map.insert(Field::from_u32(30), Field::from_u32(300)).unwrap();
    
    println!("Initial tree created with 3 entries");
    println!("Root: {:?}", map.root());
    
    // Generate witness for inserting a new key
    let new_key = Field::from_u32(25);
    let new_value = Field::from_u32(250);
    
    let witness = map.insert_and_generate_witness(new_key, new_value, true)
        .unwrap()
        .expect("Witness generation failed");
    
    println!("\nGenerated witness for inserting key 25");
    println!("Old root: {:?}", witness.old_root);
    println!("New root: {:?}", witness.new_root);
    
    // Serialize witness to send to zkVM
    let serialized_witness = borsh::to_vec(&witness).unwrap();
    println!("Serialized witness size: {} bytes", serialized_witness.len());
    
    // ===== INSIDE zkVM: Verification Only =====
    println!("\n---\n");
    println!("INSIDE zkVM: Verifying operations with witnesses only");
    
    // In zkVM, we would receive the serialized witness
    // and deserialize it
    let zkvm_witness: InsertWitness = borsh::from_slice(&serialized_witness).unwrap();
    
    // Verify the insertion is valid using only static methods
    // Note: We use ProvableIndexedMerkleMap, not IndexedMerkleMap
    match ProvableIndexedMerkleMap::insert(&zkvm_witness) {
        Ok(()) => {
            println!("✓ Insertion verified successfully!");
            println!("  Old root matches: {:?}", zkvm_witness.old_root);
            println!("  New root verified: {:?}", zkvm_witness.new_root);
            println!("  Key {} inserted at index {}", 
                zkvm_witness.key.to_u256(), 
                zkvm_witness.new_leaf_index
            );
        }
        Err(e) => {
            println!("✗ Verification failed: {}", e);
        }
    }
    
    // ===== Example: Update with Witness =====
    println!("\n---\n");
    println!("Example: Updating a value with witness");
    
    // Outside zkVM: Generate witness for updating
    let update_key = Field::from_u32(20);
    let new_value = Field::from_u32(999);
    
    let update_witness = map.update_and_generate_witness(update_key, new_value, true)
        .unwrap()
        .expect("Update witness generation failed");
    
    println!("Generated witness for updating key 20");
    println!("Old value: {:?}", update_witness.old_value);
    println!("New value: {:?}", update_witness.new_value);
    
    // Serialize witness to send to zkVM
    let serialized_update = borsh::to_vec(&update_witness).unwrap();
    
    // Inside zkVM: Verify update using the witness
    let zkvm_update_witness: UpdateWitness = borsh::from_slice(&serialized_update).unwrap();
    
    match ProvableIndexedMerkleMap::update(&zkvm_update_witness) {
        Ok(()) => {
            println!("✓ Update verified successfully!");
            println!("  Old root matches: {:?}", zkvm_update_witness.old_root);
            println!("  New root verified: {:?}", zkvm_update_witness.new_root);
            println!("  Value updated from {} to {}", 
                zkvm_update_witness.old_value.to_u256(),
                zkvm_update_witness.new_value.to_u256()
            );
        }
        Err(e) => {
            println!("✗ Update verification failed: {}", e);
        }
    }
    
    // ===== Feature Flag Demonstration =====
    println!("\n---\n");
    println!("Feature flags:");
    
    #[cfg(feature = "zkvm")]
    {
        println!("✓ zkvm feature enabled - IndexedMerkleMap is hidden");
        println!("  Only ProvableIndexedMerkleMap and types are available");
    }
    
    #[cfg(not(feature = "zkvm"))]
    {
        println!("✓ Default features enabled - Full API available");
        println!("  Both IndexedMerkleMap and ProvableIndexedMerkleMap are available");
    }
    
    println!("\nTo compile for zkVM only:");
    println!("  cargo build --features zkvm --no-default-features");
    println!("\nThis will exclude IndexedMerkleMap and only include:");
    println!("  - ProvableIndexedMerkleMap (static verification methods)");
    println!("  - Common types (Field, Hash, Leaf, proofs, witnesses)");
}