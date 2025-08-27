module add::main;

use commitment::action::create_action;
use commitment::state::{
    create_state_update,
    commit_action,
    get_state_element,
    has_state_element,
    get_commitment_data,
    CommitmentData
};
use coordination::app_instance::{AppInstance, AppInstanceCap};
use coordination::registry::{
    SilvanaRegistry,
    create_app_instance_from_registry
};
use std::string::String;
use sui::bcs;
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
    instance_cap: AppInstanceCap,
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
    old_commitment: CommitmentData,
    new_commitment: CommitmentData,
}

public struct ValueMultipliedEvent has copy, drop {
    app_address: address,
    index: u32,
    old_value: u256,
    new_value: u256,
    multiplier: u256,
    old_sum: u256,
    new_sum: u256,
    old_commitment: CommitmentData,
    new_commitment: CommitmentData,
}

const SUM_INDEX: u32 = 0;

// Struct for serializing transition data
public struct TransitionData has copy, drop {
    block_number: u64,
    sequence: u64,
    method: String,
    index: u32,
    value: u256,
    old_value: u256,
    old_commitment: CommitmentData,
    new_commitment: CommitmentData,
}

// Struct for optimistic state
public struct OptimisticState has copy, drop {
    sum: u256,
    commitment: CommitmentData,
}

public fun create_app(
    registry: &mut SilvanaRegistry,
    clock: &Clock,
    ctx: &mut TxContext,
): App {
    // Create an app instance from the registry's SilvanaApp
    // This creates and shares an AppInstance
    let instance_cap = create_app_instance_from_registry(
        registry,
        b"test_app".to_string(),
        option::none(), // description
        clock,
        ctx,
    );

    let app_id = object::new(ctx);
    let app_address = app_id.to_address();

    let app = App {
        id: app_id,
        instance_cap,
    };

    event::emit(AppCreatedEvent {
        app_address,
        initial_sum: 0u256,
        created_at: clock.timestamp_ms(),
        initial_actions_commitment: sui::bls12381::scalar_zero(),
        initial_actions_sequence: 1u64,
        initial_state_commitment: sui::bls12381::scalar_zero(),
    });

    app
}

public fun init_app_with_instance(
    app: &App,
    instance: &mut AppInstance,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Initialize with sum equal to 0 as there are no elements yet
    let action = create_action(b"init".to_string(), vector[]);
    let state_update = create_state_update(SUM_INDEX, vector[0u256]);
    instance
        .state_mut(&app.instance_cap)
        .commit_action(action, &vector[state_update], ctx);

    // Get initial commitment data for sequence 0
    let state = instance.state_mut(&app.instance_cap);
    let initial_commitment = get_commitment_data(state);

    // Create optimistic state for sequence 0
    let optimistic_state_struct = OptimisticState {
        sum: 0u256,
        commitment: initial_commitment,
    };
    let optimistic_state_bytes = bcs::to_bytes(&optimistic_state_struct);

    // Add sequence 0 state to the sequence state manager
    // No transition data for initial state, so use empty vector
    coordination::app_instance::increase_sequence(
        instance,
        optimistic_state_bytes,
        vector[],
        clock,
        ctx,
    );
}

const EInvalidValue: u64 = 0;
const EIndexTooLarge: u64 = 1;
const EReservedIndex: u64 = 2;
const MAX_INDEX: u32 = 1024 * 1024 * 1024;

public fun add(
    app: &mut App,
    instance: &mut AppInstance,
    index: u32,
    value: u256,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    assert!(value < 100, EInvalidValue);
    let old_value = get_value(instance, index);
    let old_sum = get_sum(instance);
    let state = instance.state_mut(&app.instance_cap);

    // Get old commitment data
    let old_commitment = get_commitment_data(state);

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

    // Get new commitment data
    let new_commitment = get_commitment_data(state);

    // Create optimistic state and transition data
    let optimistic_state_struct = OptimisticState {
        sum: new_sum,
        commitment: new_commitment,
    };
    let optimistic_state_bytes = bcs::to_bytes(&optimistic_state_struct);

    let sequence = instance.sequence();
    let block_number = instance.block_number();
    let transition_data_struct = TransitionData {
        block_number,
        sequence,
        method: b"add".to_string(),
        index,
        value,
        old_value,
        old_commitment,
        new_commitment,
    };
    let transition_data_bytes = bcs::to_bytes(&transition_data_struct);

    coordination::app_instance::create_app_job(
        instance,
        b"add".to_string(),
        option::some(b"Add operation job".to_string()),
        option::some(block_number), // block_number from instance
        option::some(vector[sequence]),
        option::none(), // sequences1
        option::none(), // sequences2
        transition_data_bytes,
        clock,
        ctx,
    );

    // Emit event for prover
    event::emit(ValueAddedEvent {
        app_address: app.id.to_address(),
        index,
        old_value,
        new_value,
        amount_added: value,
        old_sum,
        new_sum,
        old_commitment,
        new_commitment,
    });
    coordination::app_instance::increase_sequence(
        instance,
        optimistic_state_bytes,
        transition_data_bytes,
        clock,
        ctx,
    );
}

public fun multiply(
    app: &mut App,
    instance: &mut AppInstance,
    index: u32,
    value: u256,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    assert!(value < 100, EInvalidValue);
    let old_value = get_value(instance, index);
    let old_sum = get_sum(instance);
    let state = instance.state_mut(&app.instance_cap);

    // Get old commitment data
    let old_commitment = get_commitment_data(state);

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

    // Get new commitment data
    let new_commitment = get_commitment_data(state);

    // Create optimistic state and transition data
    let optimistic_state_struct = OptimisticState {
        sum: new_sum,
        commitment: new_commitment,
    };
    let optimistic_state_bytes = bcs::to_bytes(&optimistic_state_struct);

    let sequence = instance.sequence();
    let block_number = instance.block_number();
    let transition_data_struct = TransitionData {
        block_number,
        sequence,
        method: b"multiply".to_string(),
        index,
        value,
        old_value,
        old_commitment,
        new_commitment,
    };
    let transition_data_bytes = bcs::to_bytes(&transition_data_struct);

    coordination::app_instance::create_app_job(
        instance,
        b"multiply".to_string(),
        option::some(b"Multiply operation job".to_string()),
        option::some(block_number), // block_number from instance
        option::some(vector[sequence]),
        option::none(), // sequences1
        option::none(), // sequences2
        transition_data_bytes,
        clock,
        ctx,
    );

    // Emit event for prover
    event::emit(ValueMultipliedEvent {
        app_address: app.id.to_address(),
        index,
        old_value,
        new_value,
        multiplier: value,
        old_sum,
        new_sum,
        old_commitment,
        new_commitment,
    });
    coordination::app_instance::increase_sequence(
        instance,
        optimistic_state_bytes,
        transition_data_bytes,
        clock,
        ctx,
    );
}

public fun get_value(instance: &AppInstance, index: u32): u256 {
    assert!(index > SUM_INDEX, EReservedIndex);
    assert!(index < MAX_INDEX, EIndexTooLarge);
    let state = instance.state();
    if (has_state_element(state, index)) {
        *get_state_element(state, index).get_state_element_state().borrow(0)
    } else {
        0u256
    }
}

public struct GetSumEvent has copy, drop {
    sum: u256,
}

public fun get_sum(instance: &AppInstance): u256 {
    let state = instance.state();
    let sum =
        *get_state_element(state, SUM_INDEX)
            .get_state_element_state()
            .borrow(0);
    event::emit(GetSumEvent { sum });
    sum
}

public fun purge_rollback_records(
    app: &mut App,
    instance: &mut AppInstance,
    proved_sequence: u64,
    clock: &Clock,
) {
    let state = instance.state_mut(&app.instance_cap);
    let rollback = state.get_rollback_mut();
    commitment::rollback::purge_records(rollback, proved_sequence);

    // Purge sequence states for sequences older than proved_sequence - 10
    if (proved_sequence > 10) {
        let threshold_sequence = proved_sequence - 10;
        coordination::app_instance::purge_sequences_below(
            instance,
            threshold_sequence,
            clock,
        );
    };
}
