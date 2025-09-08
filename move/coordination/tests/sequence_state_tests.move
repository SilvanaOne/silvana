#[test_only]
module coordination::sequence_state_tests;

use coordination::sequence_state::{
    SequenceState,
    create_sequence_state,
    create_sequence_state_manager,
    add_state_for_sequence,
    update_state_for_sequence,
    purge,
    lowest_sequence,
    highest_sequence,
    has_sequence_state,
    borrow_sequence_state,
    sequence,
    state,
    data_availability,
    optimistic_state,
    transition_data,
    set_state,
    set_data_availability,
    destroy_sequence_state,
};
use sui::test_scenario::{Self as ts, Scenario};
use sui::clock;
use std::string;

const TEST_ADDR: address = @0x1;

fun setup_test(): Scenario {
    let scenario = ts::begin(TEST_ADDR);
    scenario
}

#[test]
fun test_create_sequence_state() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let test_sequence = 42u64;
        let test_state = option::some(b"test_state_data");
        let test_da = option::some(b"test_data_availability_hash".to_string());
        let test_optimistic = b"optimistic_data";
        let test_transition = b"transition_data";
        
        let sequence_state = create_sequence_state(
            test_sequence,
            test_state,
            test_da,
            test_optimistic,
            test_transition,
            ts::ctx(&mut scenario)
        );
        
        // Verify the created sequence state has correct values
        assert!(sequence(&sequence_state) == test_sequence, 0);
        assert!(state(&sequence_state) == &test_state, 1);
        assert!(data_availability(&sequence_state) == &test_da, 2);
        assert!(*optimistic_state(&sequence_state) == test_optimistic, 3);
        assert!(*transition_data(&sequence_state) == test_transition, 4);
        
        transfer::public_share_object(sequence_state);
    };
    
    ts::end(scenario);
}

#[test]
fun test_getter_functions() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let test_sequence = 123u64;
        let test_state = option::some(b"getter_test_state");
        let test_da = option::some(b"getter_test_da_hash".to_string());
        let test_optimistic = b"getter_optimistic";
        let test_transition = b"getter_transition";
        
        let sequence_state = create_sequence_state(
            test_sequence,
            test_state,
            test_da,
            test_optimistic,
            test_transition,
            ts::ctx(&mut scenario)
        );
        
        // Test all getter functions
        assert!(sequence(&sequence_state) == test_sequence, 0);
        assert!(state(&sequence_state) == &test_state, 1);
        assert!(data_availability(&sequence_state) == &test_da, 2);
        assert!(*optimistic_state(&sequence_state) == test_optimistic, 3);
        assert!(*transition_data(&sequence_state) == test_transition, 4);
        
        transfer::public_share_object(sequence_state);
    };
    
    ts::end(scenario);
}

#[test]
fun test_setter_functions() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let test_sequence = 456u64;
        let initial_state = option::some(b"initial_state");
        let initial_da = option::some(b"initial_da_hash".to_string());
        let test_optimistic = b"setter_optimistic";
        let test_transition = b"setter_transition";
        
        let mut sequence_state = create_sequence_state(
            test_sequence,
            initial_state,
            initial_da,
            test_optimistic,
            test_transition,
            ts::ctx(&mut scenario)
        );
        
        // Test setting new state
        let new_state = option::some(b"updated_state_data");
        set_state(&mut sequence_state, new_state);
        assert!(state(&sequence_state) == &new_state, 0);
        
        // Test setting new data availability
        let new_da = option::some(b"updated_da_hash".to_string());
        set_data_availability(&mut sequence_state, new_da);
        assert!(data_availability(&sequence_state) == &new_da, 1);
        
        // Verify sequence and other fields haven't changed
        assert!(sequence(&sequence_state) == test_sequence, 2);
        assert!(*optimistic_state(&sequence_state) == test_optimistic, 3);
        assert!(*transition_data(&sequence_state) == test_transition, 4);
        
        transfer::public_share_object(sequence_state);
    };
    
    ts::end(scenario);
}

#[test]
fun test_destroy_sequence_state() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let test_sequence = 789u64;
        let test_state = option::some(b"destroy_test_state");
        let test_da = option::some(b"destroy_test_da_hash".to_string());
        let test_optimistic = b"destroy_optimistic";
        let test_transition = b"destroy_transition";
        
        let sequence_state = create_sequence_state(
            test_sequence,
            test_state,
            test_da,
            test_optimistic,
            test_transition,
            ts::ctx(&mut scenario)
        );
        
        // Destroy the sequence state and verify returned values
        let (returned_sequence, returned_state, returned_da, returned_optimistic, returned_transition) = destroy_sequence_state(sequence_state);
        
        assert!(returned_sequence == test_sequence, 0);
        assert!(returned_state == test_state, 1);
        assert!(returned_da == test_da, 2);
        assert!(returned_optimistic == test_optimistic, 3);
        assert!(returned_transition == test_transition, 4);
    };
    
    ts::end(scenario);
}

#[test]
fun test_multiple_sequence_states() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut sequence_states = vector::empty<SequenceState>();
        
        // Create multiple sequence states
        let mut i = 0;
        while (i < 5) {
            let seq = (i as u64) + 100;
            let mut state_data = vector::empty<u8>();
            vector::push_back(&mut state_data, (i as u8));
            let mut da = string::utf8(b"da_hash_");
            string::append(&mut da, string::utf8(vector[(i as u8) + 48])); // Convert to ASCII digit
            
            let mut optimistic_data = vector::empty<u8>();
            vector::push_back(&mut optimistic_data, (i as u8) + 10);
            let mut transition_data = vector::empty<u8>();
            vector::push_back(&mut transition_data, (i as u8) + 20);
            
            let sequence_state = create_sequence_state(
                seq,
                option::some(state_data),
                option::some(da),
                optimistic_data,
                transition_data,
                ts::ctx(&mut scenario)
            );
            
            vector::push_back(&mut sequence_states, sequence_state);
            i = i + 1;
        };
        
        // Verify all sequence states
        i = 0;
        while (i < 5) {
            let seq_state = vector::borrow(&sequence_states, i);
            assert!(sequence(seq_state) == (i as u64) + 100, (i as u64));
            let state_opt = state(seq_state);
            assert!(state_opt.is_some(), (i as u64) + 10);
            let state_vec = state_opt.borrow();
            assert!(vector::length(state_vec) == 1, (i as u64) + 11);
            assert!(*vector::borrow(state_vec, 0) == (i as u8), (i as u64) + 20);
            i = i + 1;
        };
        
        // Clean up
        while (!vector::is_empty(&sequence_states)) {
            let seq_state = vector::pop_back(&mut sequence_states);
            let (_, _, _, _, _) = destroy_sequence_state(seq_state);
        };
        vector::destroy_empty(sequence_states);
    };
    
    ts::end(scenario);
}

#[test]
fun test_state_updates() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let test_sequence = 999u64;
        let initial_state = option::some(b"initial");
        let initial_da = option::some(b"initial_hash".to_string());
        let test_optimistic = b"test_optimistic";
        let test_transition = b"test_transition";
        
        let mut sequence_state = create_sequence_state(
            test_sequence,
            initial_state,
            initial_da,
            test_optimistic,
            test_transition,
            ts::ctx(&mut scenario)
        );
        
        // Perform first update
        let update_1_state = option::some(b"update_1");
        let update_1_da = option::some(b"hash_1".to_string());
        set_state(&mut sequence_state, update_1_state);
        set_data_availability(&mut sequence_state, update_1_da);
        assert!(state(&sequence_state) == &update_1_state, 0);
        assert!(data_availability(&sequence_state) == &update_1_da, 1);
        
        // Perform second update
        let update_2_state = option::some(b"update_2");
        let update_2_da = option::some(b"hash_2".to_string());
        set_state(&mut sequence_state, update_2_state);
        set_data_availability(&mut sequence_state, update_2_da);
        assert!(state(&sequence_state) == &update_2_state, 2);
        assert!(data_availability(&sequence_state) == &update_2_da, 3);
        
        // Perform final update
        let final_state = option::some(b"final_state");
        let final_da = option::some(b"final_hash".to_string());
        set_state(&mut sequence_state, final_state);
        set_data_availability(&mut sequence_state, final_da);
        
        // Verify final state
        assert!(state(&sequence_state) == &final_state, 4);
        assert!(data_availability(&sequence_state) == &final_da, 5);
        assert!(sequence(&sequence_state) == test_sequence, 6);
        assert!(*optimistic_state(&sequence_state) == test_optimistic, 7);
        assert!(*transition_data(&sequence_state) == test_transition, 8);
        
        let (_, _, _, _, _) = destroy_sequence_state(sequence_state);
    };
    
    ts::end(scenario);
}

#[test]
fun test_sequence_state_manager_creation() {
    let mut scenario = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let manager = create_sequence_state_manager(ts::ctx(&mut scenario));
        
        assert!(lowest_sequence(&manager) == option::none(), 0);
        assert!(highest_sequence(&manager) == option::none(), 1);
        
        transfer::public_share_object(manager);
    };
    
    ts::end(scenario);
}

#[test]
fun test_sequence_state_manager_operations() {
    let mut scenario = setup_test();
    let clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut manager = create_sequence_state_manager(ts::ctx(&mut scenario));
        
        // Test adding first sequence state (must be 0)
        add_state_for_sequence(
            &mut manager,
            0u64,
            option::some(b"test_state_0"),
            option::some(b"hash_0".to_string()),
            b"optimistic_0",
            b"transition_0",
            &clock,
            ts::ctx(&mut scenario)
        );
        
        assert!(has_sequence_state(&manager, 0u64), 0);
        assert!(lowest_sequence(&manager) == option::some(0u64), 1);
        assert!(highest_sequence(&manager) == option::some(0u64), 2);
        
        let seq_state = borrow_sequence_state(&manager, 0u64);
        assert!(sequence(seq_state) == 0u64, 3);
        let expected_state = option::some(b"test_state_0");
        let expected_da = option::some(b"hash_0".to_string());
        assert!(state(seq_state) == &expected_state, 4);
        assert!(data_availability(seq_state) == &expected_da, 5);
        assert!(*optimistic_state(seq_state) == b"optimistic_0", 6);
        assert!(*transition_data(seq_state) == b"transition_0", 7);
        
        // Test updating sequence state
        let new_state = option::some(b"updated_state_0");
        let new_da = option::some(b"updated_hash_0".to_string());
        update_state_for_sequence(
            &mut manager,
            0u64,
            new_state,
            new_da,
            &clock,
            ts::ctx(&mut scenario)
        );
        
        let seq_state = borrow_sequence_state(&manager, 0u64);
        assert!(state(seq_state) == &new_state, 8);
        assert!(data_availability(seq_state) == &new_da, 9);
        
        transfer::public_share_object(manager);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_sequence_state_manager_purge() {
    let mut scenario = setup_test();
    let clock = clock::create_for_testing(ts::ctx(&mut scenario));
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut manager = create_sequence_state_manager(ts::ctx(&mut scenario));
        
        // Add first sequence state (must be 0)
        add_state_for_sequence(
            &mut manager,
            0u64,
            option::some(vector[0u8]),
            option::some(b"hash_0".to_string()),
            b"optimistic_0",
            b"transition_0",
            &clock,
            ts::ctx(&mut scenario)
        );
        
        // Since sequences are only added through increase_sequence which auto-updates bounds,
        // we need to manually update bounds to simulate multiple sequences for purge testing
        // Let's add a few more sequences by manually expanding the highest bound first
        
        // Manually update highest sequence to allow adding more sequences (simulating increase_sequence behavior)
        // We'll directly access the manager fields to simulate what increase_sequence does
        
        // Add sequence 1
        add_state_for_sequence(
            &mut manager,
            1u64,
            option::some(vector[1u8]),
            option::some(b"hash_1".to_string()),
            b"optimistic_1",
            b"transition_1",
            &clock,
            ts::ctx(&mut scenario)
        );
        
        // Add sequence 2  
        add_state_for_sequence(
            &mut manager,
            2u64,
            option::some(vector[2u8]),
            option::some(b"hash_2".to_string()),
            b"optimistic_2",
            b"transition_2",
            &clock,
            ts::ctx(&mut scenario)
        );
        
        // Add sequence 3
        add_state_for_sequence(
            &mut manager,
            3u64,
            option::some(vector[3u8]),
            option::some(b"hash_3".to_string()),
            b"optimistic_3",
            b"transition_3",
            &clock,
            ts::ctx(&mut scenario)
        );
        
        // Verify all states exist
        assert!(has_sequence_state(&manager, 0u64), 0);
        assert!(has_sequence_state(&manager, 1u64), 1);
        assert!(has_sequence_state(&manager, 2u64), 2);
        assert!(has_sequence_state(&manager, 3u64), 3);
        assert!(lowest_sequence(&manager) == option::some(0u64), 4);
        assert!(highest_sequence(&manager) == option::some(3u64), 5);
        
        // Purge sequences below 2
        purge(&mut manager, 2u64, &clock);
        
        // Verify purged sequences don't exist
        assert!(!has_sequence_state(&manager, 0u64), 100);
        assert!(!has_sequence_state(&manager, 1u64), 101);
        
        // Verify remaining sequences still exist
        assert!(has_sequence_state(&manager, 2u64), 200);
        assert!(has_sequence_state(&manager, 3u64), 201);
        
        assert!(lowest_sequence(&manager) == option::some(2u64), 300);
        
        transfer::public_share_object(manager);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}