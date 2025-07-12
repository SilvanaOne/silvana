module commitment::rollback;

use sui::bls12381::Scalar;
use sui::group_ops::Element;
use sui::object_table::{Self, ObjectTable};

public struct RollbackElement has copy, drop, store {
    index: u32,
    previous_state: vector<u256>,
    new_state: vector<u256>,
    commitment_before: Element<Scalar>,
    commitment_after: Element<Scalar>,
}

public struct RollbackSequence has key, store {
    id: UID,
    sequence: u64,
    elements: vector<RollbackElement>,
}

public struct Rollback has key, store {
    id: UID,
    start_sequence: u64,
    traced_sequence: u64,
    end_sequence: u64,
    rollback_sequences: ObjectTable<u64, RollbackSequence>,
}

// Error constants
const EInvalidSequence: u64 = 1;
const ESequenceNotFound: u64 = 2;
const EInvalidTracedSequence: u64 = 3;
const EInvalidPurgeSequence: u64 = 4;

/// Create Rollback
public fun create_rollback(start_sequence: u64, ctx: &mut TxContext): Rollback {
    Rollback {
        id: object::new(ctx),
        start_sequence,
        traced_sequence: start_sequence,
        end_sequence: start_sequence,
        rollback_sequences: object_table::new(ctx),
    }
}

/// Add rollback sequence, checking that sequence is end_sequence + 1
public fun add_rollback_sequence(
    rollback: &mut Rollback,
    sequence: u64,
    elements: vector<RollbackElement>,
    ctx: &mut TxContext,
) {
    assert!(sequence == rollback.end_sequence + 1, EInvalidSequence);

    let rollback_sequence = RollbackSequence {
        id: object::new(ctx),
        sequence,
        elements,
    };

    object_table::add(
        &mut rollback.rollback_sequences,
        sequence,
        rollback_sequence,
    );
    rollback.end_sequence = sequence;
}

/// Purge records by removing the records from start_sequence to proved_sequence
public fun purge_records(rollback: &mut Rollback, proved_sequence: u64) {
    assert!(proved_sequence >= rollback.start_sequence, EInvalidPurgeSequence);
    assert!(proved_sequence <= rollback.end_sequence, EInvalidPurgeSequence);

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
}

/// Set traced_sequence checking that it is between start_sequence and end_sequence
public fun set_traced_sequence(rollback: &mut Rollback, traced_sequence: u64) {
    assert!(traced_sequence >= rollback.start_sequence, EInvalidTracedSequence);
    assert!(traced_sequence <= rollback.end_sequence, EInvalidTracedSequence);

    rollback.traced_sequence = traced_sequence;
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
): &vector<u256> {
    &element.previous_state
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
