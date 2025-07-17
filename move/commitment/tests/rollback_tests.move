#[test_only]
module commitment::rollback_tests;

use commitment::action::{create_action, Action};
use commitment::rollback::{
    create_rollback,
    create_rollback_element,
    add_rollback_sequence,
    purge_records,
    set_traced_sequence,
    get_start_sequence,
    get_traced_sequence,
    get_end_sequence,
    get_rollback_sequence,
    has_rollback_sequence,
    get_rollback_sequence_elements,
    get_rollback_sequence_number,
    get_element_index,
    get_element_previous_state,
    get_element_new_state,
    get_element_commitment_before,
    get_element_commitment_after,
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

fun create_test_action(name: vector<u8>, data: vector<u256>): Action {
    let action_string = string::utf8(name);
    create_action(action_string, data)
}

// Basic creation tests

#[test]
fun test_create_rollback() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let rollback = create_rollback(ctx);

    assert!(get_start_sequence(&rollback) == 0, 0);
    assert!(get_traced_sequence(&rollback) == 0, 1);
    assert!(get_end_sequence(&rollback) == 0, 2);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_create_rollback_element() {
    let index = 5;
    let previous_state = option::some(vector[100u256, 200u256]);
    let new_state = vector[300u256, 400u256];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();

    let element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );

    assert!(get_element_index(&element) == index, 0);
    assert!(get_element_previous_state(&element) == previous_state, 1);
    assert!(*get_element_new_state(&element) == new_state, 2);
    assert!(*get_element_commitment_before(&element) == commitment_before, 3);
    assert!(*get_element_commitment_after(&element) == commitment_after, 4);
}

#[test]
fun test_create_rollback_element_empty_vectors() {
    let index = 0;
    let previous_state = option::some(vector::empty<u256>());
    let new_state = vector::empty<u256>();
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();

    let element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );

    assert!(get_element_index(&element) == index, 0);
    assert!(option::is_some(&get_element_previous_state(&element)), 1);
    assert!(
        vector::length(option::borrow(&get_element_previous_state(&element))) == 0,
        2,
    );
    assert!(vector::length(get_element_new_state(&element)) == 0, 3);
}

#[test]
fun test_create_rollback_element_none_previous_state() {
    let index = 0;
    let previous_state = option::none<vector<u256>>();
    let new_state = vector[100u256];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_one();

    let element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );

    assert!(get_element_index(&element) == index, 0);
    assert!(option::is_none(&get_element_previous_state(&element)), 1);
    assert!(vector::length(get_element_new_state(&element)) == 1, 2);
}

// Sequence addition tests

#[test]
fun test_add_first_rollback_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256]);
    let elements = vector[create_test_element(0, 10u256, 20u256)];

    add_rollback_sequence(&mut rollback, 1, action, elements, ctx);

    assert!(get_end_sequence(&rollback) == 1, 0);
    assert!(has_rollback_sequence(&rollback, 1), 1);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_add_multiple_rollback_sequences() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add first sequence
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    // Add second sequence
    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    // Add third sequence
    let action3 = create_test_action(b"action3", vector[3u256]);
    let elements3 = vector[create_test_element(2, 50u256, 60u256)];
    add_rollback_sequence(&mut rollback, 3, action3, elements3, ctx);

    assert!(get_end_sequence(&rollback) == 3, 0);
    assert!(has_rollback_sequence(&rollback, 1), 1);
    assert!(has_rollback_sequence(&rollback, 2), 2);
    assert!(has_rollback_sequence(&rollback, 3), 3);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 1,
        location = commitment::rollback,
    ),
] // EInvalidSequence
fun test_add_rollback_sequence_invalid_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256]);
    let elements = vector[create_test_element(0, 10u256, 20u256)];

    // Try to add sequence 2 when end_sequence is 0 (should fail)
    add_rollback_sequence(&mut rollback, 2, action, elements, ctx);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 1,
        location = commitment::rollback,
    ),
] // EInvalidSequence
fun test_add_rollback_sequence_wrong_order() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add first sequence
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    // Try to add sequence 3 when end_sequence is 1 (should fail)
    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 3, action2, elements2, ctx);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

// Sequence retrieval tests

#[test]
fun test_get_rollback_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256, 2u256]);
    let elements = vector[
        create_test_element(0, 10u256, 20u256),
        create_test_element(1, 30u256, 40u256),
    ];

    add_rollback_sequence(&mut rollback, 1, action, elements, ctx);

    let sequence = get_rollback_sequence(&rollback, 1);
    assert!(get_rollback_sequence_number(sequence) == 1, 0);

    let sequence_elements = get_rollback_sequence_elements(sequence);
    assert!(vector::length(sequence_elements) == 2, 1);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 2,
        location = commitment::rollback,
    ),
] // ESequenceNotFound
fun test_get_rollback_sequence_not_found() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let rollback = create_rollback(ctx);

    // Try to get sequence that doesn't exist
    get_rollback_sequence(&rollback, 1);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_has_rollback_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256]);
    let elements = vector[create_test_element(0, 10u256, 20u256)];

    assert!(!has_rollback_sequence(&rollback, 1), 0);

    add_rollback_sequence(&mut rollback, 1, action, elements, ctx);

    assert!(has_rollback_sequence(&rollback, 1), 1);
    assert!(!has_rollback_sequence(&rollback, 2), 2);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

// Traced sequence tests

#[test]
fun test_set_traced_sequence() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256]);
    let elements = vector[create_test_element(0, 10u256, 20u256)];

    add_rollback_sequence(&mut rollback, 1, action, elements, ctx);

    set_traced_sequence(&mut rollback, 1);
    assert!(get_traced_sequence(&rollback) == 1, 0);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 3,
        location = commitment::rollback,
    ),
] // EInvalidTracedSequence
fun test_set_traced_sequence_too_high() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action = create_test_action(b"test_action", vector[1u256]);
    let elements = vector[create_test_element(0, 10u256, 20u256)];

    add_rollback_sequence(&mut rollback, 1, action, elements, ctx);

    // Try to set traced sequence higher than end sequence
    set_traced_sequence(&mut rollback, 2);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 3,
        location = commitment::rollback,
    ),
] // EInvalidTracedSequence
fun test_set_traced_sequence_too_low() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    // Purge first sequence
    purge_records(&mut rollback, 1);

    // Try to set traced sequence lower than start sequence
    set_traced_sequence(&mut rollback, 1);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

// Purge records tests

#[test]
fun test_purge_records() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add three sequences
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    let action3 = create_test_action(b"action3", vector[3u256]);
    let elements3 = vector[create_test_element(2, 50u256, 60u256)];
    add_rollback_sequence(&mut rollback, 3, action3, elements3, ctx);

    // Purge sequences 1 and 2
    purge_records(&mut rollback, 2);

    assert!(get_start_sequence(&rollback) == 3, 0);
    assert!(get_traced_sequence(&rollback) == 3, 1); // Should be updated
    assert!(get_end_sequence(&rollback) == 3, 2);
    assert!(!has_rollback_sequence(&rollback, 1), 3);
    assert!(!has_rollback_sequence(&rollback, 2), 4);
    assert!(has_rollback_sequence(&rollback, 3), 5);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_purge_records_partial() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add sequences
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    // Set traced sequence
    set_traced_sequence(&mut rollback, 2);

    // Purge only sequence 1
    purge_records(&mut rollback, 1);

    assert!(get_start_sequence(&rollback) == 2, 0);
    assert!(get_traced_sequence(&rollback) == 2, 1); // Should remain unchanged
    assert!(get_end_sequence(&rollback) == 2, 2);
    assert!(!has_rollback_sequence(&rollback, 1), 3);
    assert!(has_rollback_sequence(&rollback, 2), 4);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 4,
        location = commitment::rollback,
    ),
] // EInvalidPurgeSequence
fun test_purge_records_invalid_sequence_too_low() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add sequences
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    let action2 = create_test_action(b"action2", vector[2u256]);
    let elements2 = vector[create_test_element(1, 30u256, 40u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    // Purge sequence 1
    purge_records(&mut rollback, 1);

    // Try to purge sequence 1 again (should fail - below start_sequence)
    purge_records(&mut rollback, 1);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

#[test]
#[
    expected_failure(
        abort_code = 4,
        location = commitment::rollback,
    ),
] // EInvalidPurgeSequence
fun test_purge_records_invalid_sequence_too_high() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add sequences
    let action1 = create_test_action(b"action1", vector[1u256]);
    let elements1 = vector[create_test_element(0, 10u256, 20u256)];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    // Try to purge sequence 2 (doesn't exist - above end_sequence)
    purge_records(&mut rollback, 2);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}

// Integration tests

#[test]
fun test_rollback_element_with_multiple_data() {
    let index = 10;
    let previous_state = option::some(vector[100u256, 200u256, 300u256]);
    let new_state = vector[400u256, 500u256, 600u256, 700u256];
    let commitment_before = scalar_zero();
    let commitment_after = scalar_add(&scalar_one(), &scalar_one());

    let element = create_rollback_element(
        index,
        previous_state,
        new_state,
        &commitment_before,
        &commitment_after,
    );

    assert!(get_element_index(&element) == index, 0);
    assert!(option::is_some(&get_element_previous_state(&element)), 1);
    assert!(
        vector::length(option::borrow(&get_element_previous_state(&element))) == 3,
        2,
    );
    assert!(vector::length(get_element_new_state(&element)) == 4, 3);
    assert!(
        *vector::borrow(option::borrow(&get_element_previous_state(&element)), 0) == 100u256,
        4,
    );
    assert!(*vector::borrow(get_element_new_state(&element), 3) == 700u256, 5);
}

#[test]
fun test_full_rollback_workflow() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut rollback = create_rollback(ctx);

    // Add multiple sequences with multiple elements
    let action1 = create_test_action(b"transfer", vector[100u256, 200u256]);
    let elements1 = vector[
        create_test_element(0, 10u256, 20u256),
        create_test_element(1, 30u256, 40u256),
    ];
    add_rollback_sequence(&mut rollback, 1, action1, elements1, ctx);

    let action2 = create_test_action(b"mint", vector[50u256]);
    let elements2 = vector[create_test_element(2, 50u256, 60u256)];
    add_rollback_sequence(&mut rollback, 2, action2, elements2, ctx);

    let action3 = create_test_action(b"burn", vector[25u256]);
    let elements3 = vector[create_test_element(3, 70u256, 80u256)];
    add_rollback_sequence(&mut rollback, 3, action3, elements3, ctx);

    // Set traced sequence
    set_traced_sequence(&mut rollback, 2);

    // Verify state
    assert!(get_start_sequence(&rollback) == 0, 0);
    assert!(get_traced_sequence(&rollback) == 2, 1);
    assert!(get_end_sequence(&rollback) == 3, 2);

    // Verify sequences exist
    assert!(has_rollback_sequence(&rollback, 1), 3);
    assert!(has_rollback_sequence(&rollback, 2), 4);
    assert!(has_rollback_sequence(&rollback, 3), 5);

    // Verify sequence content
    let sequence = get_rollback_sequence(&rollback, 1);
    let elements = get_rollback_sequence_elements(sequence);
    assert!(vector::length(elements) == 2, 6);

    // Purge first sequence
    purge_records(&mut rollback, 1);

    // Verify state after purge
    assert!(get_start_sequence(&rollback) == 2, 7);
    assert!(get_traced_sequence(&rollback) == 2, 8);
    assert!(get_end_sequence(&rollback) == 3, 9);
    assert!(!has_rollback_sequence(&rollback, 1), 10);
    assert!(has_rollback_sequence(&rollback, 2), 11);
    assert!(has_rollback_sequence(&rollback, 3), 12);

    transfer::public_transfer(rollback, ADMIN);
    test_scenario::end(scenario);
}
