module coordination::settlement;

use std::string::String;
use sui::event;
use sui::vec_map::{Self, VecMap};

public struct BlockSettlement has store, copy, drop {
    block_number: u64,
    settlement_tx_hash: Option<String>,
    settlement_tx_included_in_block: bool,
    sent_to_settlement_at: Option<u64>,
    settled_at: Option<u64>,
}

public struct Settlement has store {
    chain: String,
    last_settled_block_number: u64,
    settlement_address: Option<String>,
    block_settlements: VecMap<u64, BlockSettlement>, // key is block number
}

public struct SettlementCreatedEvent has copy, drop {
    chain: String,
    settlement_address: Option<String>,
}

public struct BlockSettlementTransactionEvent has copy, drop {
    chain: String,
    block_number: u64,
    settlement_tx_hash: Option<String>,
    settlement_tx_included_in_block: bool,
    sent_to_settlement_at: Option<u64>,
    settled_at: Option<u64>,
}

public fun create_settlement(
    chain: String,
    settlement_address: Option<String>,
): Settlement {
    let settlement = Settlement {
        chain,
        last_settled_block_number: 0,
        settlement_address,
        block_settlements: vec_map::empty(),
    };
    event::emit(SettlementCreatedEvent {
        chain,
        settlement_address,
    });
    settlement
}

public fun set_block_settlement_tx(
    settlement: &mut Settlement,
    block_number: u64,
    settlement_tx_hash: Option<String>,
    settlement_tx_included_in_block: bool,
    sent_to_settlement_at: Option<u64>,
    settled_at: Option<u64>,
) {
    if (!vec_map::contains(&settlement.block_settlements, &block_number)) {
        let block_settlement = BlockSettlement {
            block_number,
            settlement_tx_hash,
            settlement_tx_included_in_block,
            sent_to_settlement_at,
            settled_at,
        };
        vec_map::insert(&mut settlement.block_settlements, block_number, block_settlement);
    } else {
        let block_settlement = vec_map::get_mut(&mut settlement.block_settlements, &block_number);
        block_settlement.settlement_tx_hash = settlement_tx_hash;
        block_settlement.settlement_tx_included_in_block = settlement_tx_included_in_block;
        block_settlement.sent_to_settlement_at = sent_to_settlement_at;
        block_settlement.settled_at = settled_at;
    };
    
    event::emit(BlockSettlementTransactionEvent {
        chain: settlement.chain,
        block_number,
        settlement_tx_hash,
        settlement_tx_included_in_block,
        sent_to_settlement_at,
        settled_at,
    });
}

// Getter functions for Settlement
public fun get_chain(settlement: &Settlement): String {
    settlement.chain
}

public fun get_last_settled_block_number(settlement: &Settlement): u64 {
    settlement.last_settled_block_number
}

public fun get_settlement_address(settlement: &Settlement): Option<String> {
    settlement.settlement_address
}

public fun set_last_settled_block_number(settlement: &mut Settlement, block_number: u64) {
    settlement.last_settled_block_number = block_number;
}

// Getter functions for BlockSettlement
public fun get_block_settlement(
    settlement: &Settlement,
    block_number: u64,
): Option<BlockSettlement> {
    if (vec_map::contains(&settlement.block_settlements, &block_number)) {
        option::some(*vec_map::get(&settlement.block_settlements, &block_number))
    } else {
        option::none()
    }
}

public fun get_block_settlement_tx_hash(
    settlement: &Settlement,
    block_number: u64,
): Option<String> {
    if (vec_map::contains(&settlement.block_settlements, &block_number)) {
        let block_settlement = vec_map::get(&settlement.block_settlements, &block_number);
        block_settlement.settlement_tx_hash
    } else {
        option::none()
    }
}

public fun get_block_settlement_tx_included_in_block(
    settlement: &Settlement,
    block_number: u64,
): bool {
    if (vec_map::contains(&settlement.block_settlements, &block_number)) {
        let block_settlement = vec_map::get(&settlement.block_settlements, &block_number);
        block_settlement.settlement_tx_included_in_block
    } else {
        false
    }
}

public fun get_sent_to_settlement_at(
    settlement: &Settlement,
    block_number: u64,
): Option<u64> {
    if (vec_map::contains(&settlement.block_settlements, &block_number)) {
        let block_settlement = vec_map::get(&settlement.block_settlements, &block_number);
        block_settlement.sent_to_settlement_at
    } else {
        option::none()
    }
}

public fun get_settled_at(
    settlement: &Settlement,
    block_number: u64,
): Option<u64> {
    if (vec_map::contains(&settlement.block_settlements, &block_number)) {
        let block_settlement = vec_map::get(&settlement.block_settlements, &block_number);
        block_settlement.settled_at
    } else {
        option::none()
    }
}
