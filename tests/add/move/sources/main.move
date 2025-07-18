module app::main;

use commitment::action::create_action;
use commitment::state::{
    create_state_update,
    commit_action,
    get_state_element,
    has_state_element
};
use coordination::app_instance::{AppInstance, create_app_instance};
use sui::bls12381::Scalar;
use sui::clock::Clock;
use sui::event;
use sui::group_ops::Element;

/// App keeps indexed elements that can be added and multiplied using values
/// less than 100.
/// Index 0 is used to keep the total sum of all elements.
/// Indexes 1-(1024*1024*1024-1) are used to keep the elements.
/// TODO: check overflows or add Field arithmetic or add Rollback support

public struct App has key, store {
    id: UID,
    instance: AppInstance,
}

// Events
public struct AppCreatedEvent has copy, drop {
    app_address: address,
    initial_sum: u256,
    initial_actions_commitment: Element<Scalar>,
    initial_actions_sequence: u64,
    initial_state_commitment: Element<Scalar>,
    created_at: u64,
}

public struct ValueAddedEvent has copy, drop {
    app_address: address,
    index: u32,
    old_value: u256,
    new_value: u256,
    amount_added: u256,
    old_sum: u256,
    new_sum: u256,
    old_actions_commitment: Element<Scalar>,
    old_actions_sequence: u64,
    old_state_commitment: Element<Scalar>,
    new_actions_commitment: Element<Scalar>,
    new_actions_sequence: u64,
    new_state_commitment: Element<Scalar>,
}

public struct ValueMultipliedEvent has copy, drop {
    app_address: address,
    index: u32,
    old_value: u256,
    new_value: u256,
    multiplier: u256,
    old_sum: u256,
    new_sum: u256,
    old_actions_commitment: Element<Scalar>,
    old_actions_sequence: u64,
    old_state_commitment: Element<Scalar>,
    new_actions_commitment: Element<Scalar>,
    new_actions_sequence: u64,
    new_state_commitment: Element<Scalar>,
}

const SUM_INDEX: u32 = 0;

public fun create_app(clock: &Clock, ctx: &mut TxContext): App {
    let mut instance = create_app_instance(
        b"test_app".to_string(),
        option::none(),
        option::none(),
        clock,
        ctx,
    );
    // Initialize with sum equal to 0 as there are no elements yet
    let action = create_action(b"init".to_string(), vector[]);
    let state_update = create_state_update(SUM_INDEX, vector[0u256]);
    instance.state_mut().commit_action(action, &vector[state_update], ctx);
    let actions_commitment_data = instance.state().get_actions_commitment();
    let actions_commitment = actions_commitment_data.get_commitment();
    let actions_sequence = actions_commitment_data.get_sequence();
    let state_commitment = instance.state().get_state_commitment();

    let app_id = object::new(ctx);
    let app_address = app_id.to_address();

    let app = App {
        id: app_id,
        instance,
    };

    event::emit(AppCreatedEvent {
        app_address,
        initial_sum: 0u256,
        created_at: clock.timestamp_ms(),
        initial_actions_commitment: actions_commitment,
        initial_actions_sequence: actions_sequence,
        initial_state_commitment: state_commitment,
    });

    app
}

const EInvalidValue: u64 = 0;
const EIndexTooLarge: u64 = 1;
const EReservedIndex: u64 = 2;
const MAX_INDEX: u32 = 1024 * 1024 * 1024;

public fun add(app: &mut App, index: u32, value: u256, ctx: &mut TxContext) {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    assert!(value < 100, EInvalidValue);
    let old_value = get_value(app, index);
    let old_sum = get_sum(app);
    let state = app.instance.state_mut();

    // Get old commitment
    let old_actions_commitment_data = state.get_actions_commitment();
    let old_actions_commitment = old_actions_commitment_data.get_commitment();
    let old_actions_sequence = old_actions_commitment_data.get_sequence();
    let old_state_commitment = state.get_state_commitment();

    // Create action
    let action = create_action(
        b"add".to_string(),
        vector[index as u256, value],
    );
    let new_value = old_value + value;
    let new_sum = old_sum + value;
    let state_update_value = create_state_update(index, vector[new_value]);
    let state_update_sum = create_state_update(SUM_INDEX, vector[new_sum]);

    // Commit action
    state.commit_action(
        action,
        &vector[state_update_value, state_update_sum],
        ctx,
    );

    // Get new commitment
    let new_actions_commitment_data = state.get_actions_commitment();
    let new_actions_commitment = new_actions_commitment_data.get_commitment();
    let new_actions_sequence = new_actions_commitment_data.get_sequence();
    let new_state_commitment = state.get_state_commitment();

    // Emit event for prover
    event::emit(ValueAddedEvent {
        app_address: app.id.to_address(),
        index,
        old_value,
        new_value,
        amount_added: value,
        old_sum,
        new_sum,
        old_actions_commitment,
        old_actions_sequence,
        old_state_commitment,
        new_actions_commitment,
        new_actions_sequence,
        new_state_commitment,
    });
}

public fun multiply(
    app: &mut App,
    index: u32,
    value: u256,
    ctx: &mut TxContext,
) {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    assert!(value < 100, EInvalidValue);
    let old_value = get_value(app, index);
    let old_sum = get_sum(app);
    let state = app.instance.state_mut();

    // Get old commitment
    let old_actions_commitment_data = state.get_actions_commitment();
    let old_actions_commitment = old_actions_commitment_data.get_commitment();
    let old_actions_sequence = old_actions_commitment_data.get_sequence();
    let old_state_commitment = state.get_state_commitment();

    // Create action
    let action = create_action(
        b"multiply".to_string(),
        vector[index as u256, value],
    );
    let new_value = old_value * value;
    let new_sum = old_sum  +  new_value - old_value;
    let state_update_value = create_state_update(index, vector[new_value]);
    let state_update_sum = create_state_update(SUM_INDEX, vector[new_sum]);

    // Commit action
    state.commit_action(
        action,
        &vector[state_update_value, state_update_sum],
        ctx,
    );

    // Get new commitment
    let new_actions_commitment_data = state.get_actions_commitment();
    let new_actions_commitment = new_actions_commitment_data.get_commitment();
    let new_actions_sequence = new_actions_commitment_data.get_sequence();
    let new_state_commitment = state.get_state_commitment();

    // Emit event for prover
    event::emit(ValueMultipliedEvent {
        app_address: app.id.to_address(),
        index,
        old_value,
        new_value,
        multiplier: value,
        old_sum,
        new_sum,
        old_actions_commitment,
        old_actions_sequence,
        old_state_commitment,
        new_actions_commitment,
        new_actions_sequence,
        new_state_commitment,
    });
}

public fun get_value(app: &App, index: u32): u256 {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    let state = app.instance.state();
    if (has_state_element(state, index)) {
        *get_state_element(state, index).get_state_element_state().borrow(0)
    } else {
        0u256
    }
}

public struct GetSumEvent has copy, drop {
    sum: u256,
}

public fun get_sum(app: &App): u256 {
    let state = app.instance.state();
    let sum =
        *get_state_element(state, SUM_INDEX)
            .get_state_element_state()
            .borrow(0);
    event::emit(GetSumEvent { sum });
    sum
}

public fun purge_rollback_records(app: &mut App, proved_sequence: u64) {
    let state = app.instance.state_mut();
    let rollback = state.get_rollback_mut();
    commitment::rollback::purge_records(rollback, proved_sequence);
}
