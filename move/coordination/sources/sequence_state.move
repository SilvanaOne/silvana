module coordination::sequence_state;

use std::string::String;
use sui::clock::Clock;
use sui::event;
use sui::object_table::{Self, ObjectTable};

public struct SequenceState has key, store {
    id: UID,
    sequence: u64,
    state: Option<vector<u8>>,
    data_availability: Option<String>,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
}

public struct SequenceStateManager has key, store {
    id: UID,
    sequence_states: ObjectTable<u64, SequenceState>,
    lowest_sequence: Option<u64>,
    highest_sequence: Option<u64>,
}

// Events
public struct SequenceStateAddedEvent has copy, drop {
    manager_address: address,
    sequence: u64,
    state_data: Option<vector<u8>>,
    data_availability_hash: Option<String>,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
    timestamp: u64,
}

public struct SequenceStateUpdatedEvent has copy, drop {
    manager_address: address,
    sequence: u64,
    new_state_data: Option<vector<u8>>,
    new_data_availability_hash: Option<String>,
    timestamp: u64,
}

public struct SequenceStatesPurgedEvent has copy, drop {
    manager_address: address,
    old_lowest_sequence: u64,
    new_lowest_sequence: u64,
    sequences_purged: u64,
    timestamp: u64,
}

// Error constants
#[error]
const ESequenceStateNotFound: vector<u8> = b"Sequence state not found";

#[error]
const ESequenceTooLow: vector<u8> = b"Sequence is lower than lowest sequence";

#[error]
const ESequenceTooHigh: vector<u8> = b"Sequence is higher than highest sequence";

public(package) fun create_sequence_state(
    sequence: u64,
    state: Option<vector<u8>>,
    data_availability: Option<String>,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
    ctx: &mut TxContext,
): SequenceState {
    SequenceState {
        id: object::new(ctx),
        sequence,
        state,
        data_availability,
        optimistic_state,
        transition_data,
    }
}

public(package) fun sequence(sequence_state: &SequenceState): u64 {
    sequence_state.sequence
}

public(package) fun state(sequence_state: &SequenceState): &Option<vector<u8>> {
    &sequence_state.state
}

public(package) fun data_availability(sequence_state: &SequenceState): &Option<String> {
    &sequence_state.data_availability
}

public(package) fun optimistic_state(sequence_state: &SequenceState): &vector<u8> {
    &sequence_state.optimistic_state
}

public(package) fun transition_data(sequence_state: &SequenceState): &vector<u8> {
    &sequence_state.transition_data
}

public(package) fun set_state(sequence_state: &mut SequenceState, new_state: Option<vector<u8>>) {
    sequence_state.state = new_state;
}

public(package) fun set_data_availability(sequence_state: &mut SequenceState, new_data_availability: Option<String>) {
    sequence_state.data_availability = new_data_availability;
}

public(package) fun destroy_sequence_state(sequence_state: SequenceState): (u64, Option<vector<u8>>, Option<String>, vector<u8>, vector<u8>) {
    let SequenceState { id, sequence, state, data_availability, optimistic_state, transition_data } = sequence_state;
    object::delete(id);
    (sequence, state, data_availability, optimistic_state, transition_data)
}

// SequenceStateManager functions
public(package) fun create_sequence_state_manager(
    ctx: &mut TxContext,
): SequenceStateManager {
    SequenceStateManager {
        id: object::new(ctx),
        sequence_states: object_table::new<u64, SequenceState>(ctx),
        lowest_sequence: option::none(),
        highest_sequence: option::none(),
    }
}

public(package) fun add_state_for_sequence(
    manager: &mut SequenceStateManager,
    sequence: u64,
    state_data: Option<vector<u8>>,
    data_availability_hash: Option<String>,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Validate sequence bounds
    if (manager.highest_sequence.is_none()) {
        // If no bounds exist, sequence must be 0
        assert!(sequence == 0, ESequenceTooHigh);
    } else {
        // Sequence must be exactly highest_sequence + 1
        assert!(sequence == *manager.highest_sequence.borrow() + 1, ESequenceTooHigh);
    };
    
    let timestamp = clock.timestamp_ms();
    let sequence_state = create_sequence_state(
        sequence,
        state_data,
        data_availability_hash,
        optimistic_state,
        transition_data,
        ctx,
    );
    object_table::add(&mut manager.sequence_states, sequence, sequence_state);
    
    // Update bounds
    if (manager.lowest_sequence.is_none() || sequence < *manager.lowest_sequence.borrow()) {
        manager.lowest_sequence = option::some(sequence);
    };
    if (manager.highest_sequence.is_none() || sequence > *manager.highest_sequence.borrow()) {
        manager.highest_sequence = option::some(sequence);
    };
    
    event::emit(SequenceStateAddedEvent {
        manager_address: manager.id.to_address(),
        sequence,
        state_data,
        data_availability_hash,
        optimistic_state,
        transition_data,
        timestamp,
    });
}

public(package) fun update_state_for_sequence(
    manager: &mut SequenceStateManager,
    sequence: u64,
    new_state_data: Option<vector<u8>>,
    new_data_availability_hash: Option<String>,
    clock: &Clock,
) {
    // Validate sequence bounds - for updates, sequence must exist within bounds
    if (manager.lowest_sequence.is_none() || manager.highest_sequence.is_none()) {
        // If no bounds exist, sequence must be 0
        assert!(sequence == 0, ESequenceTooHigh);
    } else {
        // For updates, sequence must be within existing bounds
        assert!(sequence >= *manager.lowest_sequence.borrow(), ESequenceTooLow);
        assert!(sequence <= *manager.highest_sequence.borrow(), ESequenceTooHigh);
    };
    
    let timestamp = clock.timestamp_ms();
    assert!(
        object_table::contains(&manager.sequence_states, sequence),
        ESequenceStateNotFound,
    );
    let sequence_state = object_table::borrow_mut(&mut manager.sequence_states, sequence);
    // Only update state and data_availability, not optimistic_state and transition_data
    set_state(sequence_state, new_state_data);
    set_data_availability(sequence_state, new_data_availability_hash);
    
    event::emit(SequenceStateUpdatedEvent {
        manager_address: manager.id.to_address(),
        sequence,
        new_state_data,
        new_data_availability_hash,
        timestamp,
    });
}

public(package) fun purge(
    manager: &mut SequenceStateManager,
    threshold_sequence: u64,
    clock: &Clock,
) {
    // If lowest_sequence or highest_sequence is None, do nothing
    if (manager.lowest_sequence.is_none() || manager.highest_sequence.is_none()) {
        return
    };
    
    let timestamp = clock.timestamp_ms();
    let old_lowest_sequence = *manager.lowest_sequence.borrow();
    let mut current_sequence = old_lowest_sequence;
    let mut sequences_purged = 0u64;
    let highest_bound = *manager.highest_sequence.borrow();
    
    while (current_sequence < threshold_sequence && current_sequence <= highest_bound) {
        if (object_table::contains(&manager.sequence_states, current_sequence)) {
            let sequence_state = object_table::remove(&mut manager.sequence_states, current_sequence);
            let (_, _, _, _, _) = destroy_sequence_state(sequence_state);
            sequences_purged = sequences_purged + 1;
        };
        current_sequence = current_sequence + 1;
    };
    manager.lowest_sequence = option::some(threshold_sequence);
    
    event::emit(SequenceStatesPurgedEvent {
        manager_address: manager.id.to_address(),
        old_lowest_sequence,
        new_lowest_sequence: threshold_sequence,
        sequences_purged,
        timestamp,
    });
}

// Getter functions
public(package) fun lowest_sequence(manager: &SequenceStateManager): Option<u64> {
    manager.lowest_sequence
}

public(package) fun highest_sequence(manager: &SequenceStateManager): Option<u64> {
    manager.highest_sequence
}

public(package) fun has_sequence_state(manager: &SequenceStateManager, sequence: u64): bool {
    object_table::contains(&manager.sequence_states, sequence)
}

public(package) fun borrow_sequence_state(manager: &SequenceStateManager, sequence: u64): &SequenceState {
    object_table::borrow(&manager.sequence_states, sequence)
}

public(package) fun borrow_sequence_state_mut(manager: &mut SequenceStateManager, sequence: u64): &mut SequenceState {
    object_table::borrow_mut(&mut manager.sequence_states, sequence)
}