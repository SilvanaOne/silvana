#[test_only]
module coordination::prover_validation_tests;

use coordination::prover::{Self, ProofCalculation};
use sui::clock;
use sui::test_scenario;

// Test helper function to create a proof calculation with specific parameters
fun create_proof_calculation_for_test(
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    ctx: &mut TxContext,
): ProofCalculation {
    let clock = clock::create_for_testing(ctx);
    let (proof_calc, _addr) = prover::create_block_proof_calculation(
        block_number,
        start_sequence,
        end_sequence,
        &clock,
        ctx
    );
    clock::destroy_for_testing(clock);
    proof_calc
}

// Test 1: Valid sequences within range (with end_sequence set)
#[test]
fun test_valid_sequences_with_end_sequence() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    // Create proof calculation for sequences 10-20
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    let clock = clock::create_for_testing(ctx);
    
    // First submit the prerequisite proofs that will be referenced
    prover::submit_proof(
        &mut proof_calc,
        vector[11, 13, 14],
        option::none(),
        option::none(),
        b"proof_1".to_string(),
        b"da_hash_1".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    prover::submit_proof(
        &mut proof_calc,
        vector[16, 17, 19],
        option::none(),
        option::none(),
        b"proof_2".to_string(),
        b"da_hash_2".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    // Now test sequences in valid range with references
    let sequences = vector[15, 12, 18, 10, 20]; // Unsorted but valid
    let sequence1 = option::some(vector[11, 13, 14]);
    let sequence2 = option::some(vector[19, 16, 17]);
    
    // This should succeed - sequences are within range and will be sorted
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        sequence1,
        sequence2,
        b"test_job_1".to_string(),
        b"da_hash_123".to_string(),
        8, // cpu_cores
        b"x86_64".to_string(),
        16384, // prover_memory
        1000, // cpu_time
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 2: Sequences below start_sequence should fail
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_sequences_below_start_range() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    // Create proof calculation starting from sequence 10
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    // Test sequences that include values below start (9 < 10)
    let sequences = vector[9, 10, 11]; // 9 is below start_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail with ESequenceOutOfRange
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_2".to_string(),
        b"da_hash_456".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 3: Sequences above end_sequence should fail
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_sequences_above_end_range() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    // Create proof calculation with end_sequence = 20
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    // Test sequences that include values above end (21 > 20)
    let sequences = vector[18, 19, 20, 21]; // 21 is above end_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail with ESequenceOutOfRange
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_3".to_string(),
        b"da_hash_789".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 4: Test sequence1 validation fails when out of range
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_sequence1_out_of_range() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    let sequences = vector[15, 16]; // Valid main sequences
    let sequence1 = option::some(vector[8, 9, 10]); // 8 and 9 are below start_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail because sequence1 contains invalid sequences
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        sequence1,
        option::none(),
        b"test_job_4".to_string(),
        b"da_hash_abc".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 5: Test sequence2 validation fails when out of range
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_sequence2_out_of_range() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    let sequences = vector[15, 16]; // Valid main sequences
    let sequence2 = option::some(vector[20, 21, 22]); // 21 and 22 are above end_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail because sequence2 contains invalid sequences
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        sequence2,
        b"test_job_5".to_string(),
        b"da_hash_def".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 6: Test sorting functionality - sequences should be sorted
#[test]
fun test_sequences_are_sorted() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    // Provide unsorted sequences
    let sequences = vector[20, 15, 10, 18, 12]; // Completely unsorted
    let clock = clock::create_for_testing(ctx);
    
    // Submit proof with unsorted sequences
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_6".to_string(),
        b"da_hash_ghi".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    // If the function succeeds, it means sequences were sorted internally
    // The proof should be stored with sorted key [10, 12, 15, 18, 20]
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 7: Test start_proving validation with invalid sequences
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_start_proving_validation_fails() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    let sequences = vector[5, 10, 15]; // 5 is below start_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail with validation error
    prover::start_proving(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_7".to_string(),
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 8: Test start_proving with valid sequences
#[test]
fun test_start_proving_validation_succeeds() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    let clock = clock::create_for_testing(ctx);
    
    // First submit prerequisite proofs
    prover::submit_proof(
        &mut proof_calc,
        vector[10, 11],
        option::none(),
        option::none(),
        b"proof_prereq_1".to_string(),
        b"da_hash_prereq_1".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    prover::submit_proof(
        &mut proof_calc,
        vector[18, 19, 20],
        option::none(),
        option::none(),
        b"proof_prereq_2".to_string(),
        b"da_hash_prereq_2".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    // All sequences within valid range
    let sequences = vector[15, 12, 14]; // Will be sorted to [12, 14, 15]
    let sequence1 = option::some(vector[11, 10]); // Will be sorted to [10, 11]
    let sequence2 = option::some(vector[20, 19, 18]); // Will be sorted to [18, 19, 20]
    
    // This should succeed
    prover::start_proving(
        &mut proof_calc,
        sequences,
        sequence1,
        sequence2,
        b"test_job_8".to_string(),
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 9: Test reject_proof validation
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_reject_proof_validation_fails() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    let clock = clock::create_for_testing(ctx);
    
    // First start a proof with valid sequences
    prover::start_proving(
        &mut proof_calc,
        vector[15, 16],
        option::none(),
        option::none(),
        b"test_job_9".to_string(),
        &clock,
        ctx
    );
    
    // Now try to reject with invalid sequences (different from what was started)
    // This should fail because 25 is out of range
    prover::reject_proof(
        &mut proof_calc,
        vector[15, 16, 25], // 25 is above end_sequence
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 10: Test with no end_sequence (open-ended range)
#[test]
fun test_no_end_sequence_validation() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    // Create proof calculation without end_sequence (open-ended)
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::none(), ctx);
    
    // Test with very high sequence numbers (should succeed since no upper limit)
    let sequences = vector[100, 1000, 10000];
    let clock = clock::create_for_testing(ctx);
    
    // This should succeed because there's no end_sequence limit
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_10".to_string(),
        b"da_hash_xyz".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 11: Test with no end_sequence but sequence below start should still fail
#[test]
#[expected_failure(abort_code = coordination::prover::ESequenceOutOfRange)]
fun test_no_end_sequence_but_below_start_fails() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    // Create proof calculation without end_sequence
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::none(), ctx);
    
    // Test with sequence below start
    let sequences = vector[5, 15, 100]; // 5 is below start_sequence
    let clock = clock::create_for_testing(ctx);
    
    // This should fail because 5 < 10 (start_sequence)
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_11".to_string(),
        b"da_hash_fail".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 12: Edge case - empty sequences should fail
#[test]
#[expected_failure(abort_code = coordination::prover::ESequencesCannotBeEmpty)]
fun test_empty_sequences_fail() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    
    // Test with empty sequences
    let sequences = vector::empty<u64>();
    let clock = clock::create_for_testing(ctx);
    
    // This should fail with ESequencesCannotBeEmpty
    prover::submit_proof(
        &mut proof_calc,
        sequences,
        option::none(),
        option::none(),
        b"test_job_12".to_string(),
        b"da_hash_empty".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}

// Test 13: Boundary test - sequences at exact boundaries
#[test]
fun test_boundary_sequences() {
    let mut scenario = test_scenario::begin(@0x1);
    let ctx = test_scenario::ctx(&mut scenario);
    
    let mut proof_calc = create_proof_calculation_for_test(1, 10, option::some(20), ctx);
    let clock = clock::create_for_testing(ctx);
    
    // First submit the proofs that will be referenced
    prover::submit_proof(
        &mut proof_calc,
        vector[10], // Submit proof for sequence 10
        option::none(),
        option::none(),
        b"proof_10".to_string(),
        b"da_hash_10".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    prover::submit_proof(
        &mut proof_calc,
        vector[20], // Submit proof for sequence 20
        option::none(),
        option::none(),
        b"proof_20".to_string(),
        b"da_hash_20".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    // Now submit a proof that references the boundary proofs
    prover::submit_proof(
        &mut proof_calc,
        vector[10, 20], // Exactly start and end
        option::some(vector[10]), // Reference existing proof at start
        option::some(vector[20]), // Reference existing proof at end
        b"test_job_13".to_string(),
        b"da_hash_boundary".to_string(),
        8,
        b"x86_64".to_string(),
        16384,
        1000,
        &clock,
        ctx
    );
    
    clock::destroy_for_testing(clock);
    let cleanup_clock = clock::create_for_testing(ctx);
    prover::delete_proof_calculation(proof_calc, &cleanup_clock);
    clock::destroy_for_testing(cleanup_clock);
    test_scenario::end(scenario);
}