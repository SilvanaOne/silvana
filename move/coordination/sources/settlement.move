module coordination::settlement;

use std::string::String;
use sui::event;
use sui::object_table::{Self, ObjectTable};

public struct BlockSettlement has key, store {
    id: UID,
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
    block_settlements: ObjectTable<u64, BlockSettlement>, // key is block number
    settlement_job: Option<u64>, // ID of the active settlement job for this chain
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

public struct BlockSettledEvent has copy, drop {
    chain: String,
    block_number: u64,
    settlement_tx_hash: Option<String>,
    sent_to_settlement_at: Option<u64>,
    settled_at: Option<u64>,
}

public struct BlockSettlementPurgedEvent has copy, drop {
    chain: String,
    block_number: u64,
    reason: String,
}

public fun create_settlement(
    chain: String,
    settlement_address: Option<String>,
    ctx: &mut TxContext,
): Settlement {
    let settlement = Settlement {
        chain,
        last_settled_block_number: 0,
        settlement_address,
        block_settlements: object_table::new<u64, BlockSettlement>(ctx),
        settlement_job: option::none(),
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
    ctx: &mut TxContext,
) {
    if (!object_table::contains(&settlement.block_settlements, block_number)) {
        let block_settlement = BlockSettlement {
            id: object::new(ctx),
            block_number,
            settlement_tx_hash,
            settlement_tx_included_in_block,
            sent_to_settlement_at,
            settled_at,
        };
        object_table::add(
            &mut settlement.block_settlements,
            block_number,
            block_settlement,
        );
    } else {
        let block_settlement = object_table::borrow_mut(
            &mut settlement.block_settlements,
            block_number,
        );
        block_settlement.settlement_tx_hash = settlement_tx_hash;
        block_settlement.settlement_tx_included_in_block =
            settlement_tx_included_in_block;
        block_settlement.sent_to_settlement_at = sent_to_settlement_at;
        block_settlement.settled_at = settled_at;
    };

    if (settlement_tx_included_in_block) {
        event::emit(BlockSettledEvent {
            chain: settlement.chain,
            block_number,
            settlement_tx_hash,
            sent_to_settlement_at,
            settled_at,
        });
        let mut last_settled_block_number = settlement.last_settled_block_number;
        while (last_settled_block_number < block_number) {
            // Check if the next block is settled
            let next_block_number = last_settled_block_number + 1;
            if (
                object_table::contains(
                    &settlement.block_settlements,
                    next_block_number,
                )
            ) {
                let next_block_settlement = object_table::borrow(
                    &settlement.block_settlements,
                    next_block_number,
                );
                if (next_block_settlement.settlement_tx_included_in_block) {
                    // Remove the previous block if it exists (cleanup old settled blocks)
                    if (
                        object_table::contains(
                            &settlement.block_settlements,
                            last_settled_block_number,
                        )
                    ) {
                        let block_settlement = object_table::remove(
                            &mut settlement.block_settlements,
                            last_settled_block_number,
                        );

                        // Emit purge event before deleting
                        event::emit(BlockSettlementPurgedEvent {
                            chain: settlement.chain,
                            block_number: last_settled_block_number,
                            reason: b"Block already settled and finalized".to_string(),
                        });

                        let BlockSettlement {
                            id,
                            block_number: _,
                            settlement_tx_hash: _,
                            settlement_tx_included_in_block: _,
                            sent_to_settlement_at: _,
                            settled_at: _,
                        } = block_settlement;
                        object::delete(id);
                    };
                    last_settled_block_number = next_block_number;
                } else {
                    break
                };
            } else {
                break
            };
        };
        settlement.last_settled_block_number = last_settled_block_number;
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

public fun set_last_settled_block_number(
    settlement: &mut Settlement,
    block_number: u64,
) {
    settlement.last_settled_block_number = block_number;
}

public fun get_settlement_job(settlement: &Settlement): Option<u64> {
    settlement.settlement_job
}

public fun set_settlement_job(
    settlement: &mut Settlement,
    job_id: Option<u64>,
) {
    settlement.settlement_job = job_id;
}

// Check if block settlement exists
public fun has_block_settlement(
    settlement: &Settlement,
    block_number: u64,
): bool {
    object_table::contains(&settlement.block_settlements, block_number)
}

public fun get_block_settlement_tx_hash(
    settlement: &Settlement,
    block_number: u64,
): Option<String> {
    if (object_table::contains(&settlement.block_settlements, block_number)) {
        let block_settlement = object_table::borrow(
            &settlement.block_settlements,
            block_number,
        );
        block_settlement.settlement_tx_hash
    } else {
        option::none()
    }
}

public fun get_block_settlement_tx_included_in_block(
    settlement: &Settlement,
    block_number: u64,
): bool {
    if (object_table::contains(&settlement.block_settlements, block_number)) {
        let block_settlement = object_table::borrow(
            &settlement.block_settlements,
            block_number,
        );
        block_settlement.settlement_tx_included_in_block
    } else {
        false
    }
}

public fun get_sent_to_settlement_at(
    settlement: &Settlement,
    block_number: u64,
): Option<u64> {
    if (object_table::contains(&settlement.block_settlements, block_number)) {
        let block_settlement = object_table::borrow(
            &settlement.block_settlements,
            block_number,
        );
        block_settlement.sent_to_settlement_at
    } else {
        option::none()
    }
}

public fun get_settled_at(
    settlement: &Settlement,
    block_number: u64,
): Option<u64> {
    if (object_table::contains(&settlement.block_settlements, block_number)) {
        let block_settlement = object_table::borrow(
            &settlement.block_settlements,
            block_number,
        );
        block_settlement.settled_at
    } else {
        option::none()
    }
}
