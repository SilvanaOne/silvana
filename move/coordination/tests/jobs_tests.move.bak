#[test_only]
module coordination::jobs_tests;

use coordination::jobs::{
    Self,
    create_jobs,
    create_job,
    start_job,
    complete_job,
    fail_job,
    terminate_job,
    restart_failed_jobs,
    remove_failed_jobs,
    get_job,
    job_exists,
    is_job_in_failed_index,
    get_pending_jobs,
    get_pending_jobs_count,
    get_pending_jobs_for_method,
    get_next_pending_job,
    get_failed_jobs_count,
    update_max_attempts,
    job_sequence,
    job_attempts,
    max_attempts,
    is_job_pending,
    is_job_running,
    is_job_failed,
};
use sui::clock::{Self, Clock};
use sui::test_scenario::{Self as ts, Scenario};

const TEST_ADDR: address = @0x1;

fun setup_test(): (Scenario, Clock) {
    let mut scenario = ts::begin(TEST_ADDR);
    let clock = clock::create_for_testing(ts::ctx(&mut scenario));
    (scenario, clock)
}

#[test]
fun test_create_jobs_with_default_max_attempts() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        assert!(max_attempts(&jobs) == 3, 0);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_create_jobs_with_custom_max_attempts() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let jobs = create_jobs(option::some(5), ts::ctx(&mut scenario));
        assert!(max_attempts(&jobs) == 5, 0);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_create_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::some(b"Test job".to_string()),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::some(vector[1, 2, 3]),
            option::none(),
            option::none(),
            b"test_data",
            option::none(), // interval_ms - not periodic
            option::none(), // next_scheduled_at - not periodic  
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        assert!(job_sequence == 1, 0);
        assert!(job_exists(&jobs, job_sequence), 1);
        assert!(get_pending_jobs_count(&jobs) == 1, 2);
        
        let job = get_job(&jobs, job_sequence);
        assert!(job_sequence(job) == 1, 3);
        assert!(is_job_pending(job), 4);
        assert!(job_attempts(job) == 0, 5);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_start_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"test_data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Start the job
        start_job(&mut jobs, job_sequence, &clock);
        
        let job = get_job(&jobs, job_sequence);
        assert!(is_job_running(job), 0);
        assert!(job_attempts(job) == 1, 1);
        
        // Check that job was removed from pending
        assert!(get_pending_jobs_count(&jobs) == 0, 2);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::EJobNotPending)]
fun test_start_job_not_pending() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"test_data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        start_job(&mut jobs, job_sequence, &clock);
        // This should fail - trying to start an already running job
        start_job(&mut jobs, job_sequence, &clock);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_complete_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"test_data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        start_job(&mut jobs, job_sequence, &clock);
        complete_job(&mut jobs, job_sequence, &clock);
        
        // Job should be deleted
        assert!(!job_exists(&jobs, job_sequence), 0);
        assert!(get_pending_jobs_count(&jobs) == 0, 1);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_fail_job_with_retry() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(3), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"test_data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // First attempt
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 1, 0);
        
        // Fail the job - should go back to pending
        fail_job(&mut jobs, job_sequence, b"Error 1".to_string(), &clock);
        assert!(job_exists(&jobs, job_sequence), 1);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 2);
        assert!(get_pending_jobs_count(&jobs) == 1, 3);
        
        // Second attempt
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 2, 4);
        fail_job(&mut jobs, job_sequence, b"Error 2".to_string(), &clock);
        assert!(job_exists(&jobs, job_sequence), 5);
        
        // Third attempt
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 3, 6);
        
        // Fail after max attempts - job should be marked as failed
        fail_job(&mut jobs, job_sequence, b"Error 3".to_string(), &clock);
        assert!(job_exists(&jobs, job_sequence), 7);
        assert!(is_job_failed(get_job(&jobs, job_sequence)), 8);
        assert!(is_job_in_failed_index(&jobs, job_sequence), 9);
        assert!(get_failed_jobs_count(&jobs) == 1, 10);
        assert!(get_pending_jobs_count(&jobs) == 0, 11);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_multiple_pending_jobs() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        // Create multiple jobs
        let _job_sequence1 = create_job(
            &mut jobs,
            option::some(b"Job 1".to_string()),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data1",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job_sequence2 = create_job(
            &mut jobs,
            option::some(b"Job 2".to_string()),
            b"dev2".to_string(),
            b"agent2".to_string(),
            b"method2".to_string(),
            b"app2".to_string(),
            b"instance2".to_string(),
            b"app_method2".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data2",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let _job_sequence3 = create_job(
            &mut jobs,
            option::some(b"Job 3".to_string()),
            b"dev3".to_string(),
            b"agent3".to_string(),
            b"method3".to_string(),
            b"app3".to_string(),
            b"instance3".to_string(),
            b"app_method3".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data3",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        assert!(get_pending_jobs_count(&jobs) == 3, 0);
        
        let pending = get_pending_jobs(&jobs);
        assert!(vector::length(&pending) == 3, 1);
        
        // Get next pending job
        let next = get_next_pending_job(&jobs);
        assert!(option::is_some(&next), 2);
        
        // Start one job
        start_job(&mut jobs, job_sequence2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 2, 3);
        
        // Complete one job
        complete_job(&mut jobs, job_sequence2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 2, 4);
        assert!(!job_exists(&jobs, job_sequence2), 5);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_update_max_attempts() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(3), ts::ctx(&mut scenario));
        assert!(max_attempts(&jobs) == 3, 0);
        
        update_max_attempts(&mut jobs, 5);
        assert!(max_attempts(&jobs) == 5, 1);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::EJobNotFound)]
fun test_get_nonexistent_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        // This should fail - job doesn't exist
        let _job = get_job(&jobs, 999);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_get_next_pending_job_empty() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let next = get_next_pending_job(&jobs);
        assert!(option::is_none(&next), 0);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_pending_jobs_index() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        // Create jobs for same developer/agent/method
        let job_sequence1 = create_job(
            &mut jobs,
            option::some(b"Job 1".to_string()),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data1",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job_sequence2 = create_job(
            &mut jobs,
            option::some(b"Job 2".to_string()),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app2".to_string(),
            b"instance2".to_string(),
            b"app_method2".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data2",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Create job for different method
        let _job_sequence3 = create_job(
            &mut jobs,
            option::some(b"Job 3".to_string()),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method2".to_string(),
            b"app3".to_string(),
            b"instance3".to_string(),
            b"app_method3".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data3",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Check index for method1
        let method1_jobs = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::length(&method1_jobs) == 2, 0);
        assert!(vector::contains(&method1_jobs, &job_sequence1), 1);
        assert!(vector::contains(&method1_jobs, &job_sequence2), 2);
        
        // Check index for method2
        let method2_jobs = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method2".to_string(),
        );
        assert!(vector::length(&method2_jobs) == 1, 3);
        
        // Check non-existent method
        let no_jobs = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method_nonexistent".to_string(),
        );
        assert!(vector::is_empty(&no_jobs), 4);
        
        // Start job1 - should remove from index
        start_job(&mut jobs, job_sequence1, &clock);
        let method1_jobs_after = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::length(&method1_jobs_after) == 1, 5);
        assert!(vector::contains(&method1_jobs_after, &job_sequence2), 6);
        assert!(!vector::contains(&method1_jobs_after, &job_sequence1), 7);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_pending_jobs_count_tracking() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        // Initially no pending jobs
        assert!(get_pending_jobs_count(&jobs) == 0, 0);
        
        // Create first job
        let job_sequence1 = create_job(
            &mut jobs,
            option::none(),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data1",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        assert!(get_pending_jobs_count(&jobs) == 1, 1);
        
        // Create second job
        let job_sequence2 = create_job(
            &mut jobs,
            option::none(),
            b"dev2".to_string(),
            b"agent2".to_string(),
            b"method2".to_string(),
            b"app2".to_string(),
            b"instance2".to_string(),
            b"app_method2".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data2",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        assert!(get_pending_jobs_count(&jobs) == 2, 2);
        
        // Start job1 - count should decrease
        start_job(&mut jobs, job_sequence1, &clock);
        assert!(get_pending_jobs_count(&jobs) == 1, 3);
        
        // Fail job1 - should go back to pending, count increases
        fail_job(&mut jobs, job_sequence1, b"Error".to_string(), &clock);
        assert!(get_pending_jobs_count(&jobs) == 2, 4);
        
        // Start and complete job1 - count should decrease
        start_job(&mut jobs, job_sequence1, &clock);
        assert!(get_pending_jobs_count(&jobs) == 1, 5);
        complete_job(&mut jobs, job_sequence1, &clock);
        assert!(get_pending_jobs_count(&jobs) == 1, 6);
        
        // Start and complete job2
        start_job(&mut jobs, job_sequence2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 0, 7);
        complete_job(&mut jobs, job_sequence2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 0, 8);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::EJobNotRunning)]
fun test_complete_job_not_running() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Try to complete a pending job - should fail
        complete_job(&mut jobs, job_sequence, &clock);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::EJobNotRunning)]
fun test_fail_job_not_running() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Try to fail a pending job - should fail
        fail_job(&mut jobs, job_sequence, b"Error".to_string(), &clock);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_index_with_retry() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(2), ts::ctx(&mut scenario));
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::none(),  // interval_ms
            option::none(),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Job should be in index
        let jobs_in_index = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::contains(&jobs_in_index, &job_sequence), 0);
        
        // Start job - should be removed from index
        start_job(&mut jobs, job_sequence, &clock);
        let jobs_after_start = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::is_empty(&jobs_after_start), 1);
        
        // Fail job (first attempt) - should be back in index
        fail_job(&mut jobs, job_sequence, b"Error".to_string(), &clock);
        let jobs_after_fail = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::contains(&jobs_after_fail, &job_sequence), 2);
        
        // Start again and fail on max attempts - should be marked as failed
        start_job(&mut jobs, job_sequence, &clock);
        fail_job(&mut jobs, job_sequence, b"Error 2".to_string(), &clock);
        
        // Job should exist but be marked as failed (max attempts reached)
        assert!(job_exists(&jobs, job_sequence), 3);
        assert!(is_job_failed(get_job(&jobs, job_sequence)), 4);
        assert!(is_job_in_failed_index(&jobs, job_sequence), 5);
        let jobs_final = get_pending_jobs_for_method(
            &jobs,
            &b"dev1".to_string(),
            &b"agent1".to_string(),
            &b"method1".to_string(),
        );
        assert!(vector::is_empty(&jobs_final), 4);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

// ============= Periodic Task Tests =============

#[test]
fun test_create_periodic_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time + interval;
        
        let job_sequence = create_job(
            &mut jobs,
            option::some(b"Periodic Task".to_string()),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"periodic_data",
            option::some(interval),  // interval_ms
            option::some(next_scheduled),  // next_scheduled_at
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Verify job was created with periodic fields
        assert!(job_exists(&jobs, job_sequence), 0);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 1);
        assert!(get_pending_jobs_count(&jobs) == 1, 2);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::EIntervalTooShort)]
fun test_periodic_job_interval_too_short() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 30_000u64; // 30 seconds - too short!
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time + interval;
        
        // This should fail - interval must be >= 60000ms
        let _job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),  // interval_ms - too short!
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = jobs::ENotDueYet)]
fun test_periodic_job_cannot_start_before_due() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time + interval; // Due in 1 minute
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Try to start immediately - should fail because not due yet
        start_job(&mut jobs, job_sequence, &clock);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_periodic_job_can_start_when_due() {
    let (mut scenario, mut clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time + interval;
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Advance clock to due time
        clock::increment_for_testing(&mut clock, interval);
        
        // Now should be able to start
        start_job(&mut jobs, job_sequence, &clock);
        assert!(is_job_running(get_job(&jobs, job_sequence)), 0);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_periodic_job_reschedule_on_complete() {
    let (mut scenario, mut clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time; // Can start immediately
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Start and complete the job
        start_job(&mut jobs, job_sequence, &clock);
        assert!(is_job_running(get_job(&jobs, job_sequence)), 0);
        
        complete_job(&mut jobs, job_sequence, &clock);
        
        // Job should still exist and be pending (rescheduled)
        assert!(job_exists(&jobs, job_sequence), 1);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 2);
        assert!(get_pending_jobs_count(&jobs) == 1, 3);
        
        // Job attempts should be reset to 0
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 0, 4);
        
        // Cannot start immediately - must wait for next interval
        clock::increment_for_testing(&mut clock, interval - 1); // Not quite there
        // This would fail with ENotDueYet if we tried to start
        
        // Advance to next interval
        clock::increment_for_testing(&mut clock, 1); // Now at next interval
        
        // Can start again
        start_job(&mut jobs, job_sequence, &clock);
        assert!(is_job_running(get_job(&jobs, job_sequence)), 5);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_periodic_job_reschedule_after_max_failures() {
    let (mut scenario, mut clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(2), ts::ctx(&mut scenario)); // Max 2 attempts
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time; // Can start immediately
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // First attempt - fail
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 1, 0);
        fail_job(&mut jobs, job_sequence, b"Error 1".to_string(), &clock);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 1); // Should retry
        
        // Wait for retry interval before second attempt
        clock::increment_for_testing(&mut clock, 60_000); // RETRY_INTERVAL_MS
        
        // Second attempt - fail again (max attempts reached)
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 2, 2);
        fail_job(&mut jobs, job_sequence, b"Error 2".to_string(), &clock);
        
        // Job should still exist but be rescheduled for next interval
        assert!(job_exists(&jobs, job_sequence), 3);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 4);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 0, 5); // Reset attempts
        
        // Cannot start immediately - must wait for next interval
        // Would fail with ENotDueYet if we tried
        
        // Advance to next interval
        clock::increment_for_testing(&mut clock, interval);
        
        // Can start again in new interval
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 1, 6); // Fresh attempt count
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_periodic_job_retry_within_interval() {
    let (mut scenario, mut clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(3), ts::ctx(&mut scenario)); // Max 3 attempts
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time; // Can start immediately
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // First attempt - fail
        start_job(&mut jobs, job_sequence, &clock);
        fail_job(&mut jobs, job_sequence, b"Error 1".to_string(), &clock);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 0);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 1, 1);
        
        // Wait for retry interval before retry
        clock::increment_for_testing(&mut clock, 60_000); // RETRY_INTERVAL_MS
        
        // Can retry after waiting for retry interval
        start_job(&mut jobs, job_sequence, &clock);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 2, 2);
        
        // Complete successfully on second attempt
        complete_job(&mut jobs, job_sequence, &clock);
        
        // Job should be rescheduled for next interval with attempts reset
        assert!(job_exists(&jobs, job_sequence), 3);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 4);
        assert!(job_attempts(get_job(&jobs, job_sequence)) == 0, 5);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_terminate_periodic_job() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        let interval = 60_000u64; // 1 minute
        let start_time = clock::timestamp_ms(&clock);
        let next_scheduled = start_time;
        
        let job_sequence = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data",
            option::some(interval),
            option::some(next_scheduled),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Start the job
        start_job(&mut jobs, job_sequence, &clock);
        assert!(is_job_running(get_job(&jobs, job_sequence)), 0);
        
        // Complete it once
        complete_job(&mut jobs, job_sequence, &clock);
        assert!(is_job_pending(get_job(&jobs, job_sequence)), 1);
        
        // Terminate the periodic job
        terminate_job(&mut jobs, job_sequence, &clock);
        
        // Job should be completely removed
        assert!(!job_exists(&jobs, job_sequence), 2);
        assert!(get_pending_jobs_count(&jobs) == 0, 3);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_mixed_periodic_and_onetime_jobs() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::none(), ts::ctx(&mut scenario));
        
        // Create one-time job
        let onetime_job = create_job(
            &mut jobs,
            option::some(b"One-time Job".to_string()),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"onetime_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"onetime_data",
            option::none(),  // No interval - one-time job
            option::none(),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Create periodic job
        let interval = 60_000u64;
        let periodic_job = create_job(
            &mut jobs,
            option::some(b"Periodic Job".to_string()),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"periodic_method".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method2".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"periodic_data",
            option::some(interval),
            option::some(clock::timestamp_ms(&clock)),
            false, // is_settlement_job
            &clock,
            ts::ctx(&mut scenario),
        );
        
        assert!(get_pending_jobs_count(&jobs) == 2, 0);
        
        // Start and complete one-time job
        start_job(&mut jobs, onetime_job, &clock);
        complete_job(&mut jobs, onetime_job, &clock);
        
        // One-time job should be deleted
        assert!(!job_exists(&jobs, onetime_job), 1);
        assert!(get_pending_jobs_count(&jobs) == 1, 2);
        
        // Start and complete periodic job
        start_job(&mut jobs, periodic_job, &clock);
        complete_job(&mut jobs, periodic_job, &clock);
        
        // Periodic job should still exist (rescheduled)
        assert!(job_exists(&jobs, periodic_job), 3);
        assert!(is_job_pending(get_job(&jobs, periodic_job)), 4);
        assert!(get_pending_jobs_count(&jobs) == 1, 5);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}
#[test]
fun test_restart_failed_jobs_with_vector() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
        
        // Create multiple jobs
        let job1 = create_job(
            &mut jobs,
            option::none(),
            b"dev".to_string(),
            b"agent".to_string(),
            b"method".to_string(),
            b"app".to_string(),
            b"instance".to_string(),
            b"app_method".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data1",
            option::none(),
            option::none(),
            false,
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job2 = create_job(
            &mut jobs,
            option::none(),
            b"dev".to_string(),
            b"agent".to_string(),
            b"method".to_string(),
            b"app".to_string(),
            b"instance".to_string(),
            b"app_method".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data2",
            option::none(),
            option::none(),
            false,
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job3 = create_job(
            &mut jobs,
            option::none(),
            b"dev".to_string(),
            b"agent".to_string(),
            b"method".to_string(),
            b"app".to_string(),
            b"instance".to_string(),
            b"app_method".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data3",
            option::none(),
            option::none(),
            false,
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Fail all three jobs
        start_job(&mut jobs, job1, &clock);
        fail_job(&mut jobs, job1, b"Error".to_string(), &clock);
        
        start_job(&mut jobs, job2, &clock);
        fail_job(&mut jobs, job2, b"Error".to_string(), &clock);
        
        start_job(&mut jobs, job3, &clock);
        fail_job(&mut jobs, job3, b"Error".to_string(), &clock);
        
        // All should be failed
        assert!(get_failed_jobs_count(&jobs) == 3, 0);
        
        // Restart only job1 and job3
        let mut jobs_to_restart = vector::empty<u64>();
        vector::push_back(&mut jobs_to_restart, job1);
        vector::push_back(&mut jobs_to_restart, job3);
        restart_failed_jobs(&mut jobs, option::some(jobs_to_restart), &clock);
        
        // job1 and job3 should be pending, job2 still failed
        assert!(is_job_pending(get_job(&jobs, job1)), 1);
        assert!(is_job_failed(get_job(&jobs, job2)), 2);
        assert!(is_job_pending(get_job(&jobs, job3)), 3);
        assert!(get_failed_jobs_count(&jobs) == 1, 4);
        assert!(get_pending_jobs_count(&jobs) == 2, 5);
        
        // Restart all remaining failed jobs (just job2)
        restart_failed_jobs(&mut jobs, option::none(), &clock);
        assert!(is_job_pending(get_job(&jobs, job2)), 6);
        assert!(get_failed_jobs_count(&jobs) == 0, 7);
        assert!(get_pending_jobs_count(&jobs) == 3, 8);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}

#[test]
fun test_remove_failed_jobs() {
    let (mut scenario, clock) = setup_test();
    
    ts::next_tx(&mut scenario, TEST_ADDR);
    {
        let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
        
        // Create multiple jobs
        let job1 = create_job(
            &mut jobs,
            option::none(),
            b"dev".to_string(),
            b"agent".to_string(),
            b"method".to_string(),
            b"app".to_string(),
            b"instance".to_string(),
            b"app_method".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data1",
            option::none(),
            option::none(),
            false,
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job2 = create_job(
            &mut jobs,
            option::none(),
            b"dev".to_string(),
            b"agent".to_string(),
            b"method".to_string(),
            b"app".to_string(),
            b"instance".to_string(),
            b"app_method".to_string(),
            option::none(),
            option::none(),
            option::none(),
            option::none(),
            b"data2",
            option::none(),
            option::none(),
            false,
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Fail both jobs
        start_job(&mut jobs, job1, &clock);
        fail_job(&mut jobs, job1, b"Error".to_string(), &clock);
        
        start_job(&mut jobs, job2, &clock);
        fail_job(&mut jobs, job2, b"Error".to_string(), &clock);
        
        assert!(get_failed_jobs_count(&jobs) == 2, 0);
        
        // Remove only job1
        let mut jobs_to_remove = vector::empty<u64>();
        vector::push_back(&mut jobs_to_remove, job1);
        remove_failed_jobs(&mut jobs, option::some(jobs_to_remove), &clock);
        
        // job1 should be deleted, job2 still failed
        assert!(!job_exists(&jobs, job1), 1);
        assert!(job_exists(&jobs, job2), 2);
        assert!(is_job_failed(get_job(&jobs, job2)), 3);
        assert!(get_failed_jobs_count(&jobs) == 1, 4);
        
        // Remove all remaining failed jobs
        remove_failed_jobs(&mut jobs, option::none(), &clock);
        assert!(!job_exists(&jobs, job2), 5);
        assert!(get_failed_jobs_count(&jobs) == 0, 6);
        
        transfer::public_share_object(jobs);
    };
    
    clock::destroy_for_testing(clock);
    ts::end(scenario);
}
