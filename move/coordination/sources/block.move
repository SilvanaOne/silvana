module coordination::block;

use std::string::String;
use sui::bls12381::Scalar;
use sui::event;
use sui::group_ops::Element;

public struct Block has key, store {
    id: UID,
    name: String,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    actions_commitment: Element<Scalar>,
    state_commitment: Element<Scalar>,
    time_since_last_block: u64,
    number_of_transactions: u64,
    start_actions_commitment: Element<Scalar>,
    end_actions_commitment: Element<Scalar>,
    state_data_availability: Option<String>,
    proof_data_availability: Option<String>,
    created_at: u64,
    state_calculated_at: Option<u64>,
    proved_at: Option<u64>,
}

public struct BlockEvent has copy, drop {
    address: address,
    name: String,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    created_at: u64,
    time_since_last_block: u64,
    number_of_transactions: u64,
    start_actions_commitment: Element<Scalar>,
    end_actions_commitment: Element<Scalar>,
    state_commitment: Element<Scalar>,
}

// TODO: review options
public struct DataAvailabilityEvent has copy, drop {
    block_number: u64,
    state_data_availability: Option<String>,
    proof_data_availability: Option<String>,
    state_calculated_at: Option<u64>,
    proof_calculated_at: Option<u64>,
}


public fun create_block(
    block_number: u64,
    name: String,
    start_sequence: u64,
    end_sequence: u64,
    actions_commitment: Element<Scalar>,
    state_commitment: Element<Scalar>,
    time_since_last_block: u64,
    number_of_transactions: u64,
    start_actions_commitment: Element<Scalar>,
    end_actions_commitment: Element<Scalar>,
    state_data_availability: Option<String>,
    proof_data_availability: Option<String>,
    created_at: u64,
    state_calculated_at: Option<u64>,
    proved_at: Option<u64>,
    ctx: &mut TxContext,
): Block {
    let id = object::new(ctx);
    let block_address = id.to_address();
    let block = Block {
        id,
        name,
        block_number,
        start_sequence,
        end_sequence,
        actions_commitment,
        state_commitment,
        time_since_last_block,
        number_of_transactions,
        start_actions_commitment,
        end_actions_commitment,
        state_data_availability,
        proof_data_availability,
        created_at,
        state_calculated_at,
        proved_at,
    };
    event::emit(BlockEvent {
        address: block_address,
        block_number,
        start_sequence,
        end_sequence,
        name,
        created_at,
        time_since_last_block,
        number_of_transactions: end_sequence - start_sequence + 1,
        start_actions_commitment,
        end_actions_commitment,
        state_commitment,
    });
    block
}

public fun set_data_availability(
    block: &mut Block,
    state_data_availability: Option<String>,
    proof_data_availability: Option<String>,
    state_calculated_at: Option<u64>,
    proof_calculated_at: Option<u64>,
) {
    block.state_data_availability = state_data_availability;
    block.proof_data_availability = proof_data_availability;
    block.state_calculated_at = state_calculated_at;
    block.proved_at = proof_calculated_at;
    event::emit(DataAvailabilityEvent {
        block_number: block.block_number,
        state_data_availability,
        proof_data_availability,
        state_calculated_at,
        proof_calculated_at,
    });
}


// Getter functions
public fun get_block_number(block: &Block): u64 {
    block.block_number
}

public fun get_name(block: &Block): String {
    block.name
}

public fun get_start_sequence(block: &Block): u64 {
    block.start_sequence
}

public fun get_end_sequence(block: &Block): u64 {
    block.end_sequence
}

public fun get_start_actions_commitment(block: &Block): Element<Scalar> {
    block.start_actions_commitment
}

public fun get_end_actions_commitment(block: &Block): Element<Scalar> {
    block.end_actions_commitment
}

public fun get_state_commitment(block: &Block): Element<Scalar> {
    block.state_commitment
}

public fun get_time_since_last_block(block: &Block): u64 {
    block.time_since_last_block
}

public fun get_number_of_transactions(block: &Block): u64 {
    block.number_of_transactions
}

public fun get_state_data_availability(block: &Block): Option<String> {
    block.state_data_availability
}

public fun get_proof_data_availability(block: &Block): Option<String> {
    block.proof_data_availability
}

public fun get_created_at(block: &Block): u64 {
    block.created_at
}

public fun get_state_calculated_at(block: &Block): Option<u64> {
    block.state_calculated_at
}

public fun get_proved_at(block: &Block): Option<u64> {
    block.proved_at
}
