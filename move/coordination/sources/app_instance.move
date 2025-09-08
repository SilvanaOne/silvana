module coordination::app_instance;

use commitment::state::{AppState, get_state_commitment, create_app_state};
use coordination::app_method::{Self, AppMethod};
use coordination::block::{Self, Block};
use coordination::jobs::{Self, Jobs};
use coordination::multicall;
use coordination::prover::{Self, ProofCalculation};
use coordination::sequence_state::{Self, SequenceState, SequenceStateManager};
use coordination::settlement::{Self, Settlement};
use coordination::silvana_app::{Self, SilvanaApp};
use std::string::{Self, String};
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
    metadata: VecMap<String, String>,
    kv: VecMap<String, String>,
    methods: VecMap<String, AppMethod>,
    state: AppState,
    blocks: ObjectTable<u64, Block>, // key is block number
    settlements: VecMap<String, Settlement>, // key is chain
    proof_calculations: ObjectTable<u64, ProofCalculation>, // key is block number
    sequence_state_manager: SequenceStateManager,
    jobs: Jobs,
    sequence: u64,
    admin: address,
    block_number: u64,
    previous_block_timestamp: u64,
    previous_block_last_sequence: u64,
    previous_block_actions_state: Element<Scalar>,
    last_proved_block_number: u64,
    last_settled_block_number: u64,
    isPaused: bool,
    min_time_between_blocks: u64, // Configurable minimum time between blocks
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

public struct KVSetEvent has copy, drop {
    app_instance_address: address,
    key: String,
    value: String,
}

public struct KVDeletedEvent has copy, drop {
    app_instance_address: address,
    key: String,
}

public struct MetadataAddedEvent has copy, drop {
    app_instance_address: address,
    key: String,
    value: String,
}

public struct BlockPurgedEvent has copy, drop {
    app_instance_address: address,
    block_number: u64,
    reason: String,
}

// Debug event for tracking block cleanup
public struct BlockCleanupEvent has copy, drop {
    app_instance_address: address,
    previous_last_settled: u64,
    new_min_settled: u64,
    start_block: u64,
    settlements_count: u64,
}


public struct APP_INSTANCE has drop {}

// Constants
const MIN_TIME_BETWEEN_BLOCKS: u64 = 60_000;

// Error codes
#[error]
const ENotAuthorized: vector<u8> = b"Not authorized";

#[error]
const EAppInstancePaused: vector<u8> = b"App instance is paused";

#[error]
const EMinTimeBetweenBlocksTooLow: vector<u8> =
    b"Min time between blocks must be at least 60000ms";

#[error]
const EBlockNotProved: vector<u8> = b"Block not proved";

#[error]
const ESettlementChainNotFound: vector<u8> = b"Settlement chain not found";

#[error]
const ESettlementVectorLengthMismatch: vector<u8> =
    b"Settlement chains and addresses vectors must have the same length";

#[error]
const ESettlementJobAlreadyExists: vector<u8> =
    b"Settlement job already exists for this chain";

#[error]
const EMethodNotFound: vector<u8> = b"Method not found in app instance";

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
    settlement_chains: vector<String>, // vector of chain names
    settlement_addresses: vector<Option<String>>, // vector of optional settlement addresses
    min_time_between_blocks: u64, // Minimum time between blocks in milliseconds
    clock: &Clock,
    ctx: &mut TxContext,
): AppInstanceCap {
    // Enforce minimum value
    assert!(
        min_time_between_blocks >= MIN_TIME_BETWEEN_BLOCKS,
        EMinTimeBetweenBlocksTooLow,
    );
    // Create settlements VecMap from provided chains
    // First check that both vectors have the same length
    assert!(
        vector::length(&settlement_chains) == vector::length(&settlement_addresses),
        ESettlementVectorLengthMismatch,
    );

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

    let mut settlements = vec_map::empty<String, Settlement>();
    let mut i = 0;
    while (i < vector::length(&settlement_chains)) {
        let chain = *vector::borrow(&settlement_chains, i);
        let address = *vector::borrow(&settlement_addresses, i);
        let settlement = settlement::create_settlement(
            chain,
            address,
            ctx,
        );
        vec_map::insert(&mut settlements, chain, settlement);
        i = i + 1;
    };

    let mut app_instance: AppInstance = AppInstance {
        id: instance_id,
        silvana_app_name: app_name,
        description: instance_description,
        metadata: vec_map::empty(),
        kv: vec_map::empty(),
        methods,
        state,
        blocks,
        settlements,
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
        last_settled_block_number: 0u64,
        isPaused: false,
        min_time_between_blocks,
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

public(package) fun only_admin(app_instance: &AppInstance, ctx: &TxContext) {
    assert!(app_instance.admin == ctx.sender(), ENotAuthorized);
}

public(package) fun not_paused(app_instance: &AppInstance) {
    assert!(app_instance.isPaused == false, EAppInstancePaused);
}

// Update the minimum time between blocks
// Only admin can call this function
public fun update_min_time_between_blocks(
    app_instance: &mut AppInstance,
    new_min_time: u64,
    ctx: &TxContext,
) {
    // Only admin can update this setting
    assert!(ctx.sender() == app_instance.admin, ENotAuthorized);

    // Enforce minimum value
    assert!(
        new_min_time >= MIN_TIME_BETWEEN_BLOCKS,
        EMinTimeBetweenBlocksTooLow,
    );

    app_instance.min_time_between_blocks = new_min_time;
}

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
        current_time - app_instance.previous_block_timestamp <= app_instance.min_time_between_blocks
    ) {
        event::emit(InsufficientTimeForBlockEvent {
            app_instance_address: app_instance.id.to_address(),
            current_time,
            previous_block_timestamp: app_instance.previous_block_timestamp,
            time_since_last_block: current_time - app_instance.previous_block_timestamp,
            min_time_required: app_instance.min_time_between_blocks,
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
    clock: &Clock,
    ctx: &mut TxContext,
): bool {
    let proof_calculation = borrow_mut(
        &mut app_instance.proof_calculations,
        block_number,
    );
    prover::start_proving(
        proof_calculation,
        sequences,
        merged_sequences_1,
        merged_sequences_2,
        clock,
        ctx,
    )
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
        timestamp,
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
    _ctx: &mut TxContext,
) {
    // only_admin(app_instance, ctx);
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
    _ctx: &mut TxContext,
) {
    // only_admin(app_instance, ctx);
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

public fun update_block_settlement_tx_hash(
    app_instance: &mut AppInstance,
    chain: String,
    block_number: u64,
    settlement_tx_hash: String,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // only_admin(app_instance, ctx);
    let timestamp = clock.timestamp_ms();
    let block = borrow(
        &app_instance.blocks,
        block_number,
    );
    assert!(
        block::get_state_data_availability(block).is_some() && (block::get_proof_data_availability(block).is_some() || block_number == 0),
        EBlockNotProved,
    );

    // Update settlement for the specific chain
    assert!(
        vec_map::contains(&app_instance.settlements, &chain),
        ESettlementChainNotFound,
    );
    let settlement = vec_map::get_mut(&mut app_instance.settlements, &chain);
    settlement::set_block_settlement_tx(
        settlement,
        block_number,
        option::some(settlement_tx_hash),
        false,
        option::some(timestamp),
        option::none(),
        ctx,
    );
}

// Helper function to update AppInstance's last_settled_block_number
// and clean up old blocks that have been settled across all chains
fun update_app_last_settled_block_number(app_instance: &mut AppInstance) {
    // Find the minimum last_settled_block_number across all settlements
    let settlements_size = vec_map::size(&app_instance.settlements);
    if (settlements_size == 0) {
        return
    };

    // Initialize with the first settlement's last_settled_block_number
    let (_, first_settlement) = vec_map::get_entry_by_idx(
        &app_instance.settlements,
        0,
    );
    let mut min_settled_block = settlement::get_last_settled_block_number(
        first_settlement,
    );

    // Find the minimum across all settlements
    let mut idx = 1;
    while (idx < settlements_size) {
        let (_, settlement) = vec_map::get_entry_by_idx(
            &app_instance.settlements,
            idx,
        );
        let last_settled = settlement::get_last_settled_block_number(
            settlement,
        );
        if (last_settled < min_settled_block) {
            min_settled_block = last_settled;
        };
        idx = idx + 1;
    };

    // Store the previous last_settled_block_number before updating
    let previous_last_settled = app_instance.last_settled_block_number;

    // Update AppInstance's last_settled_block_number
    app_instance.last_settled_block_number = min_settled_block;

    // Clean up blocks that are below the minimum settled block number
    // We keep block 0 (genesis) and blocks from min_settled_block onwards
    // Start from previous_last_settled (including it) to check for deletion
    let start_block = if (previous_last_settled == 0) {
        1 // Never try to delete block 0 (genesis)
    } else {
        previous_last_settled // Include the previous settled block in cleanup check
    };
    let end_block = min_settled_block; // Delete everything before this

    // Emit debug event to track cleanup behavior
    event::emit(BlockCleanupEvent {
        app_instance_address: app_instance.id.to_address(),
        previous_last_settled,
        new_min_settled: min_settled_block,
        start_block,
        settlements_count: settlements_size,
    });

    // Only proceed if there's a change in settled block number
    if (min_settled_block > previous_last_settled && min_settled_block > 0) {
        let mut block_to_remove = start_block;
        while (block_to_remove < end_block) {
            if (object_table::contains(&app_instance.blocks, block_to_remove)) {
                let block = object_table::remove(
                    &mut app_instance.blocks,
                    block_to_remove,
                );
                // Delete the Block object using the delete function from block module
                block::delete_block(block);

                // Emit event for block purging
                event::emit(BlockPurgedEvent {
                    app_instance_address: app_instance.id.to_address(),
                    block_number: block_to_remove,
                    reason: b"Block settled on all chains".to_string(),
                });
            };
            block_to_remove = block_to_remove + 1;
        };
    };
}

// #[error]
// const EBlockNotSentToMina: vector<u8> = b"Block not sent to Mina";

public fun update_block_settlement_tx_included_in_block(
    app_instance: &mut AppInstance,
    chain: String,
    block_number: u64,
    settled_at: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // only_admin(app_instance, ctx);
    let block = borrow(
        &app_instance.blocks,
        block_number,
    );
    assert!(
        block::get_state_data_availability(block).is_some() && (block::get_proof_data_availability(block).is_some() || block_number == 0),
        EBlockNotProved,
    );

    // Update settlement for the specific chain
    assert!(
        vec_map::contains(&app_instance.settlements, &chain),
        ESettlementChainNotFound,
    );
    let settlement = vec_map::get_mut(&mut app_instance.settlements, &chain);

    let sent_to_settlement_at = settlement::get_sent_to_settlement_at(
        settlement,
        block_number,
    );
    let settlement_tx_hash = settlement::get_block_settlement_tx_hash(
        settlement,
        block_number,
    );

    settlement::set_block_settlement_tx(
        settlement,
        block_number,
        settlement_tx_hash,
        true,
        sent_to_settlement_at,
        option::some(settled_at),
        ctx,
    );

    // Update last_settled_block_number by checking all blocks from
    // last_settled_block_number + 1 up to and including the current block_number
    // to find the highest consecutive settled block
    let current_last_settled = settlement::get_last_settled_block_number(
        settlement,
    );
    let mut i = current_last_settled + 1;
    while (
        i <= block_number && object_table::contains(&app_instance.blocks, i)
    ) {
        if (
            settlement::get_block_settlement_tx_included_in_block(settlement, i)
        ) {
            // This block is settled, update last_settled_block_number
            settlement::set_last_settled_block_number(settlement, i);
            i = i + 1;
        } else {
            // Found a gap - this block is not settled yet
            break
        };
    };

    // Also check if there are any blocks after block_number that were settled out of order
    let updated_last_settled = settlement::get_last_settled_block_number(
        settlement,
    );
    if (updated_last_settled == block_number) {
        let mut j = block_number + 1;
        while (
            j < app_instance.block_number && object_table::contains(&app_instance.blocks, j)
        ) {
            if (
                settlement::get_block_settlement_tx_included_in_block(
                    settlement,
                    j,
                )
            ) {
                settlement::set_last_settled_block_number(settlement, j);
                j = j + 1;
            } else {
                break
            };
        };
    };

    // Update the AppInstance's last_settled_block_number to be the minimum across all chains
    update_app_last_settled_block_number(app_instance);

    // Check if ALL chains have settled this block before deleting ProofCalculation and purging
    // We need to ensure all chains have last_settled_block_number >= block_number
    let mut all_chains_settled = true;
    let mut min_settled_block = block_number;

    let settlements_size = vec_map::size(&app_instance.settlements);
    let mut idx = 0;
    while (idx < settlements_size) {
        let (_, other_settlement) = vec_map::get_entry_by_idx(
            &app_instance.settlements,
            idx,
        );
        let other_last_settled = settlement::get_last_settled_block_number(
            other_settlement,
        );
        if (other_last_settled < block_number) {
            all_chains_settled = false;
            // Track the minimum settled block across all chains
            if (other_last_settled < min_settled_block) {
                min_settled_block = other_last_settled;
            };
        };
        idx = idx + 1;
    };

    // Only delete ProofCalculation and purge sequence states if ALL chains have settled this block
    if (all_chains_settled) {
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

        // Use the block's end_sequence for purging
        let block_to_purge = app_instance.blocks.borrow(block_number);
        let proved_sequence = block::get_end_sequence(block_to_purge);

        let rollback = app_instance.state.get_rollback_mut();
        commitment::rollback::purge_records(rollback, proved_sequence);

        sequence_state::purge(
            &mut app_instance.sequence_state_manager,
            proved_sequence,
            clock,
        );
    };
}

// Methods for managing metadata and kv
public fun add_metadata(
    app_instance: &mut AppInstance,
    key: String,
    value: String,
    _ctx: &TxContext,
) {
    //only_admin(app_instance, ctx);
    vec_map::insert(&mut app_instance.metadata, key, value);

    event::emit(MetadataAddedEvent {
        app_instance_address: app_instance.id.to_address(),
        key,
        value,
    });
}

public fun set_kv(
    app_instance: &mut AppInstance,
    key: String,
    value: String,
    _ctx: &TxContext,
) {
    //only_admin(app_instance, ctx);
    if (vec_map::contains(&app_instance.kv, &key)) {
        vec_map::remove(&mut app_instance.kv, &key);
    };
    vec_map::insert(&mut app_instance.kv, key, value);

    event::emit(KVSetEvent {
        app_instance_address: app_instance.id.to_address(),
        key,
        value,
    });
}

public fun delete_kv(
    app_instance: &mut AppInstance,
    key: String,
    _ctx: &TxContext,
) {
    //only_admin(app_instance, ctx);
    if (vec_map::contains(&app_instance.kv, &key)) {
        vec_map::remove(&mut app_instance.kv, &key);

        event::emit(KVDeletedEvent {
            app_instance_address: app_instance.id.to_address(),
            key,
        });
    };
}

// Getter functions for AppInstance
public fun silvana_app_name(app_instance: &AppInstance): &String {
    &app_instance.silvana_app_name
}

public fun description(app_instance: &AppInstance): &Option<String> {
    &app_instance.description
}

public fun get_metadata(app_instance: &AppInstance): &VecMap<String, String> {
    &app_instance.metadata
}

public fun get_kv(app_instance: &AppInstance): &VecMap<String, String> {
    &app_instance.kv
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

public fun min_time_between_blocks(app_instance: &AppInstance): u64 {
    app_instance.min_time_between_blocks
}

public fun previous_block_actions_state(
    app_instance: &AppInstance,
): &Element<Scalar> {
    &app_instance.previous_block_actions_state
}

public fun last_proved_block_number(app_instance: &AppInstance): u64 {
    app_instance.last_proved_block_number
}

public fun last_settled_block_number(
    app_instance: &AppInstance,
    chain: String,
): u64 {
    if (!vec_map::contains(&app_instance.settlements, &chain)) {
        return 0
    };
    let settlement = vec_map::get(&app_instance.settlements, &chain);
    settlement::get_last_settled_block_number(settlement)
}

public fun get_settlement_chains(app_instance: &AppInstance): vector<String> {
    vec_map::keys(&app_instance.settlements)
}

public fun get_settlement_address(
    app_instance: &AppInstance,
    chain: String,
): Option<String> {
    if (!vec_map::contains(&app_instance.settlements, &chain)) {
        return option::none()
    };
    let settlement = vec_map::get(&app_instance.settlements, &chain);
    settlement::get_settlement_address(settlement)
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
public fun create_app_job(
    app_instance: &mut AppInstance,
    method_name: String,
    job_description: Option<String>,
    block_number: Option<u64>,
    sequences: Option<vector<u64>>,
    sequences1: Option<vector<u64>>,
    sequences2: Option<vector<u64>>,
    data: vector<u8>,
    interval_ms: Option<u64>,
    next_scheduled_at: Option<u64>,
    settlement_chain: Option<String>, // None for regular jobs, Some(chain) for settlement jobs
    clock: &Clock,
    ctx: &mut TxContext,
): Option<u64> {
    // Get method from app instance methods
    if (!vec_map::contains(&app_instance.methods, &method_name)) {
        multicall::emit_operation_failed(
            string::utf8(b"create_job"),
            string::utf8(b"app_instance"),
            0, // Using 0 as entity_id since we don't have a job sequence yet
            string::utf8(EMethodNotFound),
            ctx.sender(),
            timestamp_ms(clock),
            option::some(method_name.into_bytes()),
        );
        return option::none()
    };
    let method = vec_map::get(&app_instance.methods, &method_name);

    // If this is a settlement job, validate BEFORE creating the job to avoid orphans
    if (option::is_some(&settlement_chain)) {
        let chain = *option::borrow(&settlement_chain);
        if (!vec_map::contains(&app_instance.settlements, &chain)) {
            multicall::emit_operation_failed(
                string::utf8(b"create_job"),
                string::utf8(b"app_instance"),
                0, // Using 0 as entity_id since we don't have a job sequence yet
                string::utf8(ESettlementChainNotFound),
                ctx.sender(),
                timestamp_ms(clock),
                option::some(method_name.into_bytes()),
            );
            return option::none()
        };
        
        // Check that there's no existing settlement job for this chain BEFORE creating job
        let settlement = vec_map::get(&app_instance.settlements, &chain);
        let existing_job = settlement::get_settlement_job(settlement);
        if (option::is_some(&existing_job)) {
            multicall::emit_operation_failed(
                string::utf8(b"create_job"),
                string::utf8(b"app_instance"),
                0, // Using 0 as entity_id since we don't have a job sequence yet
                string::utf8(ESettlementJobAlreadyExists),
                ctx.sender(),
                timestamp_ms(clock),
                option::some(method_name.into_bytes()),
            );
            // Return early - no job created, no orphans
            return option::none()
        };
    };

    // Now create the job after all validation passes
    let (success, job_id) = jobs::create_job(
        &mut app_instance.jobs,
        job_description,
        *app_method::developer(method),
        *app_method::agent(method),
        *app_method::agent_method(method),
        app_instance.silvana_app_name,
        app_instance.id.to_address().to_string(),
        method_name,
        block_number,
        sequences,
        sequences1,
        sequences2,
        data,
        interval_ms,
        next_scheduled_at,
        clock,
        ctx,
    );

    if (!success) {
        return option::none()
    };

    // If this is a settlement job, update the settlement_job field in the Settlement struct  
    if (option::is_some(&settlement_chain)) {
        let chain = *option::borrow(&settlement_chain);
        let settlement = vec_map::get_mut(
            &mut app_instance.settlements,
            &chain,
        );
        settlement::set_settlement_job(settlement, option::some(job_id));
    };

    option::some(job_id)
}

// Create a merge job after first reserving the proofs
// Returns Option<u64> with job_id if successful, None if start_proving fails or job creation fails
public fun create_merge_job(
    app_instance: &mut AppInstance,
    block_number: u64,
    sequences: vector<u64>, // Combined sequences to merge
    merged_sequences_1: vector<u64>, // First proof sequences
    merged_sequences_2: vector<u64>, // Second proof sequences
    job_description: Option<String>,
    clock: &Clock,
    ctx: &mut TxContext,
): Option<u64> {
    // Step 1: Try to reserve the proofs with start_proving
    let proving_success = start_proving(
        app_instance,
        block_number,
        sequences,
        option::some(merged_sequences_1),
        option::some(merged_sequences_2),
        clock,
        ctx,
    );

    // Step 2: Only create the merge job if proof reservation succeeded
    if (!proving_success) {
        multicall::emit_operation_failed(
            string::utf8(b"create_merge_job"),
            string::utf8(b"app_instance"),
            0, // Using 0 as entity_id since we don't have a job sequence yet
            string::utf8(b"Failed to reserve proofs for merge"),
            ctx.sender(),
            timestamp_ms(clock),
            option::none(),
        );
        return option::none()
    };

    // Step 3: Create the merge job
    // Note: We pass the individual sequences as sequences1 and sequences2
    // The 'sequences' parameter holds the combined sequences
    create_app_job(
        app_instance,
        string::utf8(b"merge"),
        job_description,
        option::some(block_number),
        option::some(sequences), // Combined sequences
        option::some(merged_sequences_1), // First proof sequences
        option::some(merged_sequences_2), // Second proof sequences
        vector::empty<u8>(), // No additional data needed
        option::none(), // Not a periodic job
        option::none(), // No scheduled time
        option::none(), // Not a settlement job
        clock,
        ctx,
    )
}

public fun start_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    jobs::start_job(&mut app_instance.jobs, job_id, clock, ctx)
}

public fun complete_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    jobs::complete_job(&mut app_instance.jobs, job_id, clock, ctx)
}

public fun fail_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    error: String,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    jobs::fail_job(&mut app_instance.jobs, job_id, error, clock, ctx)
}

public fun terminate_app_job(
    app_instance: &mut AppInstance,
    job_id: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    // Check if this job is a settlement job for any chain and clear it
    let keys = vec_map::keys(&app_instance.settlements);
    let mut i = 0;
    let len = vector::length(&keys);
    while (i < len) {
        let chain = vector::borrow(&keys, i);
        let settlement = vec_map::get_mut(&mut app_instance.settlements, chain);
        let settlement_job_id = settlement::get_settlement_job(settlement);
        if (
            option::is_some(&settlement_job_id) && *option::borrow(&settlement_job_id) == job_id
        ) {
            settlement::set_settlement_job(settlement, option::none());
        };
        i = i + 1;
    };

    jobs::terminate_job(&mut app_instance.jobs, job_id, clock, ctx)
}

public fun restart_failed_app_jobs(
    app_instance: &mut AppInstance,
    job_sequences: Option<vector<u64>>,
    clock: &Clock,
) {
    jobs::restart_failed_jobs(&mut app_instance.jobs, job_sequences, clock)
}

public fun remove_failed_app_jobs(
    app_instance: &mut AppInstance,
    job_sequences: Option<vector<u64>>,
    clock: &Clock,
) {
    jobs::remove_failed_jobs(&mut app_instance.jobs, job_sequences, clock)
}

public fun multicall_app_job_operations(
    app_instance: &mut AppInstance,
    complete_job_sequences: vector<u64>,
    fail_job_sequences: vector<u64>,
    fail_errors: vector<String>,
    terminate_job_sequences: vector<u64>,
    start_job_sequences: vector<u64>,
    start_job_memory_requirements: vector<u64>,
    available_memory: u64,
    clock: &Clock,
    ctx: &TxContext,
) {
    jobs::multicall_job_operations(
        &mut app_instance.jobs,
        complete_job_sequences,
        fail_job_sequences,
        fail_errors,
        terminate_job_sequences,
        start_job_sequences,
        start_job_memory_requirements,
        available_memory,
        clock,
        ctx,
    )
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

public fun get_app_failed_jobs(app_instance: &AppInstance): vector<u64> {
    jobs::get_failed_jobs(&app_instance.jobs)
}

public fun get_app_failed_jobs_count(app_instance: &AppInstance): u64 {
    jobs::get_failed_jobs_count(&app_instance.jobs)
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
    ctx: &TxContext,
): bool {
    sequence_state::update_state_for_sequence(
        &mut app_instance.sequence_state_manager,
        sequence,
        new_state_data,
        new_data_availability_hash,
        clock,
        ctx,
    )
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
