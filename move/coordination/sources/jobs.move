module coordination::jobs;

use std::string::String;
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
    pending_jobs: VecSet<u64>,
    pending_jobs_count: u64,
    // index: developer -> agent -> agent_method -> job_id for pending jobs
    pending_jobs_indexes: VecMap<
        String,
        VecMap<String, VecMap<String, VecSet<u64>>>,
    >,
    next_job_sequence: u64,
    max_attempts: u8,
    settlement_job: Option<u64>,
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
    started_at: u64,
}

public struct JobCompletedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    completed_at: u64,
}

public struct JobFailedEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
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
    terminated_at: u64,
}

public struct JobRescheduledEvent has copy, drop {
    job_sequence: u64,
    app_instance: String,
    next_scheduled_at: u64,
    rescheduled_at: u64,
}

// Constants
const DEFAULT_MAX_ATTEMPTS: u8 = 3;
const MIN_INTERVAL_MS: u64 = 60_000; // >= 1 minute for periodic tasks

// Error codes
#[error]
const EJobNotFound: vector<u8> = b"Job not found";

#[error]
const EJobNotPending: vector<u8> = b"Job is not in pending state";

#[error]
const EJobNotRunning: vector<u8> = b"Job is not in running state";

#[error]
const EIntervalTooShort: vector<u8> = b"Interval must be >= 60000 ms";

#[error]
const ENotDueYet: vector<u8> = b"Periodic job is not due yet";

#[error]
const ESettlementJobAlreadyExists: vector<u8> =
    b"Settlement job already exists";

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
        pending_jobs: vec_set::empty(),
        pending_jobs_count: 0,
        pending_jobs_indexes: vec_map::empty(),
        next_job_sequence: 1,
        max_attempts: attempts,
        settlement_job: option::none(),
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
    is_settlement_job: bool,
    clock: &Clock,
    ctx: &mut TxContext,
): u64 {
    // Validate interval if provided
    if (option::is_some(&interval_ms)) {
        let interval = *option::borrow(&interval_ms);
        assert!(interval >= MIN_INTERVAL_MS, EIntervalTooShort);
    };

    let job_sequence = jobs.next_job_sequence;
    let timestamp = clock::timestamp_ms(clock);
    if (is_settlement_job) {
        assert!(
            option::is_none(&jobs.settlement_job),
            ESettlementJobAlreadyExists,
        );
        jobs.settlement_job = option::some(job_sequence);
    };

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
        created_at: timestamp,
        updated_at: timestamp,
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
        created_at: timestamp,
    });

    // Add to storage
    object_table::add(&mut jobs.jobs, job_sequence, job);
    add_pending_job(jobs, job_sequence);

    jobs.next_job_sequence = job_sequence + 1;

    job_sequence
}

// Update job status to running
public fun start_job(jobs: &mut Jobs, job_sequence: u64, clock: &Clock) {
    assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);

    let now = clock::timestamp_ms(clock);

    // First, get the values we need and update the job
    let (developer, agent, agent_method, app_instance, attempts) = {
        let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);
        assert!(job.status == JobStatus::Pending, EJobNotPending);

        // If periodic, enforce that it's due
        if (is_periodic_job(job)) {
            // next_scheduled_at must be Some for periodic
            let due_at = *option::borrow(&job.next_scheduled_at);
            assert!(now >= due_at, ENotDueYet);
        };

        // Copy values we need
        let developer = job.developer;
        let agent = job.agent;
        let agent_method = job.agent_method;
        let app_instance = job.app_instance;
        let attempts = job.attempts + 1;

        // Update job
        job.status = JobStatus::Running;
        job.attempts = attempts;
        job.updated_at = now;

        (developer, agent, agent_method, app_instance, attempts)
    }; // Mutable borrow of job ends here

    // Remove from pending set and index
    remove_pending_job(jobs, job_sequence);

    event::emit(JobUpdatedEvent {
        job_sequence,
        developer,
        agent,
        agent_method,
        app_instance,
        status: JobStatus::Running,
        attempts,
        updated_at: now,
    });

    // Emit JobStartedEvent for transparency
    event::emit(JobStartedEvent {
        job_sequence,
        app_instance,
        started_at: now,
    });
}

// Mark job as completed and remove it (or reschedule if periodic)
// Only running jobs can be completed
public fun complete_job(jobs: &mut Jobs, job_sequence: u64, clock: &Clock) {
    assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);

    let now = clock::timestamp_ms(clock);

    // Check if job is periodic and reschedule if needed
    let (
        is_periodic,
        app_instance,
        developer,
        agent,
        agent_method,
        next_run,
    ) = {
        let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);
        assert!(job.status == JobStatus::Running, EJobNotRunning);

        let app_instance = job.app_instance;
        let developer = job.developer;
        let agent = job.agent;
        let agent_method = job.agent_method;

        if (is_periodic_job(job)) {
            // Periodic: reschedule instead of delete
            let interval = *option::borrow(&job.interval_ms);
            let next_run = now + interval;

            job.status = JobStatus::Pending;
            job.attempts = 0;
            job.updated_at = now;
            job.next_scheduled_at = option::some(next_run);

            (true, app_instance, developer, agent, agent_method, next_run)
        } else {
            // One-time job
            (false, app_instance, developer, agent, agent_method, 0)
        }
    };

    // Always emit completion for this run
    event::emit(JobCompletedEvent {
        job_sequence,
        app_instance,
        completed_at: now,
    });

    if (is_periodic) {
        // Re-add to pending and index
        add_pending_job(jobs, job_sequence);

        event::emit(JobUpdatedEvent {
            job_sequence,
            developer,
            agent,
            agent_method,
            app_instance,
            status: JobStatus::Pending,
            attempts: 0,
            updated_at: now,
        });

        event::emit(JobRescheduledEvent {
            job_sequence,
            app_instance,
            next_scheduled_at: next_run,
            rescheduled_at: now,
        });
    } else {
        // One-time: remove and delete
        let job = object_table::remove(&mut jobs.jobs, job_sequence);

        event::emit(JobDeletedEvent {
            job_sequence,
            app_instance,
            deleted_at: now,
        });

        let Job { id, .. } = job;
        object::delete(id);
    }
}

// Mark job as failed and retry or remove (or reschedule if periodic)
// Only running jobs can fail
public fun fail_job(
    jobs: &mut Jobs,
    job_sequence: u64,
    error: String,
    clock: &Clock,
) {
    assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);

    let now = clock::timestamp_ms(clock);

    // Get job info and determine if we should retry
    let (
        developer,
        agent,
        agent_method,
        app_instance,
        attempts,
        should_retry,
        periodic,
        next_run,
    ) = {
        let job = object_table::borrow_mut(&mut jobs.jobs, job_sequence);

        // Job must be in Running state to fail
        assert!(job.status == JobStatus::Running, EJobNotRunning);

        // Copy values we need
        let developer = job.developer;
        let agent = job.agent;
        let agent_method = job.agent_method;
        let app_instance = job.app_instance;
        let attempts = job.attempts; // attempts already incremented in start_job
        let should_retry = attempts < jobs.max_attempts;
        let periodic = is_periodic_job(job);

        // Mark failed (this run)
        job.status = JobStatus::Failed(error);
        job.updated_at = now;

        let mut next_run: u64 = 0;
        if (!should_retry && periodic) {
            // Reached max attempts for this run -> schedule next interval
            let interval = *option::borrow(&job.interval_ms);
            next_run = now + interval;
            job.status = JobStatus::Pending;
            job.attempts = 0;
            job.next_scheduled_at = option::some(next_run);
        } else if (should_retry) {
            // Will retry within current interval window
            job.status = JobStatus::Pending;
        };

        (
            developer,
            agent,
            agent_method,
            app_instance,
            attempts,
            should_retry,
            periodic,
            next_run,
        )
    }; // Mutable borrow ends here

    event::emit(JobFailedEvent {
        job_sequence,
        app_instance,
        error,
        attempts,
        failed_at: now,
    });

    // Check if we should retry or reschedule
    if (should_retry || periodic) {
        // Put back to pending and index
        add_pending_job(jobs, job_sequence);

        event::emit(JobUpdatedEvent {
            job_sequence,
            developer,
            agent,
            agent_method,
            app_instance,
            status: JobStatus::Pending,
            attempts: if (should_retry) attempts else 0,
            updated_at: now,
        });

        if (!should_retry && periodic) {
            // We scheduled the next interval
            event::emit(JobRescheduledEvent {
                job_sequence,
                app_instance,
                next_scheduled_at: next_run,
                rescheduled_at: now,
            });
        }
    } else {
        // One-time job & no retries left -> delete
        // Remove from pending tracking first (needs job details from storage)
        remove_pending_job(jobs, job_sequence);

        let job = object_table::remove(&mut jobs.jobs, job_sequence);

        event::emit(JobDeletedEvent {
            job_sequence,
            app_instance,
            deleted_at: now,
        });

        let Job { id, .. } = job;
        object::delete(id);
    }
}

// Terminate a job (one-time or periodic) - removes it completely
public fun terminate_job(jobs: &mut Jobs, job_sequence: u64, clock: &Clock) {
    assert!(object_table::contains(&jobs.jobs, job_sequence), EJobNotFound);

    if (
        option::is_some(&jobs.settlement_job)
        && *option::borrow(&jobs.settlement_job) == job_sequence
    ) {
        jobs.settlement_job = option::none();
    };

    let now = clock::timestamp_ms(clock);

    remove_pending_job(jobs, job_sequence);

    // Remove and delete the job
    let job = object_table::remove(&mut jobs.jobs, job_sequence);

    event::emit(JobTerminatedEvent {
        job_sequence,
        app_instance: job.app_instance,
        terminated_at: now,
    });

    event::emit(JobDeletedEvent {
        job_sequence,
        app_instance: job.app_instance,
        deleted_at: now,
    });

    let Job { id, .. } = job;
    object::delete(id);
}

// Internal helper function to add a job to pending jobs set, update count and index
// Tolerant to duplicate calls - ensures job is in pending set and index after call
fun add_pending_job(jobs: &mut Jobs, job_sequence: u64) {
    // Add to pending set if not already present
    if (!vec_set::contains(&jobs.pending_jobs, &job_sequence)) {
        vec_set::insert(&mut jobs.pending_jobs, job_sequence);
        jobs.pending_jobs_count = jobs.pending_jobs_count + 1;
    };

    // Always ensure the job is in the index (even if already in pending_jobs)
    // This handles cases where pending_jobs and indexes might be out of sync
    let job = object_table::borrow(&jobs.jobs, job_sequence);
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
