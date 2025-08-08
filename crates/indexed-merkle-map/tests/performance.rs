use indexed_merkle_map::{Field, IndexedMerkleMap};
use std::time::Instant;

#[test]
fn test_large_sequential_inserts() {
    let mut map = IndexedMerkleMap::new(20); // Height 20 can hold ~1M entries
    
    let start = Instant::now();
    let count = 1000;
    
    for i in 1..=count {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i * 10);
        map.insert(key, value).expect("Insert should succeed");
    }
    
    let duration = start.elapsed();
    println!("Inserted {} items in {:?}", count, duration);
    println!("Average time per insert: {:?}", duration / count);
    
    // Verify random samples
    for i in (1..=count).step_by(100) {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i * 10);
        let proof = map.get_membership_proof(&key).unwrap();
        assert_eq!(proof.leaf.value, value);
    }
}

#[test]
fn test_large_random_inserts() {
    let mut map = IndexedMerkleMap::new(20);
    
    // Generate pseudo-random keys (deterministic for reproducibility)
    let mut keys = Vec::new();
    let count = 1000;
    let mut seed = 12345u32;
    
    for _ in 0..count {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let key = seed % 1000000; // Keep keys in reasonable range
        keys.push(key);
    }
    
    let start = Instant::now();
    
    for (i, &key) in keys.iter().enumerate() {
        let key_field = Field::from_u32(key);
        let value = Field::from_u32(i as u32);
        
        // Skip duplicates
        if map.get_membership_proof(&key_field).is_none() {
            map.insert(key_field, value).expect("Insert should succeed");
        }
    }
    
    let duration = start.elapsed();
    println!("Random inserts completed in {:?}", duration);
}

#[test]
fn test_proof_generation_performance() {
    let mut map = IndexedMerkleMap::new(15);
    
    // Populate the tree
    for i in 1..=500 {
        let key = Field::from_u32(i * 2); // Even numbers
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    // Measure membership proof generation
    let key = Field::from_u32(500);
    let start = Instant::now();
    let iterations = 1000;
    
    for _ in 0..iterations {
        let _ = map.get_membership_proof(&key);
    }
    
    let duration = start.elapsed();
    println!("Generated {} membership proofs in {:?}", iterations, duration);
    println!("Average time per proof: {:?}", duration / iterations);
    
    // Measure non-membership proof generation
    let non_key = Field::from_u32(501); // Odd number not in tree
    let start = Instant::now();
    
    for _ in 0..iterations {
        let _ = map.get_non_membership_proof(&non_key);
    }
    
    let duration = start.elapsed();
    println!("Generated {} non-membership proofs in {:?}", iterations, duration);
    println!("Average time per proof: {:?}", duration / iterations);
}

#[test]
fn test_proof_verification_performance() {
    let mut map = IndexedMerkleMap::new(15);
    
    // Populate the tree
    for i in 1..=500 {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i * 10);
        map.insert(key, value).unwrap();
    }
    
    let root = map.root();
    let key = Field::from_u32(250);
    let value = Field::from_u32(2500);
    let membership_proof = map.get_membership_proof(&key).unwrap();
    
    // Measure membership proof verification
    let start = Instant::now();
    let iterations = 10000;
    
    for _ in 0..iterations {
        let _ = IndexedMerkleMap::verify_membership_proof(&root, &membership_proof, &key, &value, map.length());
    }
    
    let duration = start.elapsed();
    println!("Verified {} membership proofs in {:?}", iterations, duration);
    println!("Average time per verification: {:?}", duration / iterations);
    
    // Non-membership proof verification
    let non_key = Field::from_u32(999999);
    let non_proof = map.get_non_membership_proof(&non_key).unwrap();
    
    let start = Instant::now();
    
    for _ in 0..iterations {
        let _ = IndexedMerkleMap::verify_non_membership_proof(&root, &non_proof, &non_key, map.length());
    }
    
    let duration = start.elapsed();
    println!("Verified {} non-membership proofs in {:?}", iterations, duration);
    println!("Average time per verification: {:?}", duration / iterations);
}

#[test]
fn test_update_performance() {
    let mut map = IndexedMerkleMap::new(15);
    
    // Populate the tree
    let count = 500;
    for i in 1..=count {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    // Measure update performance
    let start = Instant::now();
    
    for i in 1..=count {
        let key = Field::from_u32(i);
        let new_value = Field::from_u32(i + 1000);
        map.update(key, new_value).unwrap();
    }
    
    let duration = start.elapsed();
    println!("Updated {} items in {:?}", count, duration);
    println!("Average time per update: {:?}", duration / count);
}

#[test]
fn test_tree_size_scaling() {
    println!("\nTree size scaling test:");
    
    for height in [5, 10, 15, 20] {
        let mut map = IndexedMerkleMap::new(height);
        let max_leaves = 1 << (height - 1); // 2^(height-1) leaves
        let test_size = std::cmp::min(max_leaves / 10, 1000); // Test with 10% capacity or 1000
        
        let start = Instant::now();
        
        for i in 1..=test_size {
            let key = Field::from_u32(i as u32);
            let value = Field::from_u32(i as u32);
            map.insert(key, value).unwrap();
        }
        
        let duration = start.elapsed();
        println!(
            "Height {}: Inserted {} items in {:?} (avg: {:?}/insert)",
            height,
            test_size,
            duration,
            duration / test_size as u32
        );
    }
}

#[test]
fn test_worst_case_insert() {
    // Worst case: inserting at the beginning of sorted list each time
    let mut map = IndexedMerkleMap::new(10);
    
    // First insert some spread out values
    for i in [100, 200, 300, 400, 500] {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    let start = Instant::now();
    
    // Now insert values that go at the beginning
    for i in 1..50 {
        let key = Field::from_u32(i);
        let value = Field::from_u32(i);
        map.insert(key, value).unwrap();
    }
    
    let duration = start.elapsed();
    println!("Worst-case inserts (beginning): {:?}", duration);
}

#[test]
fn test_memory_usage() {
    use std::mem;
    
    let map = IndexedMerkleMap::new(20);
    let base_size = mem::size_of_val(&map);
    println!("Base IndexedMerkleMap size: {} bytes", base_size);
    
    let mut map = IndexedMerkleMap::new(20);
    
    // Measure memory growth
    let sizes = [10, 100, 500, 1000];
    
    for &count in &sizes {
        for i in (map.sorted_leaves().len() as u32)..count {
            let key = Field::from_u32(i * 10);
            let value = Field::from_u32(i);
            if map.get_membership_proof(&key).is_none() {
                map.insert(key, value).unwrap();
            }
        }
        
        let leaf_size = mem::size_of_val(&*map.sorted_leaves()) / map.sorted_leaves().len();
        let total_leaves_size = mem::size_of_val(&*map.sorted_leaves());
        
        println!(
            "After {} inserts: {} leaves, ~{} bytes total, {} bytes/leaf",
            count,
            map.sorted_leaves().len(),
            total_leaves_size,
            leaf_size
        );
    }
}