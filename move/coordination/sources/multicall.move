module coordination::multicall;

use std::string::String;

public struct OperationFailedEvent has copy, drop {
    operation: String,
    entity_type: String,
    entity_id: u64,
    reason: String,
    caller: address,
    timestamp: u64,
    additional_data: Option<vector<u8>>,
}

public fun emit_operation_failed(
    operation: String,
    entity_type: String,
    entity_id: u64,
    reason: String,
    caller: address,
    timestamp: u64,
    additional_data: Option<vector<u8>>,
) {
    sui::event::emit(OperationFailedEvent {
        operation,
        entity_type,
        entity_id,
        reason,
        caller,
        timestamp,
        additional_data,
    });
}