module commitment::rollback;

use commitment::action::Action;
use std::hash::sha2_256;
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

public struct RollbackSequence has key, store {
    id: UID,
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
    rollback_id: address,
    sequence_id: address,
    sequence: u64,
    action: Action,
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
const ESequenceNotFound: u64 = 2;
const EInvalidPurgeSequence: u64 = 3;

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
    ctx: &mut TxContext,
) {
    assert!(sequence == rollback.end_sequence + 1, EInvalidSequence);

    let rollback_sequence_id = object::new(ctx);
    let sequence_address = rollback_sequence_id.to_address();

    let rollback_sequence = RollbackSequence {
        id: rollback_sequence_id,
        sequence,
        action,
        elements,
    };

    object_table::add(
        &mut rollback.rollback_sequences,
        sequence,
        rollback_sequence,
    );
    rollback.end_sequence = sequence;

    event::emit(RollbackSequenceAddedEvent {
        rollback_id: rollback.id.to_address(),
        sequence_id: sequence_address,
        sequence,
        action,
        elements_count: elements.length(),
        new_end_sequence: rollback.end_sequence,
    });
}

/// Purge records by removing the records from start_sequence to proved_sequence
public fun purge_records(rollback: &mut Rollback, proved_sequence: u64) {
    assert!(proved_sequence >= rollback.start_sequence, EInvalidPurgeSequence);
    assert!(proved_sequence <= rollback.end_sequence, EInvalidPurgeSequence);

    let old_start_sequence = rollback.start_sequence;
    let old_traced_sequence = rollback.traced_sequence;

    let mut current_sequence = rollback.start_sequence;
    while (current_sequence <= proved_sequence) {
        if (
            object_table::contains(
                &rollback.rollback_sequences,
                current_sequence,
            )
        ) {
            let RollbackSequence {
                id,
                sequence: _,
                action: _,
                elements: _,
            } = object_table::remove(
                &mut rollback.rollback_sequences,
                current_sequence,
            );
            object::delete(id);
        };
        current_sequence = current_sequence + 1;
    };

    rollback.start_sequence = proved_sequence + 1;

    // Update traced_sequence if it was purged
    if (rollback.traced_sequence <= proved_sequence) {
        rollback.traced_sequence = rollback.start_sequence;
    };

    event::emit(RecordsPurgedEvent {
        rollback_id: rollback.id.to_address(),
        purged_from_sequence: old_start_sequence,
        purged_to_sequence: proved_sequence,
        new_start_sequence: rollback.start_sequence,
        old_traced_sequence: old_traced_sequence,
        new_traced_sequence: rollback.traced_sequence,
    });
}

/// Set traced_sequence checking that it is between start_sequence and end_sequence
public fun set_traced_sequence(rollback: &mut Rollback, traced_sequence: u64) {
    assert!(traced_sequence >= rollback.start_sequence, EInvalidTracedSequence);
    assert!(traced_sequence <= rollback.end_sequence, EInvalidTracedSequence);

    let old_traced_sequence = rollback.traced_sequence;
    rollback.traced_sequence = traced_sequence;

    event::emit(TracedSequenceSetEvent {
        rollback_id: rollback.id.to_address(),
        old_traced_sequence,
        new_traced_sequence: traced_sequence,
    });
}

// Getter functions
public fun get_start_sequence(rollback: &Rollback): u64 {
    rollback.start_sequence
}

public fun get_traced_sequence(rollback: &Rollback): u64 {
    rollback.traced_sequence
}

public fun get_end_sequence(rollback: &Rollback): u64 {
    rollback.end_sequence
}

public fun get_rollback_sequence(
    rollback: &Rollback,
    sequence: u64,
): &RollbackSequence {
    assert!(
        object_table::contains(&rollback.rollback_sequences, sequence),
        ESequenceNotFound,
    );
    object_table::borrow(&rollback.rollback_sequences, sequence)
}

public fun has_rollback_sequence(rollback: &Rollback, sequence: u64): bool {
    object_table::contains(&rollback.rollback_sequences, sequence)
}

public fun get_rollback_sequence_elements(
    rollback_sequence: &RollbackSequence,
): &vector<RollbackElement> {
    &rollback_sequence.elements
}

public fun get_rollback_sequence_number(
    rollback_sequence: &RollbackSequence,
): u64 {
    rollback_sequence.sequence
}

// RollbackElement getter functions
public fun get_element_index(element: &RollbackElement): u32 {
    element.index
}

public fun get_element_previous_state(
    element: &RollbackElement,
): Option<vector<u256>> {
    element.previous_state
}

public fun get_element_new_state(element: &RollbackElement): &vector<u256> {
    &element.new_state
}

public fun get_element_commitment_before(
    element: &RollbackElement,
): &Element<Scalar> {
    &element.commitment_before
}

public fun get_element_commitment_after(
    element: &RollbackElement,
): &Element<Scalar> {
    &element.commitment_after
}
