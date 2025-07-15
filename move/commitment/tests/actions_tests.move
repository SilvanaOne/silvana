#[test_only]
module commitment::actions_tests;

use commitment::action::{
    create_action,
    get_action,
    get_action_data,
    calculate_action_commitment,
    Action
};
use commitment::actions::{
    create_actions_commitment,
    commit_action,
    get_sequence,
    get_commitment
};
use std::string;
use sui::bls12381::scalar_zero;
use sui::test_scenario::{Self, Scenario};

const ADMIN: address = @0xAD;

// Test helper functions
fun setup_test_scenario(): Scenario {
    test_scenario::begin(ADMIN)
}

fun create_test_action(): Action {
    let action_string = string::utf8(b"test_action");
    let action_data = vector[1u256, 2u256, 3u256];
    create_action(action_string, action_data)
}

fun create_test_action_with_data(
    action_str: vector<u8>,
    data: vector<u256>,
): Action {
    let action_string = string::utf8(action_str);
    create_action(action_string, data)
}

// Action module tests

#[test]
fun test_create_action_success() {
    let action_string = string::utf8(b"test_action");
    let action_data = vector[1u256, 2u256, 3u256];
    let action = create_action(action_string, action_data);

    assert!(get_action(&action) == action_string, 0);
    assert!(get_action_data(&action) == action_data, 1);
}

#[test]
fun test_create_action_max_length() {
    // Test with exactly 30 characters (should pass)
    let action_string = string::utf8(b"012345678901234567890123456789"); // 30 chars
    let action_data = vector[1u256];
    let action = create_action(action_string, action_data);

    assert!(get_action(&action) == action_string, 0);
}

#[test]
#[
    expected_failure(
        abort_code = 1,
        location = commitment::action,
    ),
] // EInvalidAction
fun test_create_action_too_long() {
    // Test with 31 characters (should fail)
    let action_string = string::utf8(b"0123456789012345678901234567890"); // 31 chars
    let action_data = vector[1u256];
    create_action(action_string, action_data);
}

#[test]
fun test_create_action_empty_data() {
    let action_string = string::utf8(b"empty_data");
    let action_data = vector::empty<u256>();
    let action = create_action(action_string, action_data);

    assert!(get_action(&action) == action_string, 0);
    assert!(get_action_data(&action) == action_data, 1);
    assert!(vector::length(&get_action_data(&action)) == 0, 2);
}

#[test]
fun test_create_action_large_data() {
    let action_string = string::utf8(b"large_data");
    let action_data = vector[
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
    let action = create_action(action_string, action_data);

    assert!(get_action(&action) == action_string, 0);
    assert!(get_action_data(&action) == action_data, 1);
    assert!(vector::length(&get_action_data(&action)) == 10, 2);
}

#[test]
fun test_get_action_and_data() {
    let action = create_test_action();
    let expected_action = string::utf8(b"test_action");
    let expected_data = vector[1u256, 2u256, 3u256];

    assert!(get_action(&action) == expected_action, 0);
    assert!(get_action_data(&action) == expected_data, 1);
}

#[test]
fun test_calculate_action_commitment() {
    let action = create_test_action();
    let commitment = calculate_action_commitment(&action);

    // Test that commitment is calculated (not zero)
    assert!(commitment != scalar_zero(), 0);
}

#[test]
fun test_calculate_action_commitment_consistency() {
    let action1 = create_test_action_with_data(b"same", vector[1u256, 2u256]);
    let action2 = create_test_action_with_data(b"same", vector[1u256, 2u256]);

    let commitment1 = calculate_action_commitment(&action1);
    let commitment2 = calculate_action_commitment(&action2);

    // Same action should produce same commitment
    assert!(commitment1 == commitment2, 0);
}

#[test]
fun test_calculate_action_commitment_different() {
    let action1 = create_test_action_with_data(b"action1", vector[1u256]);
    let action2 = create_test_action_with_data(b"action2", vector[1u256]);

    let commitment1 = calculate_action_commitment(&action1);
    let commitment2 = calculate_action_commitment(&action2);

    // Different actions should produce different commitments
    assert!(commitment1 != commitment2, 0);
}

// Actions module tests

#[test]
fun test_create_actions_commitment() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let actions_commitment = create_actions_commitment(ctx);

    assert!(get_sequence(&actions_commitment) == 0, 0);
    assert!(get_commitment(&actions_commitment) == scalar_zero(), 1);

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_commit_single_action() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut actions_commitment = create_actions_commitment(ctx);
    let action = create_test_action();
    let action_commitment = calculate_action_commitment(&action);

    commit_action(&mut actions_commitment, &action_commitment);

    assert!(get_sequence(&actions_commitment) == 1, 0);
    assert!(get_commitment(&actions_commitment) != scalar_zero(), 1);

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_commit_multiple_actions() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut actions_commitment = create_actions_commitment(ctx);

    // Commit first action
    let action1 = create_test_action_with_data(b"action1", vector[1u256]);
    let action1_commitment = calculate_action_commitment(&action1);
    commit_action(&mut actions_commitment, &action1_commitment);

    let commitment_after_first = get_commitment(&actions_commitment);

    // Commit second action
    let action2 = create_test_action_with_data(b"action2", vector[2u256]);
    let action2_commitment = calculate_action_commitment(&action2);
    commit_action(&mut actions_commitment, &action2_commitment);

    let commitment_after_second = get_commitment(&actions_commitment);

    assert!(get_sequence(&actions_commitment) == 2, 0);
    assert!(commitment_after_first != commitment_after_second, 1);
    assert!(commitment_after_second != scalar_zero(), 2);

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_sequence_progression() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut actions_commitment = create_actions_commitment(ctx);

    // Initial sequence should be 0
    assert!(get_sequence(&actions_commitment) == 0, 0);

    // Commit 5 actions and verify sequence progression
    let mut i = 0;
    while (i < 5) {
        let action_str = b"action";
        let action_data = vector[i as u256];
        let action = create_test_action_with_data(action_str, action_data);
        let action_commitment = calculate_action_commitment(&action);
        commit_action(&mut actions_commitment, &action_commitment);

        assert!(get_sequence(&actions_commitment) == i + 1, i + 1);
        i = i + 1;
    };

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_commitment_changes_with_each_action() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut actions_commitment = create_actions_commitment(ctx);
    let mut previous_commitment = get_commitment(&actions_commitment);

    // Commit 3 actions and verify commitment changes each time
    let mut i = 0;
    while (i < 3) {
        let action_str = b"action";
        let action_data = vector[i as u256];
        let action = create_test_action_with_data(action_str, action_data);
        let action_commitment = calculate_action_commitment(&action);
        commit_action(&mut actions_commitment, &action_commitment);

        let current_commitment = get_commitment(&actions_commitment);
        assert!(current_commitment != previous_commitment, i);
        previous_commitment = current_commitment;
        i = i + 1;
    };

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}

#[test]
fun test_empty_action_commitment() {
    let action = create_test_action_with_data(b"empty", vector::empty<u256>());
    let commitment = calculate_action_commitment(&action);

    // Even empty action data should produce a valid commitment
    assert!(commitment != scalar_zero(), 0);
}

#[test]
fun test_action_commitment_with_zero_data() {
    let action = create_test_action_with_data(b"zero", vector[0u256]);
    let commitment = calculate_action_commitment(&action);

    // Action with zero data should still produce a valid commitment
    assert!(commitment != scalar_zero(), 0);
}

#[test]
fun test_integration_action_and_actions_commitment() {
    let mut scenario = setup_test_scenario();
    let ctx = test_scenario::ctx(&mut scenario);

    let mut actions_commitment = create_actions_commitment(ctx);

    // Create and commit multiple different actions
    let action1 = create_test_action_with_data(
        b"transfer",
        vector[100u256, 200u256],
    );
    let action1_commitment = calculate_action_commitment(&action1);
    commit_action(&mut actions_commitment, &action1_commitment);

    let action2 = create_test_action_with_data(b"mint", vector[50u256]);
    let action2_commitment = calculate_action_commitment(&action2);
    commit_action(&mut actions_commitment, &action2_commitment);

    let action3 = create_test_action_with_data(
        b"burn",
        vector[25u256, 75u256, 125u256],
    );
    let action3_commitment = calculate_action_commitment(&action3);
    commit_action(&mut actions_commitment, &action3_commitment);

    assert!(get_sequence(&actions_commitment) == 3, 0);
    assert!(get_commitment(&actions_commitment) != scalar_zero(), 1);

    // Transfer to sender and clean up
    transfer::public_transfer(actions_commitment, ADMIN);
    test_scenario::end(scenario);
}
