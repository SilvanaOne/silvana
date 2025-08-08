# IndexedMerkleMap

A Rust implementation of an Indexed Merkle Map, inspired by the IndexedMerkleMap design by [o1js](https://github.com/o1-labs/o1js/blob/main/src/lib/provable/merkle-tree-indexed.ts) and [aztec](https://docs.aztec.network/aztec/concepts/advanced/storage/indexed_merkle_tree)

## Features

- **Efficient key-value storage**: Uses an indexed merkle tree structure for efficient membership and non-membership proofs
- **SHA256 hashing**: Uses SHA256 as the hash function, with SP1 precompile support when running in zkVM
- **Small tree height**: Configurable tree height (1-32) to minimize proof sizes and hashing operations
- **Membership proofs**: Generate and verify proofs that a key-value pair exists in the map
- **Non-membership proofs**: Generate and verify proofs that a key does not exist in the map
- **no_std support**: Can be used in embedded and zkVM environments

## Usage

```rust
use indexed_merkle_map::{IndexedMerkleMap, Field};

// Create a new map with height 10 (supports up to 2^9 = 512 entries)
let mut map = IndexedMerkleMap::new(10);

// Insert key-value pairs
let key = Field::from_u32(100);
let value = Field::from_u32(200);
map.insert(key, value).unwrap();

// Update an existing value
let new_value = Field::from_u32(300);
let old_value = map.update(key, new_value).unwrap();

// Or use set() for insert-or-update (o1js compatible)
let key2 = Field::from_u32(50);
let value2 = Field::from_u32(400);
let previous = map.set(key2, value2)?; // Returns Ok(None) if key didn't exist, Err(TreeFull) if at capacity

// Get value with get_option() (returns Option<Field>)
let value = map.get_option(&key).unwrap_or(Field::zero());

// Or use get() which panics if key doesn't exist
let value = map.get(&key);

// Get membership proof
let proof = map.get_membership_proof(&key).unwrap();
let root = map.root();
let tree_length = map.length();

// Verify the proof
assert!(IndexedMerkleMap::verify_membership_proof(&root, &proof, &key, &new_value, tree_length));

// Get non-membership proof for a non-existent key
let non_existent_key = Field::from_u32(999);
let non_proof = map.get_non_membership_proof(&non_existent_key).unwrap();
assert!(IndexedMerkleMap::verify_non_membership_proof(&root, &non_proof, &non_existent_key, tree_length));
```

## SP1 zkVM Support

This library is optimized for use in SP1 zkVM with:

- Patched `crypto-bigint` and `sha2` dependencies for better performance
- Static methods for updating/inserting using only proofs (no full map needed)

For efficient zkVM usage, the library provides static methods that work with proofs only:

```rust
use indexed_merkle_map::{IndexedMerkleMap, ProvableIndexedMerkleMap, Field};

// === Outside zkVM: Generate witnesses ===

// Insert using witness-based approach
let witness = map.insert_and_generate_witness(new_key, new_value, true)?
    .expect("Witness generation failed");

// Update using witness-based approach
let update_witness = map.update_and_generate_witness(key, new_value, true)?
    .expect("Update witness generation failed");

// === Inside zkVM: Verify using static methods ===

// Verify insertion (no full map needed)
ProvableIndexedMerkleMap::insert(&witness)?;
// The witness contains both old_root and new_root for verification

// Verify update (no full map needed)
ProvableIndexedMerkleMap::update(&update_witness)?;
// The witness contains old_value, new_value, old_root and new_root

// This allows zkVM programs to:
// 1. Receive only the witness (small data)
// 2. Verify the operation is valid
// 3. Return the new root
// Without ever loading the entire merkle map into the zkVM
```

This pattern significantly reduces the amount of data that needs to be read inside the zkVM, making operations much more efficient.

## Running Examples

The library includes comprehensive examples demonstrating various usage patterns:

```bash
# Basic usage - insert, update, proofs
cargo run --example basic_usage

# zkVM witness generation and verification
cargo run --example zkvm_witness

# Complete zkVM workflow demonstration
cargo run --example zkvm
```

### Example Descriptions

- **basic_usage**: Demonstrates fundamental operations including insert, update, set/get methods, and proof generation/verification
- **zkvm_witness**: Shows the witness-based API for efficient zkVM usage with serialization
- **zkvm**: Complete end-to-end zkVM workflow with feature flags and optimization patterns

## Implementation Details

The IndexedMerkleMap uses a sorted linked-list structure within a Merkle tree:

- Each leaf contains: `(key, value, next_key)`
- Leaves are sorted by key with the linked list encoded via `next_key`
- The first leaf is always `(0, 0, 0)`
- Non-membership is proven by showing a "low leaf" where `low.key < target_key < low.next_key`

This design allows for:

- Efficient non-membership proofs without sparse Merkle trees
- Small tree heights (e.g., height 20 for ~1 million entries)
- Significantly fewer hash operations compared to sparse 256-level trees
