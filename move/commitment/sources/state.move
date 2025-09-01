module commitment::state;

use commitment::action::Action;
use commitment::actions::ActionsCommitment;
use commitment::commitment::{digest_fields, update};
use commitment::rollback::{
    Rollback,
    RollbackElement,
    create_rollback,
    add_rollback_sequence,
    create_rollback_element
};
use sui::bls12381::{Scalar, scalar_zero};
use sui::event;
use sui::group_ops::Element;
use sui::object_table::{Self, ObjectTable};

public struct StateElement has key, store {
    id: UID,
    state: vector<u256>,
}

public struct StateUpdate has copy, drop, store {
    index: u32,
    new_state: vector<u256>,
}

public struct AppState has key, store {
    id: UID,
    sequence: u64,
    actions_commitment: ActionsCommitment,
    state: ObjectTable<u32, StateElement>,
    state_commitment: Element<Scalar>,
    rollback: Rollback, // TODO: implement rollback calls
}

public struct AppStateCreatedEvent has copy, drop {
    id: address,
    sequence: u64,
    state_commitment: Element<Scalar>,
}

public struct StateUpdateCreatedEvent has copy, drop {
    index: u32,
    new_state: vector<u256>,
    state_length: u64,
}

public struct StateActionCommittedEvent has copy, drop {
    app_state_id: address,
    action: Action,
    old_sequence: u64,
    new_sequence: u64,
    state_updates_count: u64,
    old_state_commitment: Element<Scalar>,
    new_state_commitment: Element<Scalar>,
}

public struct StateElementUpdatedEvent has copy, drop {
    app_state_id: address,
    element_index: u32,
    previous_state: Option<vector<u256>>,
    new_state: vector<u256>,
    previous_commitment: Element<Scalar>,
    new_commitment: Element<Scalar>,
}

public struct CommitmentData has copy, drop {
    actions_commitment: Element<Scalar>,
    actions_sequence: u64,
    state_commitment: Element<Scalar>,
}

/// Create a new AppState
public fun create_app_state(ctx: &mut TxContext): AppState {
    let app_state_id = object::new(ctx);
    let address = app_state_id.to_address();
    let initial_state_commitment = scalar_zero();

    let app_state = AppState {
        id: app_state_id,
        sequence: 0,
        actions_commitment: commitment::actions::create_actions_commitment(ctx),
        state: object_table::new<u32, StateElement>(ctx),
        state_commitment: initial_state_commitment,
        rollback: create_rollback(ctx),
    };

    event::emit(AppStateCreatedEvent {
        id: address,
        sequence: 0,
        state_commitment: initial_state_commitment,
    });

    app_state
}

/// Create a new StateUpdate
public fun create_state_update(
    index: u32,
    new_state: vector<u256>,
): StateUpdate {
    let state_update = StateUpdate {
        index,
        new_state,
    };

    event::emit(StateUpdateCreatedEvent {
        index,
        new_state,
        state_length: new_state.length(),
    });

    state_update
}

const EInvalidSequence: u64 = 0;

public fun commit_action(
    app_state: &mut AppState,
    action: Action,
    state_updates: &vector<StateUpdate>,
    ctx: &mut TxContext,
) {
    let old_sequence = app_state.sequence;
    let old_state_commitment = app_state.state_commitment;

    app_state
        .actions_commitment
        .commit_action(&action.calculate_action_commitment());
    let mut rollback_elements: vector<RollbackElement> = vector::empty();
    let new_sequence = app_state.sequence + 1;
    assert!(
        app_state
        .actions_commitment.get_sequence() == new_sequence,
        EInvalidSequence,
    );
    let mut i = 0;
    while (i < state_updates.length()) {
        let state_update = vector::borrow(state_updates, i);
        let new_state_element_commitment = digest_fields(
            &state_update.new_state,
        );
        let (previous_state_element_commitment, previous_state) = if (
            app_state.state.contains(state_update.index)
        ) {
            (
                digest_fields(
                    &app_state.state.borrow(state_update.index).state,
                ),
                option::some(app_state.state.borrow(state_update.index).state),
            )
        } else {
            (scalar_zero(), option::none())
        };

        let updated_state_commitment = update(
            &app_state.state_commitment,
            &previous_state_element_commitment,
            &new_state_element_commitment,
            state_update.index,
        );
        let rollback_element = create_rollback_element(
            state_update.index,
            previous_state,
            state_update.new_state,
            &app_state.state_commitment,
            &updated_state_commitment,
        );
        vector::push_back(&mut rollback_elements, rollback_element);

        // Emit event for individual state element update
        event::emit(StateElementUpdatedEvent {
            app_state_id: app_state.id.to_address(),
            element_index: state_update.index,
            previous_state: previous_state,
            new_state: state_update.new_state,
            previous_commitment: previous_state_element_commitment,
            new_commitment: new_state_element_commitment,
        });
        if (app_state.state.contains(state_update.index)) {
            app_state.state.borrow_mut(state_update.index).state =
                state_update.new_state;
        } else {
            app_state
                .state
                .add(
                    state_update.index,
                    StateElement {
                        id: object::new(ctx),
                        state: state_update.new_state,
                    },
                );
        };
        app_state.state_commitment = updated_state_commitment;
        i = i + 1;
    };
    app_state
        .rollback
        .add_rollback_sequence(
            new_sequence,
            action,
            rollback_elements,
        );
    app_state.sequence = new_sequence;

    // Emit event for the overall action commitment
    event::emit(StateActionCommittedEvent {
        app_state_id: app_state.id.to_address(),
        action,
        old_sequence,
        new_sequence: app_state.sequence,
        state_updates_count: state_updates.length(),
        old_state_commitment,
        new_state_commitment: app_state.state_commitment,
    });
}

// Getter functions
public fun get_sequence(app_state: &AppState): u64 {
    app_state.sequence
}

public fun get_state_commitment(app_state: &AppState): Element<Scalar> {
    app_state.state_commitment
}

public fun get_actions_commitment(app_state: &AppState): &ActionsCommitment {
    &app_state.actions_commitment
}

public fun get_rollback(app_state: &AppState): &Rollback {
    &app_state.rollback
}

public fun get_rollback_mut(app_state: &mut AppState): &mut Rollback {
    &mut app_state.rollback
}

public fun get_state_element(app_state: &AppState, index: u32): &StateElement {
    app_state.state.borrow(index)
}

public fun has_state_element(app_state: &AppState, index: u32): bool {
    app_state.state.contains(index)
}

public fun get_state_element_state(
    state_element: &StateElement,
): &vector<u256> {
    &state_element.state
}

public fun get_state_update_index(state_update: &StateUpdate): u32 {
    state_update.index
}

public fun get_state_update_new_state(
    state_update: &StateUpdate,
): &vector<u256> {
    &state_update.new_state
}

public fun get_commitment_data(app_state: &AppState): CommitmentData {
    CommitmentData {
        actions_commitment: app_state.actions_commitment.get_commitment(),
        actions_sequence: app_state.actions_commitment.get_sequence(),
        state_commitment: app_state.state_commitment,
    }
}

public fun get_commitment_data_actions_commitment(
    commitment_data: &CommitmentData,
): Element<Scalar> {
    commitment_data.actions_commitment
}

public fun get_commitment_data_actions_sequence(
    commitment_data: &CommitmentData,
): u64 {
    commitment_data.actions_sequence
}

public fun get_commitment_data_state_commitment(
    commitment_data: &CommitmentData,
): Element<Scalar> {
    commitment_data.state_commitment
}
