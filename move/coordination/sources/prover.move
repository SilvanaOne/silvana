module coordination::prover;

use coordination::multicall;
use std::bcs;
use std::string::{Self, String};
use sui::clock::Clock;
use sui::display;
use sui::event;
use sui::package;
use sui::vec_map::{Self, VecMap, contains, insert, get_mut, empty};

public struct Proof has copy, drop, store {
    status: u8,
    da_hash: Option<String>,
    sequence1: Option<vector<u64>>,
    sequence2: Option<vector<u64>>,
    rejected_count: u16,
    timestamp: u64,
    prover: address,
    user: Option<address>,
    job_id: Option<String>,
}

public struct ProofCalculation has key, store {
    id: UID,
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    proofs: VecMap<vector<u64>, Proof>,
    block_proof: Option<String>,
    is_finished: bool,
}

public struct ProofStartedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    timestamp: u64,
    prover: address,
}

public struct ProofSubmittedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    da_hash: String,
    timestamp: u64,
    prover: address,
    cpu_time: u64,
    cpu_cores: u8,
    prover_memory: u64,
    prover_architecture: String,
}

public struct ProofUsedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    da_hash: String,
    timestamp: u64,
    user: address,
}

public struct ProofReservedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    da_hash: String,
    timestamp: u64,
    user: address,
}

public struct ProofReturnedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    timestamp: u64,
}

public struct ProofRejectedEvent has copy, drop {
    block_number: u64,
    sequences: vector<u64>,
    timestamp: u64,
    verifier: address,
}

public struct BlockProofEvent has copy, drop {
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    da_hash: String,
    timestamp: u64,
}

public struct ProofCalculationCreatedEvent has copy, drop {
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    timestamp: u64,
}

public struct ProofCalculationFinishedEvent has copy, drop {
    proof_calculation_address: address,
    block_number: u64,
    start_sequence: u64,
    end_sequence: u64,
    block_proof: String,
    is_finished: bool,
    proofs_count: u64,
    timestamp: u64,
}

public struct ProofCalculationDeletedEvent has copy, drop {
    app_instance_id: address,
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    proofs: VecMap<vector<u64>, Proof>,
    block_proof: Option<String>,
    is_finished: bool,
    timestamp: u64,
}

public struct PROVER has drop {}

// Constants
const PROOF_STATUS_STARTED: u8 = 1;
const PROOF_STATUS_CALCULATED: u8 = 2;
const PROOF_STATUS_REJECTED: u8 = 3;
const PROOF_STATUS_RESERVED: u8 = 4;
const PROOF_STATUS_USED: u8 = 5;

// Universal timeout for all proof statuses (5 minutes)
// After this timeout, any proof can be restarted/reserved/merged regardless of status
const PROOF_TIMEOUT_MS: u64 = 300000;

// Error codes
#[error]
const EProofNotCalculated: vector<u8> = b"Proof not calculated or already used";

#[error]
const ESequenceOutOfRange: vector<u8> =
    b"Sequence is outside valid range for this block";

#[error]
const ESequencesCannotBeEmpty: vector<u8> = b"Sequences vector cannot be empty";

fun init(otw: PROVER, ctx: &mut TxContext) {
    let publisher = package::claim(otw, ctx);

    let keys = vector[
        b"name".to_string(),
        b"description".to_string(),
        b"project_url".to_string(),
        b"creator".to_string(),
    ];

    let values = vector[
        b"Block {block_number} proofs".to_string(),
        b"Silvana Proofs for block {block_number}".to_string(),
        b"https://silvana.one".to_string(),
        b"DFST".to_string(),
    ];
    let mut display_prover = display::new_with_fields<ProofCalculation>(
        &publisher,
        keys,
        values,
        ctx,
    );

    display_prover.update_version();
    transfer::public_transfer(publisher, ctx.sender());
    transfer::public_transfer(display_prover, ctx.sender());
}

public(package) fun create_block_proof_calculation(
    block_number: u64,
    start_sequence: u64,
    end_sequence: Option<u64>,
    clock: &Clock,
    ctx: &mut TxContext,
): (ProofCalculation, address) {
    let proofs = empty<vector<u64>, Proof>();
    let id = object::new(ctx);
    let address = id.to_address();
    let timestamp = sui::clock::timestamp_ms(clock);
    let proof_calculation = ProofCalculation {
        id,
        block_number,
        start_sequence,
        end_sequence,
        proofs,
        block_proof: option::none(),
        is_finished: block_number == 0,
    };
    event::emit(ProofCalculationCreatedEvent {
        block_number,
        start_sequence,
        end_sequence,
        timestamp,
    });
    (proof_calculation, address)
}

public(package) fun finish_proof_calculation(
    proof_calculation: &mut ProofCalculation,
    end_sequence: u64,
    block_proof: String,
    clock: &Clock,
) {
    proof_calculation.block_proof = option::some(block_proof);
    proof_calculation.end_sequence = option::some(end_sequence);
    proof_calculation.is_finished = true;
    event::emit(BlockProofEvent {
        block_number: proof_calculation.block_number,
        start_sequence: proof_calculation.start_sequence,
        end_sequence,
        da_hash: block_proof,
        timestamp: sui::clock::timestamp_ms(clock),
    });
}

public(package) fun set_end_sequence(
    proof_calculation: &mut ProofCalculation,
    end_sequence: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    proof_calculation.end_sequence = option::some(end_sequence);
    let mut i = proof_calculation.start_sequence;
    let mut sequences = vector::empty<u64>();
    while (i <= end_sequence) {
        vector::push_back(&mut sequences, i);
        i = i + 1;
    };
    if (contains(&proof_calculation.proofs, &sequences)) {
        let mut da_hash = option::none<String>();
        {
            let proof = get_mut(&mut proof_calculation.proofs, &sequences);
            if (
                proof.status == PROOF_STATUS_CALCULATED && proof.da_hash.is_some()
            ) {
                proof.status = PROOF_STATUS_USED;
                proof.timestamp = sui::clock::timestamp_ms(clock);
                proof.user = option::some(ctx.sender());
                da_hash = option::some(*proof.da_hash.borrow());
            };
        };
        {
            if (da_hash.is_some()) {
                finish_proof_calculation(
                    proof_calculation,
                    end_sequence,
                    *da_hash.borrow(),
                    clock,
                );
                return true
            }
        };
    };
    false
}

public fun get_proof_calculation_address(
    proof_calculation: &ProofCalculation,
): address {
    proof_calculation.id.to_address()
}

public fun get_proof_calculation_end_sequence(
    proof_calculation: &ProofCalculation,
): Option<u64> {
    proof_calculation.end_sequence
}

public fun is_finished(proof_calculation: &ProofCalculation): bool {
    proof_calculation.is_finished
}

// Delete proof calculation after block is settled and emit comprehensive event
public(package) fun delete_proof_calculation(
    proof_calculation: ProofCalculation,
    app_instance_id: address,
    clock: &Clock,
) {
    let ProofCalculation {
        id,
        block_number,
        start_sequence,
        end_sequence,
        proofs,
        block_proof,
        is_finished,
    } = proof_calculation;

    let proof_calculation_address = id.to_address();
    let proofs_count = sui::vec_map::size(&proofs);
    let timestamp = sui::clock::timestamp_ms(clock);

    // Emit comprehensive event with all data before deletion
    event::emit(ProofCalculationFinishedEvent {
        proof_calculation_address,
        block_number,
        start_sequence,
        end_sequence: *option::borrow_with_default(&end_sequence, &0),
        block_proof: *option::borrow_with_default(
            &block_proof,
            &b"".to_string(),
        ),
        is_finished,
        proofs_count,
        timestamp,
    });
    event::emit(ProofCalculationDeletedEvent {
        app_instance_id,
        block_number,
        start_sequence,
        end_sequence,
        proofs,
        block_proof,
        is_finished,
        timestamp,
    });

    // Delete the object
    object::delete(id);
}

fun use_proof(proof: &mut Proof, clock: &Clock, ctx: &TxContext) {
    assert!(
        proof.status != PROOF_STATUS_REJECTED && proof.status != PROOF_STATUS_STARTED,
        EProofNotCalculated,
    );
    proof.status = PROOF_STATUS_USED;
    proof.timestamp = sui::clock::timestamp_ms(clock);
    proof.user = option::some(ctx.sender());
}

fun return_proof(proof: &mut Proof, clock: &Clock) {
    proof.status = PROOF_STATUS_CALCULATED;
    proof.timestamp = sui::clock::timestamp_ms(clock);
    proof.user = option::none();
}

fun reserve_proof(
    proof: &mut Proof,
    isBlockProof: bool,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    let current_time = sui::clock::timestamp_ms(clock);
    let is_timed_out = current_time > proof.timestamp + PROOF_TIMEOUT_MS;

    if (
        !(
            proof.status == PROOF_STATUS_CALCULATED || 
            is_timed_out ||  // Allow any timed-out proof to be reserved (5 minutes)
            (isBlockProof && (proof.status == PROOF_STATUS_USED || proof.status == PROOF_STATUS_RESERVED)),
        )
    ) {
        multicall::emit_operation_failed(
            string::utf8(b"reserve_proof"),
            string::utf8(b"proof"),
            0,
            EProofNotCalculated.to_string(),
            ctx.sender(),
            sui::clock::timestamp_ms(clock),
            option::none(),
        );
        return false
    };

    proof.status = PROOF_STATUS_RESERVED;
    proof.timestamp = current_time;
    proof.user = option::some(ctx.sender());
    true
}

public(package) fun start_proving(
    proof_calculation: &mut ProofCalculation,
    sequences: vector<u64>,
    sequence1: Option<vector<u64>>,
    sequence2: Option<vector<u64>>,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    // Sort and validate all sequences, get back sorted versions
    let sorted_sequences = sort_and_validate_sequences(
        &sequences,
        proof_calculation.start_sequence,
        &proof_calculation.end_sequence,
    );

    // Sort and validate sequence1 if provided
    let sorted_sequence1 = if (sequence1.is_some()) {
        option::some(
            sort_and_validate_sequences(
                sequence1.borrow(),
                proof_calculation.start_sequence,
                &proof_calculation.end_sequence,
            ),
        )
    } else {
        option::none()
    };

    // Sort and validate sequence2 if provided
    let sorted_sequence2 = if (sequence2.is_some()) {
        option::some(
            sort_and_validate_sequences(
                sequence2.borrow(),
                proof_calculation.start_sequence,
                &proof_calculation.end_sequence,
            ),
        )
    } else {
        option::none()
    };

    let mut isBlockProof = false;
    if (proof_calculation.end_sequence.is_some()) {
        let end_sequence = *proof_calculation.end_sequence.borrow();
        if (
            sorted_sequences[0] == proof_calculation.start_sequence && sorted_sequences[vector::length(&sorted_sequences)-1] == end_sequence
        ) {
            isBlockProof = true;
        }
    };
    let proof = Proof {
        sequence1: sorted_sequence1,
        sequence2: sorted_sequence2,
        rejected_count: 0,
        job_id: option::none(),
        prover: ctx.sender(),
        user: option::none(),
        da_hash: option::none(),
        status: PROOF_STATUS_STARTED,
        timestamp: sui::clock::timestamp_ms(clock),
    };
    if (contains(&proof_calculation.proofs, &sorted_sequences)) {
        let mut existing_sequence_1: vector<u64> = vector::empty();
        let mut existing_sequence_2: vector<u64> = vector::empty();
        {
            let existing_proof = get_mut(
                &mut proof_calculation.proofs,
                &sorted_sequences,
            );
            // Check if proof can be restarted
            let current_time = sui::clock::timestamp_ms(clock);
            let is_timed_out =
                current_time > existing_proof.timestamp + PROOF_TIMEOUT_MS;

            // Allow restart if: rejected OR timed-out (any status after 5 minutes)
            if (
                existing_proof.status != PROOF_STATUS_REJECTED && !is_timed_out
            ) {
                multicall::emit_operation_failed(
                    string::utf8(b"start_proving"),
                    string::utf8(b"proof"),
                    proof_calculation.block_number,
                    string::utf8(b"proof_already_started"),
                    ctx.sender(),
                    sui::clock::timestamp_ms(clock),
                    option::some(bcs::to_bytes(&sorted_sequences)),
                );
                return false
            };
            let rejected_count = existing_proof.rejected_count;
            if (existing_proof.sequence1.is_some()) {
                existing_sequence_1 = *existing_proof.sequence1.borrow();
            };
            if (existing_proof.sequence2.is_some()) {
                existing_sequence_2 = *existing_proof.sequence2.borrow();
            };
            *existing_proof = proof;
            existing_proof.rejected_count = rejected_count;
        };
        {
            if (existing_sequence_1.length() > 0) {
                if (contains(&proof_calculation.proofs, &existing_sequence_1)) {
                    let proof1 = get_mut(
                        &mut proof_calculation.proofs,
                        &existing_sequence_1,
                    );
                    return_proof(proof1, clock);
                    event::emit(ProofReturnedEvent {
                        block_number: proof_calculation.block_number,
                        sequences: existing_sequence_1,
                        timestamp: sui::clock::timestamp_ms(clock),
                    });
                }
            };
            if (existing_sequence_2.length() > 0) {
                if (contains(&proof_calculation.proofs, &existing_sequence_2)) {
                    let proof2 = get_mut(
                        &mut proof_calculation.proofs,
                        &existing_sequence_2,
                    );
                    return_proof(proof2, clock);
                    event::emit(ProofReturnedEvent {
                        block_number: proof_calculation.block_number,
                        sequences: existing_sequence_2,
                        timestamp: sui::clock::timestamp_ms(clock),
                    });
                };
            }
        };
    } else {
        // New proof - first check if we can reserve the sub-proofs before inserting
        if (sorted_sequence1.is_some()) {
            let proof1 = vec_map::get(
                &proof_calculation.proofs,
                sorted_sequence1.borrow(),
            );
            let current_time = sui::clock::timestamp_ms(clock);
            let is_timed_out =
                current_time > proof1.timestamp + PROOF_TIMEOUT_MS;

            // Check if proof1 can be reserved
            if (
                !(
                    proof1.status == PROOF_STATUS_CALCULATED || 
                  is_timed_out ||  // Any status is OK after 5 minute timeout
                  (isBlockProof && (proof1.status == PROOF_STATUS_USED || proof1.status == PROOF_STATUS_RESERVED)),
                )
            ) {
                // Cannot reserve proof1 - don't insert new proof
                return false
            }
        };

        if (sorted_sequence2.is_some()) {
            let proof2 = vec_map::get(
                &proof_calculation.proofs,
                sorted_sequence2.borrow(),
            );
            let current_time = sui::clock::timestamp_ms(clock);
            let is_timed_out =
                current_time > proof2.timestamp + PROOF_TIMEOUT_MS;

            // Check if proof2 can be reserved
            if (
                !(
                    proof2.status == PROOF_STATUS_CALCULATED || 
                  is_timed_out ||  // Any status is OK after 5 minute timeout
                  (isBlockProof && (proof2.status == PROOF_STATUS_USED || proof2.status == PROOF_STATUS_RESERVED)),
                )
            ) {
                // Cannot reserve proof2 - don't insert new proof
                return false
            }
        };

        // Both sub-proofs can be reserved - now safe to insert the new proof
        insert(&mut proof_calculation.proofs, sorted_sequences, proof);
    };

    // Now reserve the sub-proofs (we know this will succeed)
    if (sorted_sequence1.is_some()) {
        let proof1 = get_mut(
            &mut proof_calculation.proofs,
            sorted_sequence1.borrow(),
        );
        reserve_proof(proof1, isBlockProof, clock, ctx);
        event::emit(ProofReservedEvent {
            block_number: proof_calculation.block_number,
            sequences: *sorted_sequence1.borrow(),
            da_hash: *proof1.da_hash.borrow(),
            timestamp: sui::clock::timestamp_ms(clock),
            user: ctx.sender(),
        });
    };
    if (sorted_sequence2.is_some()) {
        let proof2 = get_mut(
            &mut proof_calculation.proofs,
            sorted_sequence2.borrow(),
        );
        reserve_proof(proof2, isBlockProof, clock, ctx);
        event::emit(ProofReservedEvent {
            block_number: proof_calculation.block_number,
            sequences: *sorted_sequence2.borrow(),
            da_hash: *proof2.da_hash.borrow(),
            timestamp: sui::clock::timestamp_ms(clock),
            user: ctx.sender(),
        });
    };
    event::emit(ProofStartedEvent {
        block_number: proof_calculation.block_number,
        sequences: sorted_sequences,
        timestamp: sui::clock::timestamp_ms(clock),
        prover: ctx.sender(),
    });
    true
}

public fun reject_proof(
    proof_calculation: &mut ProofCalculation,
    sequences: vector<u64>,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Sort and validate sequences
    let sorted_sequences = sort_and_validate_sequences(
        &sequences,
        proof_calculation.start_sequence,
        &proof_calculation.end_sequence,
    );

    let mut sequence1 = vector::empty<u64>();
    let mut sequence2 = vector::empty<u64>();
    {
        let proof = get_mut(&mut proof_calculation.proofs, &sorted_sequences);
        proof.status = PROOF_STATUS_REJECTED;
        proof.rejected_count = proof.rejected_count + 1;
        if (proof.sequence1.is_some()) {
            sequence1 = *proof.sequence1.borrow();
        };
        if (proof.sequence2.is_some()) {
            sequence2 = *proof.sequence2.borrow();
        };
    };
    if (sequence1.length() > 0) {
        let proof1 = get_mut(
            &mut proof_calculation.proofs,
            &sequence1,
        );
        return_proof(proof1, clock);
        event::emit(ProofReturnedEvent {
            block_number: proof_calculation.block_number,
            sequences: sequence1,
            timestamp: sui::clock::timestamp_ms(clock),
        });
    };
    if (sequence2.length() > 0) {
        let proof2 = get_mut(
            &mut proof_calculation.proofs,
            &sequence2,
        );
        return_proof(proof2, clock);
        event::emit(ProofReturnedEvent {
            block_number: proof_calculation.block_number,
            sequences: sequence2,
            timestamp: sui::clock::timestamp_ms(clock),
        });
    };
    event::emit(ProofRejectedEvent {
        block_number: proof_calculation.block_number,
        sequences: sorted_sequences,
        timestamp: sui::clock::timestamp_ms(clock),
        verifier: ctx.sender(),
    });
}

// Helper function to sort and validate that all sequences are within the valid range
// Returns a sorted copy of the input sequences
fun sort_and_validate_sequences(
    sequences: &vector<u64>,
    start_sequence: u64,
    end_sequence: &Option<u64>,
): vector<u64> {
    let len = vector::length(sequences);

    // Sequences cannot be empty
    assert!(len > 0, ESequencesCannotBeEmpty);

    // Clone the input vector
    let mut sorted_sequences = *sequences;

    // Sort the sequences using bubble sort (simple for Move)
    let mut i = 0;
    while (i < len) {
        let mut j = 0;
        while (j < len - i - 1) {
            if (sorted_sequences[j] > sorted_sequences[j + 1]) {
                // Swap elements
                vector::swap(&mut sorted_sequences, j, j + 1);
            };
            j = j + 1;
        };
        i = i + 1;
    };

    // After sorting, only need to check first and last elements
    let first_seq = sorted_sequences[0];
    let last_seq = sorted_sequences[len - 1];

    // Check that first sequence is >= start_sequence
    assert!(first_seq >= start_sequence, ESequenceOutOfRange);

    // If end_sequence is set, check that last sequence is <= end_sequence
    if (end_sequence.is_some()) {
        let end_seq = *end_sequence.borrow();
        assert!(last_seq <= end_seq, ESequenceOutOfRange);
    };

    sorted_sequences
}

// TODO: add prover whitelisting or proof verification
public fun submit_proof(
    proof_calculation: &mut ProofCalculation,
    sequences: vector<u64>, // should be sorted
    sequence1: Option<vector<u64>>,
    sequence2: Option<vector<u64>>,
    job_id: String,
    da_hash: String,
    cpu_cores: u8,
    prover_architecture: String,
    prover_memory: u64,
    cpu_time: u64,
    clock: &Clock,
    ctx: &mut TxContext,
): bool {
    // Sort and validate all sequences, get back sorted versions
    let sorted_sequences = sort_and_validate_sequences(
        &sequences,
        proof_calculation.start_sequence,
        &proof_calculation.end_sequence,
    );

    // Sort and validate sequence1 if provided
    let sorted_sequence1 = if (sequence1.is_some()) {
        option::some(
            sort_and_validate_sequences(
                sequence1.borrow(),
                proof_calculation.start_sequence,
                &proof_calculation.end_sequence,
            ),
        )
    } else {
        option::none()
    };

    // Sort and validate sequence2 if provided
    let sorted_sequence2 = if (sequence2.is_some()) {
        option::some(
            sort_and_validate_sequences(
                sequence2.borrow(),
                proof_calculation.start_sequence,
                &proof_calculation.end_sequence,
            ),
        )
    } else {
        option::none()
    };
    let proof = Proof {
        sequence1: sorted_sequence1,
        sequence2: sorted_sequence2,
        rejected_count: 0,
        job_id: option::some(job_id),
        prover: ctx.sender(),
        user: option::none(),
        da_hash: option::some(da_hash),
        status: PROOF_STATUS_CALCULATED,
        timestamp: sui::clock::timestamp_ms(clock),
    };
    event::emit(ProofSubmittedEvent {
        block_number: proof_calculation.block_number,
        sequences: sorted_sequences,
        da_hash,
        timestamp: sui::clock::timestamp_ms(clock),
        prover: ctx.sender(),
        cpu_cores,
        prover_architecture,
        prover_memory,
        cpu_time,
    });
    if (contains(&proof_calculation.proofs, &sorted_sequences)) {
        let existing_proof = get_mut(
            &mut proof_calculation.proofs,
            &sorted_sequences,
        );
        let rejected_count = existing_proof.rejected_count;
        *existing_proof = proof;
        existing_proof.rejected_count = rejected_count;
    } else { insert(&mut proof_calculation.proofs, sorted_sequences, proof); };
    if (sorted_sequence1.is_some()) {
        let proof1 = get_mut(
            &mut proof_calculation.proofs,
            sorted_sequence1.borrow(),
        );
        use_proof(proof1, clock, ctx);
        event::emit(ProofUsedEvent {
            block_number: proof_calculation.block_number,
            sequences: *sorted_sequence1.borrow(),
            da_hash: *proof1.da_hash.borrow(),
            timestamp: sui::clock::timestamp_ms(clock),
            user: ctx.sender(),
        });
    };
    if (sorted_sequence2.is_some()) {
        let proof2 = get_mut(
            &mut proof_calculation.proofs,
            sorted_sequence2.borrow(),
        );
        use_proof(proof2, clock, ctx);
        event::emit(ProofUsedEvent {
            block_number: proof_calculation.block_number,
            sequences: *sorted_sequence2.borrow(),
            da_hash: *proof2.da_hash.borrow(),
            timestamp: sui::clock::timestamp_ms(clock),
            user: ctx.sender(),
        });
    };
    if (proof_calculation.end_sequence.is_some()) {
        let end_sequence = *proof_calculation.end_sequence.borrow();
        if (
            sorted_sequences[0]==proof_calculation.start_sequence && 
            sorted_sequences[vector::length(&sorted_sequences)-1] == end_sequence
        ) {
            finish_proof_calculation(
                proof_calculation,
                end_sequence,
                da_hash,
                clock,
            );
            return true
        }
    };
    false
}
