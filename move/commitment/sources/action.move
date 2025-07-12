module commitment::action;

use commitment::commitment::{digest_struct, scalar_from_u256};
use commitment::string::string_to_field;
use std::string::String;
use sui::bls12381::Scalar;
use sui::event;
use sui::group_ops::Element;

const EInvalidAction: u64 = 1;

public struct Action has copy, drop, store {
    action: String,
    action_data: vector<u256>,
}

public struct ActionCreatedEvent has copy, drop {
    action: String,
    action_data: vector<u256>,
    data_length: u64,
}

public struct ActionCommitmentCalculatedEvent has copy, drop {
    action: String,
    action_data: vector<u256>,
    commitment: Element<Scalar>,
}

public fun create_action(action: String, action_data: vector<u256>): Action {
    assert!(action.length() <= 30, EInvalidAction);

    let new_action = Action {
        action,
        action_data,
    };

    event::emit(ActionCreatedEvent {
        action,
        action_data,
        data_length: action_data.length(),
    });

    new_action
}

/// Get the action string
public fun get_action(action: &Action): String {
    action.action
}

/// Get the action data
public fun get_action_data(action: &Action): vector<u256> {
    action.action_data
}

/// Calculate the commitment for an action
public fun calculate_action_commitment(action: &Action): Element<Scalar> {
    let mut data: vector<Element<Scalar>> = vector::empty();
    data.push_back(scalar_from_u256(string_to_field(action.action)));
    let mut i = 0;
    while (i < action.action_data.length()) {
        data.push_back(scalar_from_u256(action.action_data[i]));
        i = i + 1;
    };

    let commitment = digest_struct(&data);

    event::emit(ActionCommitmentCalculatedEvent {
        action: action.action,
        action_data: action.action_data,
        commitment,
    });

    commitment
}
