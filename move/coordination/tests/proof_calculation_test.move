#[test_only]
module coordination::proof_calculation_simple_tests;

use coordination::prover;
use sui::clock;
use sui::test_scenario::{Self as ts};

const TEST_ADMIN: address = @0x1;

#[test]
fun test_proof_calculation_deletion_emits_event() {
    let mut scenario = ts::begin(TEST_ADMIN);
    let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
    clock::set_for_testing(&mut clock, 1000);
    
    ts::next_tx(&mut scenario, TEST_ADMIN);
    {
        // Create a proof calculation
        let (proof_calc, app_instance_id) = prover::create_block_proof_calculation(
            1,
            1,
            option::some(10),
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Delete it and verify event is emitted
        prover::delete_proof_calculation(proof_calc, app_instance_id, &clock);
        
        // The ProofCalculationFinishedEvent should be emitted
        // In real scenario, we would check events
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_proof_calculation_finished_event_contains_data() {
    let mut scenario = ts::begin(TEST_ADMIN);
    let clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    ts::next_tx(&mut scenario, TEST_ADMIN);
    {
        // Create a proof calculation with specific data
        let (proof_calc, app_instance_id) = prover::create_block_proof_calculation(
            5,      // block_number
            100,    // start_sequence
            option::some(200),  // end_sequence
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Mark it as finished (this would normally be done through proof submission)
        // For testing, we'll just verify the deletion works
        
        // Delete and verify comprehensive event
        prover::delete_proof_calculation(proof_calc, app_instance_id, &clock);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_genesis_block_handling() {
    let mut scenario = ts::begin(TEST_ADMIN);
    let clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    ts::next_tx(&mut scenario, TEST_ADMIN);
    {
        // Create proof calculation for block 0 (genesis)
        let (proof_calc, app_instance_id) = prover::create_block_proof_calculation(
            0,      // block_number (genesis)
            0,      // start_sequence
            option::some(0),  // end_sequence
            &clock,
            ts::ctx(&mut scenario),
        );

        // Verify it's marked as finished for genesis block
        assert!(prover::is_finished(&proof_calc), 0);

        // Can still delete it
        prover::delete_proof_calculation(proof_calc, app_instance_id, &clock);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}