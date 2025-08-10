module coordination::jobs;

use std::string::String;
use sui::object_table::{Self, ObjectTable};
use sui::vec_set::{Self, VecSet};
use sui::clock::{Self, Clock};
use sui::event;

public enum JobStatus has copy, drop, store {
    Pending,
    Running,
    Failed(String),
}

public struct Job has key, store {
    id: UID,
    job_id: u64,
    description: Option<String>,
    // Metadata of the agent method to call
    developer: String,
    agent: String,
    agent_method: String,
    // Metadata of the calling app instance
    app: String,
    app_instance: String,
    app_instance_method: String,
    sequences: Option<vector<u64>>,
    data: vector<u8>,
    // Metadata of the job
    status: JobStatus,
    attempts: u8,
    // Metadata of the job
    created_at: u64,
    updated_at: u64,
}

public struct Jobs has key, store {
    id: UID,
    jobs: ObjectTable<u64, Job>,
    pending_jobs: VecSet<u64>,
    next_job_id: u64,
    max_attempts: u8,
}

public struct JobCreatedEvent has copy, drop {
    job_id: u64,
    description: Option<String>,
    developer: String,
    agent: String,
    agent_method: String,
    app: String,
    app_instance: String,
    app_instance_method: String,
    sequences: Option<vector<u64>>,
    data: vector<u8>,
    status: JobStatus,
    created_at: u64,
}

public struct JobUpdatedEvent has copy, drop {
    job_id: u64,
    status: JobStatus,
    attempts: u8,
    updated_at: u64,
}

public struct JobCompletedEvent has copy, drop {
    job_id: u64,
    app_instance: String,
    completed_at: u64,
}

public struct JobFailedEvent has copy, drop {
    job_id: u64,
    app_instance: String,
    error: String,
    attempts: u8,
    failed_at: u64,
}

public struct JobDeletedEvent has copy, drop {
    job_id: u64,
    deleted_at: u64,
}

// Constants
const DEFAULT_MAX_ATTEMPTS: u8 = 3;

// Error codes
#[error]
const EJobNotFound: vector<u8> = b"Job not found";

#[error]
const EJobNotPending: vector<u8> = b"Job is not in pending state";

// Initialize Jobs storage with optional max_attempts
public fun create_jobs(
    max_attempts: Option<u8>,
    ctx: &mut TxContext,
): Jobs {
    let attempts = if (option::is_some(&max_attempts)) {
        *option::borrow(&max_attempts)
    } else {
        DEFAULT_MAX_ATTEMPTS
    };
    
    Jobs {
        id: object::new(ctx),
        jobs: object_table::new(ctx),
        pending_jobs: vec_set::empty(),
        next_job_id: 1,
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
    sequences: Option<vector<u64>>,
    data: vector<u8>,
    clock: &Clock,
    ctx: &mut TxContext,
): u64 {
    let job_id = jobs.next_job_id;
    let timestamp = clock::timestamp_ms(clock);
    
    let job = Job {
        id: object::new(ctx),
        job_id,
        description,
        developer,
        agent,
        agent_method,
        app,
        app_instance,
        app_instance_method,
        sequences,
        data,
        status: JobStatus::Pending,
        attempts: 0,
        created_at: timestamp,
        updated_at: timestamp,
    };

    // Emit creation event with all metadata
    event::emit(JobCreatedEvent {
        job_id,
        description,
        developer,
        agent,
        agent_method,
        app,
        app_instance,
        app_instance_method,
        sequences,
        data,
        status: JobStatus::Pending,
        created_at: timestamp,
    });

    // Add to storage
    object_table::add(&mut jobs.jobs, job_id, job);
    vec_set::insert(&mut jobs.pending_jobs, job_id);
    jobs.next_job_id = job_id + 1;
    
    job_id
}

// Update job status to running
public fun start_job(
    jobs: &mut Jobs,
    job_id: u64,
    clock: &Clock,
) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    
    let job = object_table::borrow_mut(&mut jobs.jobs, job_id);
    assert!(job.status == JobStatus::Pending, EJobNotPending);
    
    job.status = JobStatus::Running;
    job.attempts = job.attempts + 1;
    job.updated_at = clock::timestamp_ms(clock);
    
    // Remove from pending set
    vec_set::remove(&mut jobs.pending_jobs, &job_id);
    
    event::emit(JobUpdatedEvent {
        job_id,
        status: JobStatus::Running,
        attempts: job.attempts,
        updated_at: job.updated_at,
    });
}

// Mark job as completed and remove it
public fun complete_job(
    jobs: &mut Jobs,
    job_id: u64,
    clock: &Clock,
) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    
    let job = object_table::remove(&mut jobs.jobs, job_id);
    let timestamp = clock::timestamp_ms(clock);
    
    event::emit(JobCompletedEvent {
        job_id,
        app_instance: job.app_instance,
        completed_at: timestamp,
    });
    
    event::emit(JobDeletedEvent {
        job_id,
        deleted_at: timestamp,
    });
    
    // Delete the job object
    let Job { id, .. } = job;
    object::delete(id);
}

// Mark job as failed and retry or remove
public fun fail_job(
    jobs: &mut Jobs,
    job_id: u64,
    error: String,
    clock: &Clock,
) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    
    let job = object_table::borrow_mut(&mut jobs.jobs, job_id);
    let timestamp = clock::timestamp_ms(clock);
    
    job.status = JobStatus::Failed(error);
    job.updated_at = timestamp;
    
    event::emit(JobFailedEvent {
        job_id,
        app_instance: job.app_instance,
        error,
        attempts: job.attempts,
        failed_at: timestamp,
    });
    
    // Check if we should retry
    if (job.attempts < jobs.max_attempts) {
        // Add back to pending for retry
        job.status = JobStatus::Pending;
        vec_set::insert(&mut jobs.pending_jobs, job_id);
        
        event::emit(JobUpdatedEvent {
            job_id,
            status: JobStatus::Pending,
            attempts: job.attempts,
            updated_at: timestamp,
        });
    } else {
        // Max attempts reached, remove the job
        let job = object_table::remove(&mut jobs.jobs, job_id);
        
        event::emit(JobDeletedEvent {
            job_id,
            deleted_at: timestamp,
        });
        
        let Job { id, .. } = job;
        object::delete(id);
    }
}

// Update max attempts for the Jobs storage
public fun update_max_attempts(
    jobs: &mut Jobs,
    max_attempts: u8,
) {
    jobs.max_attempts = max_attempts;
}

// Getter functions
public fun get_job(jobs: &Jobs, job_id: u64): &Job {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    object_table::borrow(&jobs.jobs, job_id)
}

public fun get_job_mut(jobs: &mut Jobs, job_id: u64): &mut Job {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    object_table::borrow_mut(&mut jobs.jobs, job_id)
}

public fun job_exists(jobs: &Jobs, job_id: u64): bool {
    object_table::contains(&jobs.jobs, job_id)
}

public fun get_pending_jobs(jobs: &Jobs): vector<u64> {
    vec_set::into_keys(jobs.pending_jobs)
}

public fun get_pending_jobs_count(jobs: &Jobs): u64 {
    vec_set::size(&jobs.pending_jobs)
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
public fun job_id(job: &Job): u64 {
    job.job_id
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

public fun job_sequences(job: &Job): &Option<vector<u64>> {
    &job.sequences
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

public fun next_job_id(jobs: &Jobs): u64 {
    jobs.next_job_id
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
