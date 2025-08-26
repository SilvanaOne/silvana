module coordination::app_instance;

use commitment::state::{AppState, get_state_commitment, create_app_state};
use coordination::app_method::{Self, AppMethod};
use coordination::block::{Self, Block};
use coordination::jobs::{Self, Jobs};
use coordination::prover::{Self, ProofCalculation};
use coordination::sequence_state::{Self, SequenceState, SequenceStateManager};
use coordination::silvana_app::{Self, SilvanaApp};
use std::string::String;
use sui::bls12381::{Scalar, scalar_zero};
use sui::clock::{timestamp_ms, Clock};
use sui::display;
use sui::event;
use sui::group_ops::Element;
use sui::object_table::{Self, ObjectTable, add, borrow_mut, borrow};
use sui::package;
use sui::vec_map::{Self, VecMap};

public struct AppInstance has key, store {
    id: UID,
    silvana_app_name: String,
    description: Option<String>,
    metadata: Option<String>,
    methods: VecMap<String, AppMethod>,
    state: AppState,
    blocks: ObjectTable<u64, Block>,
    proof_calculations: ObjectTable<u64, ProofCalculation>,
    sequence_state_manager: SequenceStateManager,
    jobs: Jobs,
    sequence: u64,
    admin: address,
    block_number: u64,
    previous_block_timestamp: u64,
    previous_block_last_sequence: u64,
    previous_block_actions_state: Element<Scalar>,
    last_proved_block_number: u64,
    isPaused: bool,
    created_at: u64,
    updated_at: u64,
}

public struct AppInstanceCap has key, store {
    id: UID,
    instance_address: address,
}

public struct AppInstanceCreatedEvent has copy, drop {
    app_instance_address: address,
    admin: address,
    created_at: u64,
}

public struct BlockCreatedEvent has copy, drop {
    app_instance_address: address,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    timestamp: u64,
}

public struct NoNewSequencesEvent has copy, drop {
    app_instance_address: address,
    current_sequence: u64,
    previous_block_last_sequence: u64,
    timestamp: u64,
}

public struct InsufficientTimeForBlockEvent has copy, drop {
    app_instance_address: address,
    current_time: u64,
    previous_block_timestamp: u64,
    time_since_last_block: u64,
    min_time_required: u64,
}

public struct APP_INSTANCE has drop {}

fun init(otw: APP_INSTANCE, ctx: &mut TxContext) {
    let publisher = package::claim(otw, ctx);

    let app_instance_keys = vector[b"name".to_string()];

    let app_instance_values = vector[b"Silvana App Instance".to_string()];
    let mut display_app_instance = display::new_with_fields<AppInstance>(
        &publisher,
        app_instance_keys,
        app_instance_values,
        ctx,
    );

    display_app_instance.update_version();
    transfer::public_transfer(publisher, ctx.sender());
    transfer::public_transfer(display_app_instance, ctx.sender());
}

public fun create_app_instance(
    app: &mut SilvanaApp,
    instance_description: Option<String>,
    instance_metadata: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
): AppInstanceCap {
    let timestamp = clock.timestamp_ms();

    let blocks = object_table::new<u64, Block>(ctx);

    let (proof_calculation_block_1, _) = prover::create_block_proof_calculation(
        1u64,
        1u64,
        option::none(),
        clock,
        ctx,
    );
    let mut proof_calculations = object_table::new<
        u64,
        prover::ProofCalculation,
    >(ctx);
    proof_calculations.add(1u64, proof_calculation_block_1);

    let sequence_state_manager = sequence_state::create_sequence_state_manager(
        ctx,
    );

    let state = create_app_state(ctx);
    let jobs = jobs::create_jobs(option::none(), ctx);

    // Clone methods from the SilvanaApp
    let methods = silvana_app::clone_app_methods(app);
    let app_name = silvana_app::app_name(app);
    let instance_id = object::new(ctx);
    let instance_address = instance_id.to_address();

    let mut app_instance: AppInstance = AppInstance {
        id: instance_id,
        silvana_app_name: app_name,
        description: instance_description,
        metadata: instance_metadata,
        methods,
        state,
        blocks,
        proof_calculations,
        sequence_state_manager,
        jobs,
        sequence: 0u64,
        admin: ctx.sender(),
        block_number: 1u64,
        previous_block_timestamp: timestamp,
        previous_block_last_sequence: 0u64,
        previous_block_actions_state: scalar_zero(),
        last_proved_block_number: 0u64,
        isPaused: false,
        created_at: timestamp,
        updated_at: timestamp,
    };

    let (_block_timestamp, block_0) = create_block_internal(
        &app_instance,
        0u64,
        0u64,
        0u64,
        scalar_zero(),
        scalar_zero(),
        option::none(),
        clock,
        ctx,
    );
    app_instance.blocks.add(0u64, block_0);

    // Track the instance in the SilvanaApp
    // Pass @0x0 as admin_address since this is the app owner creating their own instance
    silvana_app::add_instance_to_app(app, instance_address, @0x0, ctx);

    event::emit(AppInstanceCreatedEvent {
        app_instance_address: instance_address,
        admin: app_instance.admin,
        created_at: timestamp,
    });

    transfer::share_object(app_instance);

    AppInstanceCap {
        id: object::new(ctx),
        instance_address,
    }
}

// Error codes
#[error]
const ENotAuthorized: vector<u8> = b"Not authorized";

public(package) fun only_admin(app_instance: &AppInstance, ctx: &TxContext) {
    assert!(app_instance.admin == ctx.sender(), ENotAuthorized);
}

#[error]
const EAppInstancePaused: vector<u8> = b"App instance is paused";

public(package) fun not_paused(app_instance: &AppInstance) {
    assert!(app_instance.isPaused == false, EAppInstancePaused);
}

const MIN_TIME_BETWEEN_BLOCKS: u64 = 60_000;

public fun increase_sequence(
    app_instance: &mut AppInstance,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Add new sequence state
    sequence_state::add_state_for_sequence(
        &mut app_instance.sequence_state_manager,
        app_instance.sequence,
        option::none(), // state is None initially
        option::none(), // data_availability is None initially
        optimistic_state,
        transition_data,
        clock,
        ctx,
    );
    app_instance.sequence = app_instance.sequence + 1;

    try_create_block(app_instance, clock, ctx);
}

public fun try_create_block(
    app_instance: &mut AppInstance,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let current_time = clock.timestamp_ms();

    if (
        (app_instance.previous_block_last_sequence + 1) == app_instance.sequence
    ) {
        event::emit(NoNewSequencesEvent {
            app_instance_address: app_instance.id.to_address(),
            current_sequence: app_instance.sequence,
            previous_block_last_sequence: app_instance.previous_block_last_sequence,
            timestamp: current_time,
        });
        return
    };

    if (
        current_time - app_instance.previous_block_timestamp <= MIN_TIME_BETWEEN_BLOCKS
    ) {
        event::emit(InsufficientTimeForBlockEvent {
            app_instance_address: app_instance.id.to_address(),
            current_time,
            previous_block_timestamp: app_instance.previous_block_timestamp,
            time_since_last_block: current_time - app_instance.previous_block_timestamp,
            min_time_required: MIN_TIME_BETWEEN_BLOCKS,
        });
        return
    };
    let mut block_number = app_instance.block_number;
    let start_sequence = app_instance.previous_block_last_sequence + 1;
    let end_sequence = app_instance.sequence - 1;
    let proof_calculation = borrow_mut(
        &mut app_instance.proof_calculations,
        block_number,
    );
    let finished = prover::set_end_sequence(
        proof_calculation,
        end_sequence,
        clock,
        ctx,
    );
    if (finished) {
        if (app_instance.last_proved_block_number == block_number - 1) {
            app_instance.last_proved_block_number = block_number;
        };
    };

    let (timestamp, block) = create_block_internal(
        app_instance,
        block_number,
        start_sequence,
        end_sequence,
        get_state_commitment(&app_instance.state),
        app_instance.previous_block_actions_state,
        option::some(app_instance.previous_block_timestamp),
        clock,
        ctx,
    );
    add(
        &mut app_instance.blocks,
        block_number,
        block,
    );
    block_number = block_number + 1;
    let (new_proof_calculation, _) = prover::create_block_proof_calculation(
        block_number,
        app_instance.sequence,
        option::none(),
        clock,
        ctx,
    );
    app_instance.block_number = block_number;
    app_instance.previous_block_timestamp = timestamp;
    app_instance.previous_block_actions_state =
        get_state_commitment(&app_instance.state);
    app_instance.previous_block_last_sequence = end_sequence;
    add(
        &mut app_instance.proof_calculations,
        block_number,
        new_proof_calculation,
    );

    event::emit(BlockCreatedEvent {
        app_instance_address: app_instance.id.to_address(),
        block_number: app_instance.block_number - 1, // Use the created block number, not the new one
        start_sequence,
        end_sequence,
        timestamp,
    });
}

public fun start_proving(
    app_instance: &mut AppInstance,
    block_number: u64,
    sequences: vector<u64>, // should be sorted
    merged_sequences_1: Option<vector<u64>>,
    merged_sequences_2: Option<vector<u64>>,
    job_id: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let proof_calculation = borrow_mut(
        &mut app_instance.proof_calculations,
        block_number,
    );
    prover::start_proving(
        proof_calculation,
        sequences,
        merged_sequences_1,
        merged_sequences_2,
        job_id,
        clock,
        ctx,
    );
}

public fun submit_proof(
    app_instance: &mut AppInstance,
    block_number: u64,
    sequences: vector<u64>, // should be sorted
    merged_sequences_1: Option<vector<u64>>,
    merged_sequences_2: Option<vector<u64>>,
    job_id: String,
    da_hash: String,
    cpu_cores: u8,
    prover_architecture: String,
    prover_memory: u64,
    cpu_time: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let proof_calculation = borrow_mut(
        &mut app_instance.proof_calculations,
        block_number,
    );
    let finished = prover::submit_proof(
        proof_calculation,
        sequences,
        merged_sequences_1,
        merged_sequences_2,
        job_id,
        da_hash,
        cpu_cores,
        prover_architecture,
        prover_memory,
        cpu_time,
        clock,
        ctx,
    );
    if (finished) {
        let mut i = app_instance.last_proved_block_number + 1;
        while (i < app_instance.block_number) {
            let proof_calculation = borrow(
                &app_instance.proof_calculations,
                i,
            );
            if (prover::is_finished(proof_calculation)) {
                app_instance.last_proved_block_number = i;
            } else return;
            i = i + 1;
        };
    };
}

public fun reject_proof(
    app_instance: &mut AppInstance,
    block_number: u64,
    sequences: vector<u64>, // should be sorted
    clock: &Clock,
    ctx: &mut TxContext,
) {
    let proof_calculation = borrow_mut(
        &mut app_instance.proof_calculations,
        block_number,
    );
    prover::reject_proof(
        proof_calculation,
        sequences,
        clock,
        ctx,
    );
}

#[allow(lint(self_transfer))]
public(package) fun create_block_internal(
    app_instance: &AppInstance,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    actions_commitment: Element<Scalar>,
    previous_block_actions_commitment: Element<Scalar>,
    previous_block_timestamp: Option<u64>,
    clock: &Clock,
    ctx: &mut TxContext,
): (u64, Block) {
    let timestamp = clock.timestamp_ms();
    let mut name: String = b"Silvana App Instance Block ".to_string();
    name.append(block_number.to_string());
    let mut block_state_name: String = b"Silvana App InstanceBlock ".to_string();
    block_state_name.append(block_number.to_string());
    block_state_name.append(b" State".to_string());

    // let block_commitment = create_block_commitment(
    //     id: object::new(ctx),
    //     name: block_state_name,
    //     block_number: block_number,
    //     sequence: end_sequence,
    //     users: *addresses,
    //     state,
    // };

    let time_since_last_block = if (previous_block_timestamp.is_some()) {
        timestamp - *previous_block_timestamp.borrow()
    } else {
        0u64
    };
    let state_commitment = get_state_commitment(&app_instance.state);

    let block = block::create_block(
        block_number,
        name,
        start_sequence,
        end_sequence,
        actions_commitment,
        state_commitment,
        time_since_last_block,
        end_sequence - start_sequence + 1,
        previous_block_actions_commitment,
        actions_commitment,
        option::none(),
        option::none(),
        option::none(),
        false,
        timestamp,
        option::none(),
        option::none(),
        option::none(),
        option::none(),
        ctx,
    );

    (timestamp, block)
}

public fun update_block_state_data_availability(
    app_instance: &mut AppInstance,
    block_number: u64,
    state_data_availability: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    only_admin(app_instance, ctx);
    let timestamp = clock.timestamp_ms();
    let block = borrow_mut(
        &mut app_instance.blocks,
        block_number,
    );
    block::set_data_availability(
        block,
        option::some(state_data_availability),
        option::none(),
        option::some(timestamp),
        option::none(),
    );
}

public fun update_block_proof_data_availability(
    app_instance: &mut AppInstance,
    block_number: u64,
    proof_data_availability: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    only_admin(app_instance, ctx);
    let timestamp = clock.timestamp_ms();
    let block = borrow_mut(
        &mut app_instance.blocks,
        block_number,
    );
    let state_data_availability = block::get_state_data_availability(block);
    let state_calculated_at = block::get_state_calculated_at(block);
    block::set_data_availability(
        block,
        state_data_availability,
        option::some(proof_data_availability),
        state_calculated_at,
        option::some(timestamp),
    );
}

#[error]
const EBlockNotProved: vector<u8> = b"Block not proved";

public fun update_block_mina_tx_hash(
    app_instance: &mut AppInstance,
    block_number: u64,
    mina_tx_hash: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    only_admin(app_instance, ctx);
    let timestamp = clock.timestamp_ms();
    let block = borrow_mut(
        &mut app_instance.blocks,
        block_number,
    );
    assert!(
        block::get_state_data_availability(block).is_some() && (block::get_proof_data_availability(block).is_some() || block_number == 0),
        EBlockNotProved,
    );
    block::set_settlement_tx(
        block,
        option::some(mina_tx_hash),
        false,
        option::some(timestamp),
        option::none(),
    );
}

#[error]
const EBlockNotSentToMina: vector<u8> = b"Block not sent to Mina";

public fun update_block_mina_tx_included_in_block(
    app_instance: &mut AppInstance,
    block_number: u64,
    settled_on_mina_at: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    only_admin(app_instance, ctx);
    let block = borrow_mut(
        &mut app_instance.blocks,
        block_number,
    );
    assert!(
        block::get_state_data_availability(block).is_some() && (block::get_proof_data_availability(block).is_some() || block_number == 0),
        EBlockNotProved,
    );
    assert!(
        block::get_sent_to_settlement_at(block).is_some(),
        EBlockNotSentToMina,
    );
    assert!(
        block::get_settlement_tx_hash(block).is_some(),
        EBlockNotSentToMina,
    );
    let sent_to_settlement_at = block::get_sent_to_settlement_at(block);
    let settlement_tx_hash = block::get_settlement_tx_hash(block);
    block::set_settlement_tx(
        block,
        settlement_tx_hash,
        true,
        sent_to_settlement_at,
        option::some(settled_on_mina_at),
    );

    // Delete the proof_calculation for this block as it's no longer needed
    // All information has been preserved in events
    // Note: Block 0 (genesis) doesn't have proof_calculation, only blocks >= 1
    if (
        block_number > 0 && object_table::contains(&app_instance.proof_calculations, block_number)
    ) {
        let proof_calculation = object_table::remove(
            &mut app_instance.proof_calculations,
            block_number,
        );
        prover::delete_proof_calculation(proof_calculation, clock);
    };
}

// Getter functions for AppInstance
public fun silvana_app_name(app_instance: &AppInstance): &String {
    &app_instance.silvana_app_name
}

public fun description(app_instance: &AppInstance): &Option<String> {
    &app_instance.description
}

public fun metadata(app_instance: &AppInstance): &Option<String> {
    &app_instance.metadata
}

public fun methods(app_instance: &AppInstance): &VecMap<String, AppMethod> {
    &app_instance.methods
}

public fun state(app_instance: &AppInstance): &AppState {
    &app_instance.state
}

public fun sequence(app_instance: &AppInstance): u64 {
    app_instance.sequence
}

public fun admin(app_instance: &AppInstance): address {
    app_instance.admin
}

public fun block_number(app_instance: &AppInstance): u64 {
    app_instance.block_number
}

public fun previous_block_timestamp(app_instance: &AppInstance): u64 {
    app_instance.previous_block_timestamp
}

public fun previous_block_last_sequence(app_instance: &AppInstance): u64 {
    app_instance.previous_block_last_sequence
}

public fun previous_block_actions_state(
    app_instance: &AppInstance,
): &Element<Scalar> {
    &app_instance.previous_block_actions_state
}

public fun last_proved_block_number(app_instance: &AppInstance): u64 {
    app_instance.last_proved_block_number
}

public fun is_paused(app_instance: &AppInstance): bool {
    app_instance.isPaused
}

public fun created_at(app_instance: &AppInstance): u64 {
    app_instance.created_at
}

public fun updated_at(app_instance: &AppInstance): u64 {
    app_instance.updated_at
}

// Mutable getter functions
public fun state_mut(
    app_instance: &mut AppInstance,
    cap: &AppInstanceCap,
): &mut AppState {
    assert!(
        cap.instance_address == app_instance.id.to_address(),
        ENotAuthorized,
    );
    &mut app_instance.state
}

public fun blocks_mut(
    app_instance: &mut AppInstance,
): &mut ObjectTable<u64, Block> {
    &mut app_instance.blocks
}

public fun proof_calculations_mut(
    app_instance: &mut AppInstance,
): &mut ObjectTable<u64, ProofCalculation> {
    &mut app_instance.proof_calculations
}

public fun jobs(app_instance: &AppInstance): &Jobs {
    &app_instance.jobs
}

public(package) fun jobs_mut(app_instance: &mut AppInstance): &mut Jobs {
    &mut app_instance.jobs
}

public fun get_block(app_instance: &AppInstance, block_number: u64): &Block {
    borrow(&app_instance.blocks, block_number)
}

public fun get_block_mut(
    app_instance: &mut AppInstance,
    block_number: u64,
): &mut Block {
    borrow_mut(&mut app_instance.blocks, block_number)
}

public fun get_proof_calculation(
    app_instance: &AppInstance,
    block_number: u64,
): &ProofCalculation {
    borrow(&app_instance.proof_calculations, block_number)
}

public fun get_proof_calculation_mut(
    app_instance: &mut AppInstance,
    block_number: u64,
): &mut ProofCalculation {
    borrow_mut(&mut app_instance.proof_calculations, block_number)
}

// Job-related convenience methods
#[error]
const EMethodNotFound: vector<u8> = b"Method not found in app instance";

public fun create_app_job(
    app_instance: &mut AppInstance,
    method_name: String,
    job_description: Option<String>,
    sequences: Option<vector<u64>>,
    data: vector<u8>,
    clock: &Clock,
    ctx: &mut TxContext,
): u64 {
    // Get method from app instance methods
    assert!(
        vec_map::contains(&app_instance.methods, &method_name),
        EMethodNotFound,
    );
    let method = vec_map::get(&app_instance.methods, &method_name);

    jobs::create_job(
        &mut app_instance.jobs,
        job_description,
        *app_method::developer(method),
        *app_method::agent(method),
        *app_method::agent_method(method),
        app_instance.silvana_app_name,
        app_instance.id.to_address().to_string(),
        method_name,
        sequences,
        data,
        clock,
        ctx,
    )
}

public fun start_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    clock: &Clock,
) {
    jobs::start_job(&mut app_instance.jobs, job_id, clock)
}

public fun complete_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    clock: &Clock,
) {
    jobs::complete_job(&mut app_instance.jobs, job_id, clock)
}

public fun fail_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    error: String,
    clock: &Clock,
) {
    jobs::fail_job(&mut app_instance.jobs, job_id, error, clock)
}

public fun get_app_job(app_instance: &AppInstance, job_id: u64): &jobs::Job {
    jobs::get_job(&app_instance.jobs, job_id)
}

public fun get_app_pending_jobs(app_instance: &AppInstance): vector<u64> {
    jobs::get_pending_jobs(&app_instance.jobs)
}

public fun get_app_pending_jobs_count(app_instance: &AppInstance): u64 {
    jobs::get_pending_jobs_count(&app_instance.jobs)
}

public fun get_next_pending_app_job(app_instance: &AppInstance): Option<u64> {
    jobs::get_next_pending_job(&app_instance.jobs)
}

// Sequence state management wrapper functions
public fun add_state_for_sequence(
    app_instance: &mut AppInstance,
    sequence: u64,
    state_data: Option<vector<u8>>,
    data_availability_hash: Option<String>,
    optimistic_state: vector<u8>,
    transition_data: vector<u8>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    sequence_state::add_state_for_sequence(
        &mut app_instance.sequence_state_manager,
        sequence,
        state_data,
        data_availability_hash,
        optimistic_state,
        transition_data,
        clock,
        ctx,
    );
}

public fun update_state_for_sequence(
    app_instance: &mut AppInstance,
    sequence: u64,
    new_state_data: Option<vector<u8>>,
    new_data_availability_hash: Option<String>,
    clock: &Clock,
) {
    sequence_state::update_state_for_sequence(
        &mut app_instance.sequence_state_manager,
        sequence,
        new_state_data,
        new_data_availability_hash,
        clock,
    );
}

public fun purge_sequences_below(
    app_instance: &mut AppInstance,
    threshold_sequence: u64,
    clock: &Clock,
) {
    sequence_state::purge(
        &mut app_instance.sequence_state_manager,
        threshold_sequence,
        clock,
    );
}

// Getter functions
public fun lowest_sequence(app_instance: &AppInstance): Option<u64> {
    sequence_state::lowest_sequence(&app_instance.sequence_state_manager)
}

public fun highest_sequence(app_instance: &AppInstance): Option<u64> {
    sequence_state::highest_sequence(&app_instance.sequence_state_manager)
}

public(package) fun has_sequence_state(
    app_instance: &AppInstance,
    sequence: u64,
): bool {
    sequence_state::has_sequence_state(
        &app_instance.sequence_state_manager,
        sequence,
    )
}

public(package) fun borrow_sequence_state(
    app_instance: &AppInstance,
    sequence: u64,
): &SequenceState {
    sequence_state::borrow_sequence_state(
        &app_instance.sequence_state_manager,
        sequence,
    )
}

public(package) fun borrow_sequence_state_mut(
    app_instance: &mut AppInstance,
    sequence: u64,
): &mut SequenceState {
    sequence_state::borrow_sequence_state_mut(
        &mut app_instance.sequence_state_manager,
        sequence,
    )
}
