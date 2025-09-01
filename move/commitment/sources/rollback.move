module commitment::rollback;

use commitment::action::Action;
use std::hash::sha2_256;
use sui::bcs;
use sui::bls12381::Scalar;
use sui::event;
use sui::group_ops::Element;
use sui::vec_map::{Self, VecMap};

public struct RollbackElement has copy, drop, store {
    index: u32,
    previous_state: Option<vector<u256>>,
    new_state: vector<u256>,
    commitment_before: Element<Scalar>,
    commitment_after: Element<Scalar>,
}

public struct RollbackSequence has copy, drop {
    sequence: u64,
    action: Action,
    elements: vector<RollbackElement>,
}

public struct Rollback has key, store {
    id: UID,
    start_sequence: u64,
    end_sequence: u64,
    rollback_sequences: VecMap<u64, vector<u8>>, // sequence -> hash of the rollback sequence
}

public struct RollbackCreatedEvent has copy, drop {
    id: address,
    start_sequence: u64,
    end_sequence: u64,
}

public struct RollbackElementCreatedEvent has copy, drop {
    index: u32,
    previous_state: Option<vector<u256>>,
    new_state: vector<u256>,
    commitment_before: Element<Scalar>,
    commitment_after: Element<Scalar>,
}

public struct RollbackSequenceAddedEvent has copy, drop {
    rollback_sequence: RollbackSequence,
    rollback_sequence_hash: vector<u8>,
    elements_count: u64,
    new_end_sequence: u64,
}

public struct RecordsPurgedEvent has copy, drop {
    rollback_id: address,
    purged_from_sequence: u64,
    purged_to_sequence: u64,
    new_start_sequence: u64,
}

// Error constants
const EInvalidSequence: u64 = 1;
const EInvalidPurgeSequence: u64 = 2;

/// Create Rollback
public fun create_rollback(ctx: &mut TxContext): Rollback {
    let rollback_id = object::new(ctx);
    let address = rollback_id.to_address();

    let rollback = Rollback {
        id: rollback_id,
        start_sequence: 0,
        end_sequence: 0,
        rollback_sequences: vec_map::empty(),
    };

    event::emit(RollbackCreatedEvent {
        id: address,
        start_sequence: 0,
        end_sequence: 0,
    });

    rollback
}

/// Create a RollbackElement
public fun create_rollback_element(
    index: u32,
    previous_state: Option<vector<u256>>,
    new_state: vector<u256>,
    commitment_before: &Element<Scalar>,
    commitment_after: &Element<Scalar>,
): RollbackElement {
    let element = RollbackElement {
        index,
        previous_state,
        new_state,
        commitment_before: *commitment_before,
        commitment_after: *commitment_after,
    };

    event::emit(RollbackElementCreatedEvent {
        index,
        previous_state,
        new_state,
        commitment_before: *commitment_before,
        commitment_after: *commitment_after,
    });

    element
}

/// Add rollback sequence, checking that sequence is end_sequence + 1
public fun add_rollback_sequence(
    rollback: &mut Rollback,
    sequence: u64,
    action: Action,
    elements: vector<RollbackElement>,
) {
    assert!(sequence == rollback.end_sequence + 1, EInvalidSequence);

    let rollback_sequence = RollbackSequence {
        sequence,
        action,
        elements,
    };

    let hash = sha2_256(bcs::to_bytes(&rollback_sequence));

    vec_map::insert(
        &mut rollback.rollback_sequences,
        sequence,
        hash,
    );
    rollback.end_sequence = sequence;

    event::emit(RollbackSequenceAddedEvent {
        rollback_sequence,
        rollback_sequence_hash: hash,
        elements_count: elements.length(),
        new_end_sequence: rollback.end_sequence,
    });
}

/// Purge records by removing the records from start_sequence to proved_sequence
public fun purge_records(rollback: &mut Rollback, proved_sequence: u64) {
    assert!(proved_sequence >= rollback.start_sequence, EInvalidPurgeSequence);
    assert!(proved_sequence <= rollback.end_sequence, EInvalidPurgeSequence);

    let old_start_sequence = rollback.start_sequence;

    let mut current_sequence = rollback.start_sequence;
    while (current_sequence <= proved_sequence) {
        if (
            vec_map::contains(
                &rollback.rollback_sequences,
                &current_sequence,
            )
        ) {
            vec_map::remove(
                &mut rollback.rollback_sequences,
                &current_sequence,
            );
        };
        current_sequence = current_sequence + 1;
    };

    rollback.start_sequence = proved_sequence + 1;

    event::emit(RecordsPurgedEvent {
        rollback_id: rollback.id.to_address(),
        purged_from_sequence: old_start_sequence,
        purged_to_sequence: proved_sequence,
        new_start_sequence: rollback.start_sequence,
    });
}

// Getter functions
public fun get_start_sequence(rollback: &Rollback): u64 {
    rollback.start_sequence
}

public fun get_end_sequence(rollback: &Rollback): u64 {
    rollback.end_sequence
}

public fun has_rollback_sequence(rollback: &Rollback, sequence: u64): bool {
    vec_map::contains(&rollback.rollback_sequences, &sequence)
}
