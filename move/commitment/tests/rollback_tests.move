#[test_only]
module commitment::rollback_tests;

use commitment::action::{create_action, Action};
use commitment::rollback::{
    create_rollback,
    create_rollback_element,
    add_rollback_sequence,
    purge_records,
    get_start_sequence,
    get_end_sequence,
    has_rollback_sequence,
    RollbackElement
};
use std::string;
use sui::bls12381::{scalar_zero, scalar_one, scalar_add};
use sui::test_scenario::{Self, Scenario};

const ADMIN: address = @0xAD;

// Test helper functions
fun setup_test_scenario(): Scenario {
    test_scenario::begin(ADMIN)
}

fun create_test_element(
    index: u32,
    prev_val: u256,
    new_val: u256,
): RollbackElement {
    let previous_state = option::some(vector[prev_val]);
    let new_state = vector[new_val];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();
    create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    )
}

fun create_test_action(): Action {
    create_action(
        string::utf8(b"test_action"),
        vector[100u256, 200u256] // action_data is a vector<u256>
    )
}

// Note: RollbackElement fields are private and cannot be accessed in tests.
// We can only test the public functions that create and use these elements.

// Tests

#[test]
fun test_create_rollback() {
    let mut scenario = setup_test_scenario();
    
    let ctx = test_scenario::ctx(&mut scenario);
    let rollback = create_rollback(ctx);
    
    // Test initial state
    assert!(get_start_sequence(&rollback) == 0, 0);
    assert!(get_end_sequence(&rollback) == 0, 1);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_create_rollback_element_with_previous_state() {
    let index = 1u32;
    let previous_state = option::some(vector[100u256]);
    let new_state = vector[200u256];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();
    
    // Just verify the element can be created successfully
    let _element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );
    // Element creation successful - fields are private so can't verify contents
}

#[test]
fun test_create_rollback_element_empty_vectors() {
    let index = 5u32;
    let previous_state = option::some(vector::empty<u256>());
    let new_state = vector::empty<u256>();
    let commitment_before = scalar_zero();
    let commitment_after = scalar_zero();
    
    // Just verify the element can be created successfully with empty vectors
    let _element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );
    // Element creation successful - fields are private so can't verify contents
}

#[test]
fun test_create_rollback_element_without_previous_state() {
    let index = 10u32;
    let previous_state = option::none<vector<u256>>();
    let new_state = vector[500u256];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();
    
    // Just verify the element can be created successfully without previous state
    let _element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );
    // Element creation successful - fields are private so can't verify contents
}

#[test]
fun test_add_rollback_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action = create_test_action();
    let elements = vector[
        create_test_element(0, 100, 200),
        create_test_element(1, 300, 400),
    ];
    
    add_rollback_sequence(&mut rollback, 1, action, elements);
    
    assert!(get_end_sequence(&rollback) == 1, 0);
    assert!(has_rollback_sequence(&rollback, 1), 1);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_add_multiple_rollback_sequences() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    let action3 = create_test_action();
    let elements3 = vector[create_test_element(2, 500, 600)];
    add_rollback_sequence(&mut rollback, 3, action3, elements3);
    
    assert!(get_end_sequence(&rollback) == 3, 0);
    assert!(has_rollback_sequence(&rollback, 1), 1);
    assert!(has_rollback_sequence(&rollback, 2), 2);
    assert!(has_rollback_sequence(&rollback, 3), 3);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test, expected_failure(abort_code = commitment::rollback::EInvalidSequence)]
fun test_add_rollback_sequence_invalid_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action = create_test_action();
    let elements = vector[create_test_element(0, 100, 200)];
    
    // Try to add sequence 2 when it should be 1
    add_rollback_sequence(&mut rollback, 2, action, elements);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test, expected_failure(abort_code = commitment::rollback::EInvalidSequence)]
fun test_add_rollback_sequence_gap_in_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    // Try to add sequence 3 when it should be 2
    add_rollback_sequence(&mut rollback, 3, action2, elements2);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_has_rollback_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action = create_test_action();
    let elements = vector[create_test_element(0, 100, 200)];
    
    assert!(!has_rollback_sequence(&rollback, 1), 0);
    
    add_rollback_sequence(&mut rollback, 1, action, elements);
    
    assert!(has_rollback_sequence(&rollback, 1), 1);
    assert!(!has_rollback_sequence(&rollback, 2), 2);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_start_and_end_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    assert!(get_start_sequence(&rollback) == 0, 0);
    assert!(get_end_sequence(&rollback) == 0, 1);
    
    let action = create_test_action();
    let elements = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action, elements);
    
    assert!(get_start_sequence(&rollback) == 0, 2);
    assert!(get_end_sequence(&rollback) == 1, 3);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_purge_records_within_range() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    assert!(has_rollback_sequence(&rollback, 1), 0);
    assert!(has_rollback_sequence(&rollback, 2), 1);
    
    // Purge sequence 1
    purge_records(&mut rollback, 1);
    
    assert!(!has_rollback_sequence(&rollback, 1), 2);
    assert!(has_rollback_sequence(&rollback, 2), 3);
    assert!(get_start_sequence(&rollback) == 2, 4);
    assert!(get_end_sequence(&rollback) == 2, 5);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test, expected_failure(abort_code = commitment::rollback::EInvalidPurgeSequence)]
fun test_purge_records_out_of_range() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    // Try to purge beyond the end sequence
    purge_records(&mut rollback, 3);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_purge_records_multiple() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    let action3 = create_test_action();
    let elements3 = vector[create_test_element(2, 500, 600)];
    add_rollback_sequence(&mut rollback, 3, action3, elements3);
    
    // Purge sequences 1 and 2
    purge_records(&mut rollback, 2);
    
    assert!(!has_rollback_sequence(&rollback, 1), 0);
    assert!(!has_rollback_sequence(&rollback, 2), 1);
    assert!(has_rollback_sequence(&rollback, 3), 2);
    assert!(get_start_sequence(&rollback) == 3, 3);
    assert!(get_end_sequence(&rollback) == 3, 4);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_purge_records_all() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[create_test_element(0, 100, 200)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(1, 300, 400)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    // Purge all sequences
    purge_records(&mut rollback, 2);
    
    assert!(!has_rollback_sequence(&rollback, 1), 0);
    assert!(!has_rollback_sequence(&rollback, 2), 1);
    assert!(get_start_sequence(&rollback) == 3, 2);
    assert!(get_end_sequence(&rollback) == 2, 3);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}

#[test]
fun test_complex_rollback_element() {
    let index = 100u32;
    let previous_state = option::some(vector[1u256, 2u256, 3u256]);
    let new_state = vector[10u256, 20u256, 30u256, 40u256];
    
    let commitment_before = scalar_zero();
    let commitment_after = scalar_add(&scalar_one(), &scalar_one());
    
    // Just verify the element can be created successfully with complex data
    let _element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );
    // Element creation successful - fields are private so can't verify contents
}

#[test]
fun test_rollback_sequence_with_multiple_elements() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);
    let mut rollback = create_rollback(ctx);
    
    let action1 = create_test_action();
    let elements1 = vector[
        create_test_element(0, 100, 200),
        create_test_element(1, 300, 400),
    ];
    add_rollback_sequence(&mut rollback, 1, action1, elements1);
    
    let action2 = create_test_action();
    let elements2 = vector[create_test_element(2, 500, 600)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2);
    
    let action3 = create_test_action();
    let elements3 = vector[
        create_test_element(3, 700, 800),
        create_test_element(4, 900, 1000),
        create_test_element(5, 1100, 1200),
    ];
    add_rollback_sequence(&mut rollback, 3, action3, elements3);
    
    assert!(get_end_sequence(&rollback) == 3, 0);
    assert!(has_rollback_sequence(&rollback, 1), 1);
    assert!(has_rollback_sequence(&rollback, 2), 2);
    assert!(has_rollback_sequence(&rollback, 3), 3);
    
    // Purge first sequence
    purge_records(&mut rollback, 1);
    
    assert!(!has_rollback_sequence(&rollback, 1), 4);
    assert!(has_rollback_sequence(&rollback, 2), 5);
    assert!(has_rollback_sequence(&rollback, 3), 6);
    assert!(get_start_sequence(&rollback) == 2, 7);
    assert!(get_end_sequence(&rollback) == 3, 8);
    
    transfer::public_share_object(rollback);
    test_scenario::end(scenario);
}