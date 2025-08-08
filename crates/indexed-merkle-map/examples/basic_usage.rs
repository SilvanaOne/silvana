//! Basic usage example for IndexedMerkleMap
//! 
//! This example demonstrates:
//! - Creating a new map
//! - Inserting and updating key-value pairs
//! - Using the o1js-compatible set() and get() methods
//! - Generating and verifying membership/non-membership proofs

use indexed_merkle_map::{IndexedMerkleMap, Field};

fn main() {
    println!("IndexedMerkleMap Basic Usage Example\n");
    println!("=====================================\n");
    
    // Create a new map with height 10 (supports up to 2^9 = 512 entries)
    let mut map = IndexedMerkleMap::new(10);
    println!("✓ Created new IndexedMerkleMap with height 10");
    println!("  Initial root: {:?}\n", map.root());
    
    // Insert key-value pairs
    println!("Inserting key-value pairs:");
    let key = Field::from_u32(100);
    let value = Field::from_u32(200);
    map.insert(key, value).unwrap();
    println!("  ✓ Inserted key=100, value=200");
    
    // Update an existing value
    let new_value = Field::from_u32(300);
    let old_value = map.update(key, new_value).unwrap();
    println!("  ✓ Updated key=100 from value={} to value=300", old_value.to_u256());
    
    // Or use set() for insert-or-update (o1js compatible)
    println!("\nUsing set() for insert-or-update:");
    let key2 = Field::from_u32(50);
    let value2 = Field::from_u32(400);
    let previous = map.set(key2, value2).expect("Set should succeed"); // Returns None if key didn't exist
    match previous {
        None => println!("  ✓ Set new key=50, value=400 (key didn't exist)"),
        Some(v) => println!("  ✓ Updated key=50 from value={} to value=400", v.to_u256()),
    }
    
    // Get value with get_option() (returns Option<Field>)
    println!("\nRetrieving values:");
    let value = map.get_option(&key).unwrap_or(Field::zero());
    println!("  get_option(100) = {}", value.to_u256());
    
    // Or use get() which panics if key doesn't exist
    let value = map.get(&key);
    println!("  get(100) = {}", value.to_u256());
    
    // Get membership proof
    println!("\nGenerating and verifying membership proof:");
    let proof = map.get_membership_proof(&key).unwrap();
    let root = map.root();
    let tree_length = map.length();
    
    println!("  Generated proof for key=100");
    println!("  Proof leaf: key={}, value={}, next_key={}", 
        proof.leaf.key.to_u256(), 
        proof.leaf.value.to_u256(),
        proof.leaf.next_key.to_u256()
    );
    
    // Verify the proof
    let is_valid = IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &new_value, tree_length);
    println!("  ✓ Membership proof verified: {}", is_valid);
    assert!(is_valid);
    
    // Get non-membership proof for a non-existent key
    println!("\nGenerating and verifying non-membership proof:");
    let non_existent_key = Field::from_u32(999);
    let non_proof = map.get_non_membership_proof(&non_existent_key).unwrap();
    
    println!("  Generated proof for non-existent key=999");
    println!("  Low leaf: key={}, next_key={}", 
        non_proof.low_leaf.key.to_u256(),
        non_proof.low_leaf.next_key.to_u256()
    );
    println!("  Verifying: {} < 999 < {}", 
        non_proof.low_leaf.key.to_u256(),
        if non_proof.low_leaf.next_key == Field::zero() {
            "∞".to_string()
        } else {
            non_proof.low_leaf.next_key.to_u256().to_string()
        }
    );
    
    let is_valid = IndexedMerkleMap::verify_non_membership_proof(&root, &non_proof, &non_existent_key, tree_length);
    println!("  ✓ Non-membership proof verified: {}", is_valid);
    assert!(is_valid);
    
    // Display final state
    println!("\nFinal map state:");
    println!("  Root: {:?}", map.root());
    println!("  Number of entries: {}", map.length());
    println!("  Sorted keys in map:");
    for leaf in map.sorted_leaves() {
        if leaf.key != Field::zero() {
            println!("    key={}, value={}", leaf.key.to_u256(), leaf.value.to_u256());
        }
    }
    
    println!("\n✅ Basic usage example completed successfully!");
}