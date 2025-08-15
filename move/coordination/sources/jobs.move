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
    pending_jobs_count: u64,
    // index: developer -> agent -> app_method -> job_id for pending jobs
    pending_jobs_indexes: VecMap<
        String,
        VecMap<String, VecMap<String, VecSet<u64>>>,
    >,
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
    developer: String,
    agent: String,
    agent_method: String,
    app_instance: String,
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

#[error]
const EJobNotRunning: vector<u8> = b"Job is not in running state";

#[error]
const EJobNotInIndex: vector<u8> = b"Job not found in pending jobs index";

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
    
    // Update pending jobs count
    jobs.pending_jobs_count = jobs.pending_jobs_count + 1;
    
    // Add to index
    add_to_index(jobs, job_id, &developer, &agent, &agent_method);
    
    jobs.next_job_id = job_id + 1;

    job_id
}

// Update job status to running
public fun start_job(jobs: &mut Jobs, job_id: u64, clock: &Clock) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);

    // First, get the values we need and update the job
    let (developer, agent, agent_method, app_instance, attempts) = {
        let job = object_table::borrow_mut(&mut jobs.jobs, job_id);
        assert!(job.status == JobStatus::Pending, EJobNotPending);
        
        // Copy values we need
        let dev = job.developer;
        let ag = job.agent;
        let meth = job.agent_method;
        let app_inst = job.app_instance;
        let att = job.attempts + 1;
        
        // Update job
        job.status = JobStatus::Running;
        job.attempts = att;
        job.updated_at = clock::timestamp_ms(clock);
        
        (dev, ag, meth, app_inst, att)
    }; // Mutable borrow of job ends here

    // Remove from pending set
    vec_set::remove(&mut jobs.pending_jobs, &job_id);
    
    // Update pending jobs count
    jobs.pending_jobs_count = jobs.pending_jobs_count - 1;
    
    // Remove from index
    remove_from_index(jobs, job_id, &developer, &agent, &agent_method);

    event::emit(JobUpdatedEvent {
        job_id,
        developer,
        agent,
        agent_method,
        app_instance,
        status: JobStatus::Running,
        attempts,
        updated_at: clock::timestamp_ms(clock),
    });
}

// Mark job as completed and remove it
// Only running jobs can be completed
public fun complete_job(jobs: &mut Jobs, job_id: u64, clock: &Clock) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);

    let job = object_table::remove(&mut jobs.jobs, job_id);
    
    // Job must be in Running state to be completed
    assert!(job.status == JobStatus::Running, EJobNotRunning);
    
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
// Only running jobs can fail
public fun fail_job(
    jobs: &mut Jobs,
    job_id: u64,
    error: String,
    clock: &Clock,
) {
    assert!(object_table::contains(&jobs.jobs, job_id), EJobNotFound);
    
    let timestamp = clock::timestamp_ms(clock);
    
    // Get job info and determine if we should retry
    let (developer, agent, agent_method, app_instance, attempts, should_retry) = {
        let job = object_table::borrow_mut(&mut jobs.jobs, job_id);
        
        // Job must be in Running state to fail
        assert!(job.status == JobStatus::Running, EJobNotRunning);
        
        // Copy values we need
        let dev = job.developer;
        let ag = job.agent;
        let meth = job.agent_method;
        let app_inst = job.app_instance;
        let att = job.attempts;
        let retry = att < jobs.max_attempts;
        
        // Update job status
        job.status = JobStatus::Failed(error);
        job.updated_at = timestamp;
        
        // If retrying, set back to pending
        if (retry) {
            job.status = JobStatus::Pending;
        };
        
        (dev, ag, meth, app_inst, att, retry)
    }; // Mutable borrow ends here

    event::emit(JobFailedEvent {
        job_id,
        app_instance,
        error,
        attempts,
        failed_at: timestamp,
    });

    // Check if we should retry
    if (should_retry) {
        // Add back to pending set
        vec_set::insert(&mut jobs.pending_jobs, job_id);
        
        // Update pending jobs count (job is being added to pending from running state)
        jobs.pending_jobs_count = jobs.pending_jobs_count + 1;
        
        // Add to index (it wasn't in index since it was running)
        add_to_index(jobs, job_id, &developer, &agent, &agent_method);

        event::emit(JobUpdatedEvent {
            job_id,
            developer,
            agent,
            agent_method,
            app_instance,
            status: JobStatus::Pending,
            attempts,
            updated_at: timestamp,
        });
    } else {
        // Max attempts reached, remove the job
        let job = object_table::remove(&mut jobs.jobs, job_id);
        
        // If job was pending (shouldn't be, but just in case), update count and index
        if (vec_set::contains(&jobs.pending_jobs, &job_id)) {
            vec_set::remove(&mut jobs.pending_jobs, &job_id);
            jobs.pending_jobs_count = jobs.pending_jobs_count - 1;
            remove_from_index(jobs, job_id, &job.developer, &job.agent, &job.agent_method);
        };

        event::emit(JobDeletedEvent {
            job_id,
            deleted_at: timestamp,
        });

        let Job { id, .. } = job;
        object::delete(id);
    }
}

// Internal helper function to add a job to the index
fun add_to_index(
    jobs: &mut Jobs,
    job_id: u64,
    developer: &String,
    agent: &String,
    agent_method: &String,
) {
    // Get or create developer level
    if (!vec_map::contains(&jobs.pending_jobs_indexes, developer)) {
        vec_map::insert(&mut jobs.pending_jobs_indexes, *developer, vec_map::empty());
    };
    let developer_map = vec_map::get_mut(&mut jobs.pending_jobs_indexes, developer);
    
    // Get or create agent level
    if (!vec_map::contains(developer_map, agent)) {
        vec_map::insert(developer_map, *agent, vec_map::empty());
    };
    let agent_map = vec_map::get_mut(developer_map, agent);
    
    // Get or create agent_method level
    if (!vec_map::contains(agent_map, agent_method)) {
        vec_map::insert(agent_map, *agent_method, vec_set::empty());
    };
    let method_set = vec_map::get_mut(agent_map, agent_method);
    
    // Add job_id to the set
    vec_set::insert(method_set, job_id);
}

// Internal helper function to remove a job from the index
// Asserts that the job exists in the index
fun remove_from_index(
    jobs: &mut Jobs,
    job_id: u64,
    developer: &String,
    agent: &String,
    agent_method: &String,
) {
    // Developer must exist in index
    assert!(vec_map::contains(&jobs.pending_jobs_indexes, developer), EJobNotInIndex);
    let developer_map = vec_map::get_mut(&mut jobs.pending_jobs_indexes, developer);
    
    // Agent must exist
    assert!(vec_map::contains(developer_map, agent), EJobNotInIndex);
    let agent_map = vec_map::get_mut(developer_map, agent);
    
    // Agent_method must exist
    assert!(vec_map::contains(agent_map, agent_method), EJobNotInIndex);
    let method_set = vec_map::get_mut(agent_map, agent_method);
    
    // Job must exist in the set
    assert!(vec_set::contains(method_set, &job_id), EJobNotInIndex);
    
    // Remove job_id from the set
    vec_set::remove(method_set, &job_id);
    
    // Keep empty structures for reuse - they will likely be needed again soon
}

// Update max attempts for the Jobs storage
public fun update_max_attempts(jobs: &mut Jobs, max_attempts: u8) {
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
