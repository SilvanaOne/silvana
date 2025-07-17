module commitment::actions;

use constants::constants::get_r;
use sui::bls12381::{Scalar, scalar_one, scalar_zero, scalar_mul, scalar_add};
use sui::event;
use sui::group_ops::Element;

public struct ActionsCommitment has key, store {
    id: UID,
    sequence: u64,
    r_power: Element<Scalar>,
    commitment: Element<Scalar>,
}

public struct ActionsCommitmentCreatedEvent has copy, drop {
    id: address,
    sequence: u64,
    r_power: Element<Scalar>,
    commitment: Element<Scalar>,
}

public struct ActionCommittedEvent has copy, drop {
    actions_commitment_id: address,
    action: Element<Scalar>,
    sequence: u64,
    new_commitment: Element<Scalar>,
    new_r_power: Element<Scalar>,
}

/// Create a new ActionsCommitment
public fun create_actions_commitment(ctx: &mut TxContext): ActionsCommitment {
    let actions_commitment_id = object::new(ctx);
    let address = actions_commitment_id.to_address();
    let initial_r_power = scalar_one();
    let initial_commitment = scalar_zero();

    let actions_commitment = ActionsCommitment {
        id: actions_commitment_id,
        sequence: 0,
        r_power: initial_r_power,
        commitment: initial_commitment,
    };

    event::emit(ActionsCommitmentCreatedEvent {
        id: address,
        sequence: 0,
        r_power: initial_r_power,
        commitment: initial_commitment,
    });

    actions_commitment
}

/// Commit an action to the ActionsCommitment
public fun commit_action(
    actions_commitment: &mut ActionsCommitment,
    action: &Element<Scalar>,
) {
    let r = get_r();
    actions_commitment.commitment =
        scalar_add(
            &actions_commitment.commitment,
            &scalar_mul(action, &actions_commitment.r_power),
        );
    actions_commitment.r_power = scalar_mul(&actions_commitment.r_power, &r);
    actions_commitment.sequence = actions_commitment.sequence + 1;

    event::emit(ActionCommittedEvent {
        actions_commitment_id: actions_commitment.id.to_address(),
        action: *action,
        sequence: actions_commitment.sequence,
        new_commitment: actions_commitment.commitment,
        new_r_power: actions_commitment.r_power,
    });
}

/// Get the current sequence number
public fun get_sequence(actions_commitment: &ActionsCommitment): u64 {
    actions_commitment.sequence
}

/// Get the current commitment
public fun get_commitment(
    actions_commitment: &ActionsCommitment,
): Element<Scalar> {
    actions_commitment.commitment
}
