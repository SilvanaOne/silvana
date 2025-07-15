#[test_only]
module commitment::state_tests;

use commitment::action::{create_action, Action};
use commitment::state::{
    create_app_state,
    create_state_update,
    commit_action,
    get_sequence,
    get_state_elements_length,
    get_state_commitment,
    get_actions_commitment,
    has_state_element,
    get_state_update_index,
    get_state_update_new_state,
    StateUpdate
};
use std::string;
use sui::bls12381::scalar_zero;
use sui::test_scenario::{Self, Scenario};

const ADMIN: address = @0xAD;

// Test helper functions
fun setup_test_scenario(): Scenario {
    test_scenario::begin(ADMIN)
}

fun create_test_action(name: vector<u8>, data: vector<u256>): Action {
    let action_string = string::utf8(name);
    create_action(action_string, data)
}

// Basic creation tests

#[test]
fun test_create_app_state() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let app_state = create_app_state(ctx);

    // Test initial state using getters
    assert!(get_sequence(&app_state) == 0, 0);
    assert!(get_state_elements_length(&app_state) == 0, 1);
    assert!(get_state_commitment(&app_state) == scalar_zero(), 2);

    // Test that actions commitment is initialized
    let actions_commitment = get_actions_commitment(&app_state);
    assert!(actions_commitment.get_sequence() == 0, 3);

    transfer::public_transfer(app_state, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_create_state_update() {
    let index = 5;
    let new_state = vector[100u256, 200u256, 300u256];

    let state_update = create_state_update(index, new_state);

    // Test StateUpdate using getters
    assert!(get_state_update_index(&state_update) == index, 0);
    assert!(*get_state_update_new_state(&state_update) == new_state, 1);
}

#[test]
fun test_create_state_update_empty() {
    let index = 0;
    let new_state = vector::empty<u256>();

    let state_update = create_state_update(index, new_state);

    // Test StateUpdate with empty data
    assert!(get_state_update_index(&state_update) == index, 0);
    assert!(*get_state_update_new_state(&state_update) == new_state, 1);
    assert!(vector::length(get_state_update_new_state(&state_update)) == 0, 2);
}

#[test]
fun test_create_state_update_large_data() {
    let index = 10;
    let new_state = vector[
        1u256,
        2u256,
        3u256,
        4u256,
        5u256,
        6u256,
        7u256,
        8u256,
        9u256,
        10u256,
    ];

    let state_update = create_state_update(index, new_state);

    // Test StateUpdate with large data
    assert!(get_state_update_index(&state_update) == index, 0);
    assert!(vector::length(get_state_update_new_state(&state_update)) == 10, 1);
}

#[test]
fun test_create_state_update_zero_index() {
    let index = 0;
    let new_state = vector[42u256];

    let _state_update = create_state_update(index, new_state);

    // StateUpdate was created successfully with index 0
}

#[test]
fun test_create_state_update_high_index() {
    let index = 999;
    let new_state = vector[1000u256, 2000u256];

    let _state_update = create_state_update(index, new_state);

    // StateUpdate was created successfully with high index
}

#[test]
fun test_create_state_update_single_element() {
    let index = 1;
    let new_state = vector[123u256];

    let _state_update = create_state_update(index, new_state);

    // StateUpdate was created successfully with single element
}

#[test]
fun test_create_state_update_max_u256() {
    let index = 42;
    let max_u256 =
        0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF;
    let new_state = vector[max_u256];

    let _state_update = create_state_update(index, new_state);

    // StateUpdate was created successfully with max u256 value
}

#[test]
fun test_multiple_state_updates() {
    let _update1 = create_state_update(0, vector[10u256]);
    let _update2 = create_state_update(1, vector[20u256, 30u256]);
    let _update3 = create_state_update(5, vector[50u256, 60u256, 70u256]);

    // All StateUpdates were created successfully
}

#[test]
fun test_state_update_data_integrity() {
    let index = 7;
    let original_data = vector[100u256, 200u256, 300u256, 400u256];

    let _state_update = create_state_update(index, original_data);

    // StateUpdate was created successfully preserving data
}

#[test]
fun test_action_creation_for_state_tests() {
    // Test that our helper function works correctly
    let action1 = create_test_action(b"test", vector[1u256]);
    let action2 = create_test_action(
        b"another_test",
        vector[1u256, 2u256, 3u256],
    );

    // Verify actions are created with correct internal state
    assert!(action1.get_action() == string::utf8(b"test"), 0);
    assert!(action1.get_action_data() == vector[1u256], 1);

    assert!(action2.get_action() == string::utf8(b"another_test"), 2);
    assert!(action2.get_action_data() == vector[1u256, 2u256, 3u256], 3);

    // Both actions should be different objects with different commitments
    let commitment1 = action1.calculate_action_commitment();
    let commitment2 = action2.calculate_action_commitment();
    assert!(commitment1 != commitment2, 4);
}

#[test]
fun test_commit_action_empty_updates() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut app_state = create_app_state(ctx);
    let action = create_test_action(b"empty_test", vector[1u256]);
    let state_updates = vector::empty<StateUpdate>();

    let old_sequence = get_sequence(&app_state);
    let old_commitment = get_state_commitment(&app_state);

    // This should work with empty updates since no state elements need to exist
    commit_action(&mut app_state, action, &state_updates, ctx);

    // Verify state changes
    assert!(get_sequence(&app_state) == old_sequence + 1, 0);
    assert!(get_actions_commitment(&app_state).get_sequence() == 1, 1);
    assert!(get_state_commitment(&app_state) == old_commitment, 2); // Should be unchanged

    transfer::public_transfer(app_state, ADMIN);
    test_scenario::end(scenario);
}

// Integration tests combining multiple components

#[test]
fun test_create_multiple_app_states() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    // Create multiple app states
    let app_state1 = create_app_state(ctx);
    let app_state2 = create_app_state(ctx);
    let app_state3 = create_app_state(ctx);

    // All should be created successfully
    transfer::public_transfer(app_state1, ADMIN);
    transfer::public_transfer(app_state2, ADMIN);
    transfer::public_transfer(app_state3, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_state_update_vector_operations() {
    let index = 3;
    let mut state_data = vector::empty<u256>();
    vector::push_back(&mut state_data, 10u256);
    vector::push_back(&mut state_data, 20u256);
    vector::push_back(&mut state_data, 30u256);

    let _state_update = create_state_update(index, state_data);

    // StateUpdate was created successfully with vector operations
}

#[test]
fun test_create_state_updates_with_different_patterns() {
    // Test creating state updates with various patterns
    let _update_small = create_state_update(0, vector[1u256]);
    let _update_medium = create_state_update(
        5,
        vector[1u256, 2u256, 3u256, 4u256, 5u256],
    );
    let _update_large = create_state_update(
        10,
        vector[
            10u256,
            20u256,
            30u256,
            40u256,
            50u256,
            60u256,
            70u256,
            80u256,
            90u256,
            100u256,
        ],
    );

    // All different sized updates created successfully
}

#[test]
fun test_app_state_creation_consistency() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    // Create multiple app states to ensure consistent behavior
    let app_state1 = create_app_state(ctx);
    let app_state2 = create_app_state(ctx);

    // Both should be created successfully and be independent objects
    transfer::public_transfer(app_state1, ADMIN);
    transfer::public_transfer(app_state2, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_state_update_with_zero_values() {
    let index = 1;
    let new_state = vector[0u256, 0u256, 0u256];

    let state_update = create_state_update(index, new_state);

    // Test StateUpdate with zero values
    assert!(get_state_update_index(&state_update) == index, 0);
    assert!(vector::length(get_state_update_new_state(&state_update)) == 3, 1);
    assert!(
        *vector::borrow(get_state_update_new_state(&state_update), 0) == 0u256,
        2,
    );
}

#[test]
fun test_multiple_state_updates_with_getters() {
    let update1 = create_state_update(0, vector[10u256]);
    let update2 = create_state_update(1, vector[20u256, 30u256]);
    let update3 = create_state_update(5, vector[50u256, 60u256, 70u256]);

    // Test that each update has correct properties using getters
    assert!(get_state_update_index(&update1) == 0, 0);
    assert!(vector::length(get_state_update_new_state(&update1)) == 1, 1);

    assert!(get_state_update_index(&update2) == 1, 2);
    assert!(vector::length(get_state_update_new_state(&update2)) == 2, 3);

    assert!(get_state_update_index(&update3) == 5, 4);
    assert!(vector::length(get_state_update_new_state(&update3)) == 3, 5);
}

#[test]
fun test_state_update_data_integrity_with_getters() {
    let index = 7;
    let original_data = vector[100u256, 200u256, 300u256, 400u256];

    let state_update = create_state_update(index, original_data);

    // Verify all data is preserved correctly using getters
    assert!(vector::length(get_state_update_new_state(&state_update)) == 4, 0);
    assert!(
        *vector::borrow(get_state_update_new_state(&state_update), 0) == 100u256,
        1,
    );
    assert!(
        *vector::borrow(get_state_update_new_state(&state_update), 1) == 200u256,
        2,
    );
    assert!(
        *vector::borrow(get_state_update_new_state(&state_update), 2) == 300u256,
        3,
    );
    assert!(
        *vector::borrow(get_state_update_new_state(&state_update), 3) == 400u256,
        4,
    );
}

#[test]
fun test_app_state_has_state_element() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let app_state = create_app_state(ctx);

    // Test that no state elements exist initially
    assert!(!has_state_element(&app_state, 0), 0);
    assert!(!has_state_element(&app_state, 1), 1);
    assert!(!has_state_element(&app_state, 999), 2);

    transfer::public_transfer(app_state, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_commit_multiple_actions_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut app_state = create_app_state(ctx);

    // Commit first action
    let action1 = create_test_action(b"action1", vector[1u256]);
    let state_updates1 = vector::empty<StateUpdate>();
    commit_action(&mut app_state, action1, &state_updates1, ctx);

    assert!(get_sequence(&app_state) == 1, 0);
    assert!(get_actions_commitment(&app_state).get_sequence() == 1, 1);

    // Commit second action
    let action2 = create_test_action(b"action2", vector[2u256]);
    let state_updates2 = vector::empty<StateUpdate>();
    commit_action(&mut app_state, action2, &state_updates2, ctx);

    assert!(get_sequence(&app_state) == 2, 2);
    assert!(get_actions_commitment(&app_state).get_sequence() == 2, 3);

    transfer::public_transfer(app_state, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_state_commitment_changes() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut app_state = create_app_state(ctx);
    let initial_commitment = get_state_commitment(&app_state);

    // Commit action without state updates - commitment should stay the same
    let action = create_test_action(b"no_state_change", vector[1u256]);
    let state_updates = vector::empty<StateUpdate>();
    commit_action(&mut app_state, action, &state_updates, ctx);

    let final_commitment = get_state_commitment(&app_state);
    assert!(initial_commitment == final_commitment, 0);

    transfer::public_transfer(app_state, ADMIN);
    test_scenario::end(scenario);
}
