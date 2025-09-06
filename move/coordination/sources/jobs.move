module coordination::jobs;

use std::string::{Self, String};
use sui::clock::{Self, Clock};
use sui::event;
use sui::object_table::{Self, ObjectTable};
use sui::vec_map::{Self, VecMap};
use sui::vec_set::{Self, VecSet};

public enum JobStatus has copy, drop, store {
    Pending,
    Running,
    Failed(String),
}

public struct Job has key, store {
    id: UID,
    job_sequence: u64,
    description: Option<String>,
    // Metadata of the agent method to call
    developer: String,
    agent: String,
    agent_method: String,
    // Metadata of the calling app instance
    app: String,
    app_instance: String,
    app_instance_method: String,
    block_number: Option<u64>,
    sequences: Option<vector<u64>>,
    sequences1: Option<vector<u64>>,
    sequences2: Option<vector<u64>>,
    data: vector<u8>,
    // Metadata of the job
    status: JobStatus,
    attempts: u8,
    // Periodic scheduling fields (None => one-time job)
    interval_ms: Option<u64>, // e.g. Some(60000) for 1 min
    next_scheduled_at: Option<u64>, // absolute ms timestamp for next run, if periodic
    // Metadata of the job
    created_at: u64,
    updated_at: u64,
}

public struct Jobs has key, store {
    id: UID,
    jobs: ObjectTable<u64, Job>,
    failed_jobs_count: u64,
    failed_jobs_index: VecSet<u64>,
    pending_jobs: VecSet<u64>,
    pending_jobs_count: u64,
    // index: developer -> agent -> agent_method -> job_id for pending jobs
    pending_jobs_indexes: VecMap<
        String,
        VecMap<String, VecMap<String, VecSet<u64>>>,
    >,
    next_job_sequence: u64,
    max_attempts: u8,
}

public struct JobCreatedEvent has copy, drop {
    job_sequence: u64,
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
    app: String,
    app_instance: String,
    app_instance_method: String,
    block_number: Option<u64>,
    sequences: Option<vector<u64>>,
    sequences1: Option<vector<u64>>,
    sequences2: Option<vector<u64>>,
    data: vector<u8>,
    status: JobStatus,
    created_at: u64,
}

public struct JobUpdatedEvent has copy, drop {
    job_sequence: u64,
    developer: String,
    agent: String,
    agent_method: String,
    app_instance: String,
    status: JobStatus,
    attempts: u8,
    updated_at: u64,
}

public struct JobStartedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    coordinator: address,
    started_at: u64,
}

public struct JobRestartedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    started_at: u64,
}

public struct JobsRestartedEvent has copy, drop {
    failed_jobs: vector<u64>,
    num_failed_jobs: u64,
    restarted_at: u64,
}

public struct JobCompletedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    coordinator: address,
    completed_at: u64,
}

public struct JobFailedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    coordinator: address,
    error: String,
    attempts: u8,
    failed_at: u64,
}

public struct JobDeletedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    deleted_at: u64,
}

public struct JobTerminatedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    coordinator: address,
    terminated_at: u64,
}

public struct JobRescheduledEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    next_scheduled_at: u64,
    rescheduled_at: u64,
}

// Error events for debugging failed operations
public struct JobOperationFailedEvent has copy, drop {
    operation: String, // "start", "complete", "fail", "terminate"
    job_sequence: u64,
    coordinator: address,
    reason: String, // "job_not_found", "wrong_status", "not_due_yet"
    current_status: Option<JobStatus>, // Current status if job exists
    timestamp: u64,
}

// Multicall result event
public struct MulticallExecutedEvent has copy, drop {
    coordinator: address,
    complete_jobs: vector<u64>,
    complete_results: vector<bool>,
    fail_jobs: vector<u64>,
    fail_results: vector<bool>,
    terminate_jobs: vector<u64>,
    terminate_results: vector<bool>,
    start_jobs: vector<u64>,
    start_results: vector<bool>,
    timestamp: u64,
}

// Constants
const DEFAULT_MAX_ATTEMPTS: u8 = 3;
const MIN_INTERVAL_MS: u64 = 60_000; // >= 1 minute for periodic tasks
const RETRY_INTERVAL_MS: u64 = 60_000; // >= 1 minute for periodic tasks

// Error codes
#[error]
const EJobNotFound: vector<u8> = b"Job not found";

#[error]
const EJobNotPending: vector<u8> = b"Job is not in pending state";

#[error]
const EIntervalTooShort: vector<u8> = b"Interval must be >= 60000 ms";

/// Multicall function arguments validation error
#[error]
const EInvalidArguments: vector<u8> =
    b"Invalid arguments: fail_job_sequences and fail_errors must have same length";

// Initialize Jobs storage with optional max_attempts
public fun create_jobs(max_attempts: Option<u8>, ctx: &mut TxContext): Jobs {
    let attempts = if (option::is_some(&max_attempts)) {
        *option::borrow(&max_attempts)
    } else {
        DEFAULT_MAX_ATTEMPTS
    };

    Jobs {
        id: object::new(ctx),
        jobs: object_table::new(ctx),
        failed_jobs_count: 0,
        failed_jobs_index: vec_set::empty(),
        pending_jobs: vec_set::empty(),
        pending_jobs_count: 0,
        pending_jobs_indexes: vec_map::empty(),
        next_job_sequence: 1,
        max_attempts: attempts,
    }
}

// Create a new job
public fun create_job(
    jobs: &mut Jobs,
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
    app: String,
    app_instance: String,
    app_instance_method: String,
    block_number: Option<u64>,
    sequences: Option<vector<u64>>,
    sequences1: Option<vector<u64>>,
    sequences2: Option<vector<u64>>,
    data: vector<u8>,
    interval_ms: Option<u64>,
    next_scheduled_at: Option<u64>,
    clock: &Clock,
    ctx: &mut TxContext,
): u64 {
    // Validate interval if provided
    if (option::is_some(&interval_ms)) {
        let interval = *option::borrow(&interval_ms);
        assert!(interval >= MIN_INTERVAL_MS, EIntervalTooShort);
    };

    let job_sequence = jobs.next_job_sequence;
    let now = clock::timestamp_ms(clock);
    // Settlement job tracking is now handled per-chain in the Settlement struct

    let job = Job {
        id: object::new(ctx),
        job_sequence,
        description,
        developer,
        agent,
        agent_method,
        app,
        app_instance,
        app_instance_method,
        block_number,
        sequences,
        sequences1,
        sequences2,
        data,
        status: JobStatus::Pending,
        attempts: 0,
        interval_ms,
        next_scheduled_at,
        created_at: now,
        updated_at: now,
    };

    // Emit creation event with all metadata
    event::emit(JobCreatedEvent {
        job_sequence,
        description,
        developer,
        agent,
        agent_method,
        app,
        app_instance,
        app_instance_method,
        block_number,
        sequences,
        sequences1,
        sequences2,
        data,
        status: JobStatus::Pending,
        created_at: now,
    });

    // Add to storage
    object_table::add(&mut jobs.jobs, job_sequence, job);
    add_pending_job(jobs, job_sequence);

    jobs.next_job_sequence = job_sequence + 1;

    job_sequence
}

// Update job status to running - returns true if successful, false otherwise
public fun start_job(
    jobs: &mut Jobs,
    job_sequence: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    let now = clock::timestamp_ms(clock);

    // Check if job exists
    if (!object_table::contains(&jobs.jobs, job_sequence)) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"start"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"job_not_found"),
            current_status: option::none(),
            timestamp: now,
        });
        return false
    };

    let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);

    // Check if job is in Pending status
    if (job.status != JobStatus::Pending) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"start"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"wrong_status"),
            current_status: option::some(job.status),
            timestamp: now,
        });
        return false
    };

    // If periodic, enforce that it's due
    if (is_periodic_job(job)) {
        let due_at = *option::borrow(&job.next_scheduled_at);
        if (now < due_at) {
            event::emit(JobOperationFailedEvent {
                operation: string::utf8(b"start"),
                job_sequence,
                coordinator: ctx.sender(),
                reason: string::utf8(b"not_due_yet"),
                current_status: option::some(job.status),
                timestamp: now,
            });
            return false
        };
    };

    // Update job
    job.status = JobStatus::Running;
    job.attempts = job.attempts + 1;
    job.updated_at = now;

    event::emit(JobUpdatedEvent {
        job_sequence,
        developer: job.developer,
        agent: job.agent,
        agent_method: job.agent_method,
        app_instance: job.app_instance,
        status: JobStatus::Running,
        attempts: job.attempts,
        updated_at: now,
    });

    // Emit JobStartedEvent for transparency
    event::emit(JobStartedEvent {
        job_sequence,
        app_instance: job.app_instance,
        coordinator: ctx.sender(),
        started_at: now,
    });
    remove_pending_job(jobs, job_sequence);
    true
}

// Mark job as completed and remove it (or reschedule if periodic)
// Only running jobs can be completed - returns true if successful, false otherwise
public fun complete_job(
    jobs: &mut Jobs,
    job_sequence: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    let now = clock::timestamp_ms(clock);

    // Check if job exists
    if (!object_table::contains(&jobs.jobs, job_sequence)) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"complete"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"job_not_found"),
            current_status: option::none(),
            timestamp: now,
        });
        return false
    };

    let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);
    // Check if job is in Running status
    if (job.status != JobStatus::Running) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"complete"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"wrong_status"),
            current_status: option::some(job.status),
            timestamp: now,
        });
        return false
    };

    event::emit(JobCompletedEvent {
        job_sequence,
        app_instance: job.app_instance,
        coordinator: ctx.sender(),
        completed_at: now,
    });

    if (is_periodic_job(job)) {
        // Periodic: reschedule instead of delete
        let interval = *option::borrow(&job.interval_ms);
        let next_run = now + interval;

        job.status = JobStatus::Pending;
        job.attempts = 0;
        job.updated_at = now;
        job.next_scheduled_at = option::some(next_run);

        event::emit(JobUpdatedEvent {
            job_sequence,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            app_instance: job.app_instance,
            status: JobStatus::Pending,
            attempts: 0,
            updated_at: now,
        });

        event::emit(JobRescheduledEvent {
            job_sequence,
            app_instance: job.app_instance,
            next_scheduled_at: next_run,
            rescheduled_at: now,
        });

        add_pending_job(jobs, job_sequence);
    } else {
        // One-time: remove and delete
        event::emit(JobDeletedEvent {
            job_sequence,
            app_instance: job.app_instance,
            deleted_at: now,
        });

        let job = object_table::remove(&mut jobs.jobs, job_sequence);
        let Job { id, .. } = job;
        object::delete(id);
    };
    true
}

// Mark job as failed and retry or remove (or reschedule if periodic)
// Only running jobs can fail - returns true if successful, false otherwise
public fun fail_job(
    jobs: &mut Jobs,
    job_sequence: u64,
    error: String,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    let now = clock::timestamp_ms(clock);

    // Check if job exists
    if (!object_table::contains(&jobs.jobs, job_sequence)) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"fail"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"job_not_found"),
            current_status: option::none(),
            timestamp: now,
        });
        return false
    };

    let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);

    // Job must be in Running state to fail
    if (job.status != JobStatus::Running) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"fail"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"wrong_status"),
            current_status: option::some(job.status),
            timestamp: now,
        });
        return false
    };
    event::emit(JobFailedEvent {
        job_sequence,
        app_instance: job.app_instance,
        coordinator: ctx.sender(),
        error,
        attempts: job.attempts,
        failed_at: now,
    });

    if (job.attempts < jobs.max_attempts) {
        job.status = JobStatus::Pending;
        job.next_scheduled_at = option::some(now + RETRY_INTERVAL_MS);
        job.updated_at = now;
        event::emit(JobUpdatedEvent {
            job_sequence,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            app_instance: job.app_instance,
            status: JobStatus::Pending,
            attempts: job.attempts,
            updated_at: now,
        });
        add_pending_job(jobs, job_sequence);
    } else {
        if (is_periodic_job(job)) {
            job.attempts = 0;
            job.status = JobStatus::Pending;
            let interval = *option::borrow(&job.interval_ms);
            let next_run = now + interval;
            job.next_scheduled_at = option::some(next_run);
            job.updated_at = now;
            event::emit(JobUpdatedEvent {
                job_sequence,
                developer: job.developer,
                agent: job.agent,
                agent_method: job.agent_method,
                app_instance: job.app_instance,
                status: JobStatus::Pending,
                attempts: job.attempts,
                updated_at: now,
            });
            event::emit(JobRescheduledEvent {
                job_sequence,
                app_instance: job.app_instance,
                next_scheduled_at: next_run,
                rescheduled_at: now,
            });
            add_pending_job(jobs, job_sequence);
        } else {
            // Mark as failed but keep in main jobs table
            job.status = JobStatus::Failed(error);
            job.updated_at = now;
            jobs.failed_jobs_count = jobs.failed_jobs_count + 1;
            vec_set::insert(&mut jobs.failed_jobs_index, job_sequence);
        }
    };
    true
}

public fun get_failed_jobs(jobs: &Jobs): vector<u64> {
    vec_set::into_keys(jobs.failed_jobs_index)
}

public fun get_failed_jobs_count(jobs: &Jobs): u64 {
    jobs.failed_jobs_count
}

public fun restart_failed_jobs(
    jobs: &mut Jobs,
    job_sequences: Option<vector<u64>>,
    clock: &Clock,
) {
    let failed_jobs = if (option::is_some(&job_sequences)) {
        *option::borrow(&job_sequences)
    } else {
        get_failed_jobs(jobs)
    };

    let len = vector::length(&failed_jobs);
    let mut i = 0;
    let now = clock::timestamp_ms(clock);

    while (i < len) {
        let job_sequence = *vector::borrow(&failed_jobs, i);

        // Verify job is actually failed
        if (vec_set::contains(&jobs.failed_jobs_index, &job_sequence)) {
            assert!(
                object_table::contains(&jobs.jobs, job_sequence),
                EJobNotFound,
            );

            // Update failed job tracking
            jobs.failed_jobs_count = jobs.failed_jobs_count - 1;
            vec_set::remove(&mut jobs.failed_jobs_index, &job_sequence);

            // Reset job to pending
            let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);
            job.status = JobStatus::Pending;
            job.attempts = 0;
            job.next_scheduled_at = option::none();
            job.updated_at = now;

            event::emit(JobUpdatedEvent {
                job_sequence,
                developer: job.developer,
                agent: job.agent,
                agent_method: job.agent_method,
                app_instance: job.app_instance,
                status: JobStatus::Pending,
                attempts: 0,
                updated_at: now,
            });
            event::emit(JobRestartedEvent {
                job_sequence,
                app_instance: job.app_instance,
                started_at: now,
            });

            add_pending_job(jobs, job_sequence);
        };

        i = i + 1;
    };

    event::emit(JobsRestartedEvent {
        failed_jobs,
        num_failed_jobs: len,
        restarted_at: now,
    });
}

public fun remove_failed_jobs(
    jobs: &mut Jobs,
    job_sequences: Option<vector<u64>>,
    clock: &Clock,
) {
    let failed_jobs = if (option::is_some(&job_sequences)) {
        *option::borrow(&job_sequences)
    } else {
        get_failed_jobs(jobs)
    };

    let len = vector::length(&failed_jobs);
    let mut i = 0;
    let now = clock::timestamp_ms(clock);

    while (i < len) {
        let job_sequence = *vector::borrow(&failed_jobs, i);

        // Verify job is actually failed
        if (vec_set::contains(&jobs.failed_jobs_index, &job_sequence)) {
            assert!(
                object_table::contains(&jobs.jobs, job_sequence),
                EJobNotFound,
            );

            // Update failed job tracking
            jobs.failed_jobs_count = jobs.failed_jobs_count - 1;
            vec_set::remove(&mut jobs.failed_jobs_index, &job_sequence);

            // Remove job from storage completely
            let job = object_table::remove(&mut jobs.jobs, job_sequence);

            event::emit(JobDeletedEvent {
                job_sequence,
                app_instance: job.app_instance,
                deleted_at: now,
            });

            let Job { id, .. } = job;
            object::delete(id);
        };

        i = i + 1;
    }
}

// Terminate a job (one-time or periodic) - removes it completely
// Returns true if successful, false otherwise
public fun terminate_job(
    jobs: &mut Jobs,
    job_sequence: u64,
    clock: &Clock,
    ctx: &TxContext,
): bool {
    let now = clock::timestamp_ms(clock);

    // Check if job exists
    if (!object_table::contains(&jobs.jobs, job_sequence)) {
        event::emit(JobOperationFailedEvent {
            operation: string::utf8(b"terminate"),
            job_sequence,
            coordinator: ctx.sender(),
            reason: string::utf8(b"job_not_found"),
            current_status: option::none(),
            timestamp: now,
        });
        return false
    };

    // Settlement job tracking is now handled per-chain in the Settlement struct
    remove_pending_job(jobs, job_sequence);
    let job = object_table::remove(&mut jobs.jobs, job_sequence);

    event::emit(JobTerminatedEvent {
        job_sequence,
        app_instance: job.app_instance,
        coordinator: ctx.sender(),
        terminated_at: now,
    });

    event::emit(JobDeletedEvent {
        job_sequence,
        app_instance: job.app_instance,
        deleted_at: now,
    });

    let Job { id, .. } = job;
    object::delete(id);
    true
}

// Multicall function to batch execute multiple job operations
// Executes in order: complete, fail, terminate, then start
public fun multicall_job_operations(
    jobs: &mut Jobs,
    complete_job_sequences: vector<u64>,
    fail_job_sequences: vector<u64>,
    fail_errors: vector<String>,
    terminate_job_sequences: vector<u64>,
    start_job_sequences: vector<u64>,
    clock: &Clock,
    ctx: &TxContext,
) {
    // Ensure fail arrays have same length
    assert!(
        vector::length(&fail_job_sequences) == vector::length(&fail_errors),
        EInvalidArguments,
    );
    let now = clock::timestamp_ms(clock);

    // Process complete operations
    let mut complete_results = vector::empty<bool>();
    let mut i = 0;
    let complete_len = vector::length(&complete_job_sequences);
    while (i < complete_len) {
        let job_sequence = *vector::borrow(&complete_job_sequences, i);
        let result = complete_job(jobs, job_sequence, clock, ctx);
        vector::push_back(&mut complete_results, result);
        i = i + 1;
    };

    // Process fail operations
    let mut fail_results = vector::empty<bool>();
    i = 0;
    let fail_len = vector::length(&fail_job_sequences);
    while (i < fail_len) {
        let job_sequence = *vector::borrow(&fail_job_sequences, i);
        let error = *vector::borrow(&fail_errors, i);
        let result = fail_job(jobs, job_sequence, error, clock, ctx);
        vector::push_back(&mut fail_results, result);
        i = i + 1;
    };

    // Process terminate operations
    let mut terminate_results = vector::empty<bool>();
    i = 0;
    let terminate_len = vector::length(&terminate_job_sequences);
    while (i < terminate_len) {
        let job_sequence = *vector::borrow(&terminate_job_sequences, i);
        let result = terminate_job(jobs, job_sequence, clock, ctx);
        vector::push_back(&mut terminate_results, result);
        i = i + 1;
    };

    // Process start operations
    let mut start_results = vector::empty<bool>();
    i = 0;
    let start_len = vector::length(&start_job_sequences);
    while (i < start_len) {
        let job_sequence = *vector::borrow(&start_job_sequences, i);
        let result = start_job(jobs, job_sequence, clock, ctx);
        vector::push_back(&mut start_results, result);
        i = i + 1;
    };

    // Emit multicall event
    event::emit(MulticallExecutedEvent {
        coordinator: ctx.sender(),
        complete_jobs: complete_job_sequences,
        complete_results,
        fail_jobs: fail_job_sequences,
        fail_results,
        terminate_jobs: terminate_job_sequences,
        terminate_results,
        start_jobs: start_job_sequences,
        start_results,
        timestamp: now,
    });
}

fun add_pending_job(jobs: &mut Jobs, job_sequence: u64) {
    // Always ensure the job is in the index (even if already in pending_jobs)
    // This handles cases where pending_jobs and indexes might be out of sync
    let job = object_table::borrow(&jobs.jobs, job_sequence);
    assert!(job.status == JobStatus::Pending, EJobNotPending);
    // Add to pending set if not already present
    if (!vec_set::contains(&jobs.pending_jobs, &job_sequence)) {
        vec_set::insert(&mut jobs.pending_jobs, job_sequence);
        jobs.pending_jobs_count = jobs.pending_jobs_count + 1;
    };
    let developer = job.developer;
    let agent = job.agent;
    let agent_method = job.agent_method;

    // Get or create developer level
    if (!vec_map::contains(&jobs.pending_jobs_indexes, &developer)) {
        vec_map::insert(
            &mut jobs.pending_jobs_indexes,
            developer,
            vec_map::empty(),
        );
    };
    let developer_map = vec_map::get_mut(
        &mut jobs.pending_jobs_indexes,
        &developer,
    );

    // Get or create agent level
    if (!vec_map::contains(developer_map, &agent)) {
        vec_map::insert(developer_map, agent, vec_map::empty());
    };
    let agent_map = vec_map::get_mut(developer_map, &agent);

    // Get or create agent_method level
    if (!vec_map::contains(agent_map, &agent_method)) {
        vec_map::insert(agent_map, agent_method, vec_set::empty());
    };
    let method_set = vec_map::get_mut(agent_map, &agent_method);

    // Add job_sequence to the set if not already there
    if (!vec_set::contains(method_set, &job_sequence)) {
        vec_set::insert(method_set, job_sequence);
    }
}

// Internal helper function to remove a job from pending jobs set, update count and index
// Tolerant to calls when job is not present - ensures job is not in pending set and index after call
fun remove_pending_job(jobs: &mut Jobs, job_sequence: u64) {
    // Only remove if present
    if (vec_set::contains(&jobs.pending_jobs, &job_sequence)) {
        vec_set::remove(&mut jobs.pending_jobs, &job_sequence);
        jobs.pending_jobs_count = jobs.pending_jobs_count - 1;

        // Get job details from storage and remove from index
        let job = object_table::borrow(&jobs.jobs, job_sequence);
        let developer = job.developer;
        let agent = job.agent;
        let agent_method = job.agent_method;

        // Remove from index - tolerant to missing entries
        if (vec_map::contains(&jobs.pending_jobs_indexes, &developer)) {
            let developer_map = vec_map::get_mut(
                &mut jobs.pending_jobs_indexes,
                &developer,
            );

            if (vec_map::contains(developer_map, &agent)) {
                let agent_map = vec_map::get_mut(developer_map, &agent);

                if (vec_map::contains(agent_map, &agent_method)) {
                    let method_set = vec_map::get_mut(agent_map, &agent_method);

                    // Remove job_sequence from the set if it exists
                    if (vec_set::contains(method_set, &job_sequence)) {
                        vec_set::remove(method_set, &job_sequence);
                    };
                };
            };
        };

        // Keep empty structures for reuse - they will likely be needed again soon
    }
}

// Internal helper function to check if a job is periodic
fun is_periodic_job(job: &Job): bool {
    option::is_some(&job.interval_ms)
}

// Update max attempts for the Jobs storage
public fun update_max_attempts(jobs: &mut Jobs, max_attempts: u8) {
    jobs.max_attempts = max_attempts;
}

// Getter functions
public fun get_job(jobs: &Jobs, job_sequence: u64): &Job {
    assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);
    object_table::borrow(&jobs.jobs, job_sequence)
}

// public fun get_job_mut(jobs: &mut Jobs, job_sequence: u64): &mut Job {
//     assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);
//     object_table::borrow_mut(&mut jobs.jobs, job_sequence)
// }

public fun job_exists(jobs: &Jobs, job_sequence: u64): bool {
    object_table::contains(&jobs.jobs, job_sequence)
}

public fun is_job_in_failed_index(jobs: &Jobs, job_sequence: u64): bool {
    vec_set::contains(&jobs.failed_jobs_index, &job_sequence)
}

public fun get_pending_jobs(jobs: &Jobs): vector<u64> {
    vec_set::into_keys(jobs.pending_jobs)
}

public fun get_pending_jobs_count(jobs: &Jobs): u64 {
    jobs.pending_jobs_count
}

// Get pending jobs for a specific developer/agent/agent_method
public fun get_pending_jobs_for_method(
    jobs: &Jobs,
    developer: &String,
    agent: &String,
    agent_method: &String,
): vector<u64> {
    // Check if developer exists
    if (!vec_map::contains(&jobs.pending_jobs_indexes, developer)) {
        return vector::empty()
    };

    let developer_map = vec_map::get(&jobs.pending_jobs_indexes, developer);

    // Check if agent exists
    if (!vec_map::contains(developer_map, agent)) {
        return vector::empty()
    };

    let agent_map = vec_map::get(developer_map, agent);

    // Check if agent_method exists
    if (!vec_map::contains(agent_map, agent_method)) {
        return vector::empty()
    };

    let method_set = vec_map::get(agent_map, agent_method);

    // Return the job IDs
    vec_set::into_keys(*method_set)
}

public fun get_next_pending_job(jobs: &Jobs): Option<u64> {
    if (vec_set::is_empty(&jobs.pending_jobs)) {
        option::none()
    } else {
        let keys = vec_set::into_keys(jobs.pending_jobs);
        option::some(*vector::borrow(&keys, 0))
    }
}

// Job field getters
public fun job_sequence(job: &Job): u64 {
    job.job_sequence
}

public fun job_description(job: &Job): &Option<String> {
    &job.description
}

public fun job_developer(job: &Job): &String {
    &job.developer
}

public fun job_agent(job: &Job): &String {
    &job.agent
}

public fun job_agent_method(job: &Job): &String {
    &job.agent_method
}

public fun job_app(job: &Job): &String {
    &job.app
}

public fun job_app_instance(job: &Job): &String {
    &job.app_instance
}

public fun job_app_instance_method(job: &Job): &String {
    &job.app_instance_method
}

public fun job_block_number(job: &Job): &Option<u64> {
    &job.block_number
}

public fun job_sequences(job: &Job): &Option<vector<u64>> {
    &job.sequences
}

public fun job_sequences1(job: &Job): &Option<vector<u64>> {
    &job.sequences1
}

public fun job_sequences2(job: &Job): &Option<vector<u64>> {
    &job.sequences2
}

public fun job_data(job: &Job): &vector<u8> {
    &job.data
}

public fun job_status(job: &Job): &JobStatus {
    &job.status
}

public fun job_attempts(job: &Job): u8 {
    job.attempts
}

public fun job_created_at(job: &Job): u64 {
    job.created_at
}

public fun job_updated_at(job: &Job): u64 {
    job.updated_at
}

// Jobs storage getters
public fun max_attempts(jobs: &Jobs): u8 {
    jobs.max_attempts
}

public fun next_job_sequence(jobs: &Jobs): u64 {
    jobs.next_job_sequence
}

// Status checking helpers
public fun is_job_pending(job: &Job): bool {
    match (&job.status) {
        JobStatus::Pending => true,
        _ => false,
    }
}

public fun is_job_running(job: &Job): bool {
    match (&job.status) {
        JobStatus::Running => true,
        _ => false,
    }
}

public fun is_job_failed(job: &Job): bool {
    match (&job.status) {
        JobStatus::Failed(_) => true,
        _ => false,
    }
}

// Getter functions for periodic task fields
public fun job_interval_ms(job: &Job): &Option<u64> {
    &job.interval_ms
}

public fun job_next_scheduled_at(job: &Job): &Option<u64> {
    &job.next_scheduled_at
}

public fun is_job_periodic(job: &Job): bool {
    is_periodic_job(job)
}
