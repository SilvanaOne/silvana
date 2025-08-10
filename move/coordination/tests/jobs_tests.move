#[test_only]
module coordination::jobs_tests;

use coordination::jobs::{
    Self,
    create_jobs,
    create_job,
    start_job,
    complete_job,
    fail_job,
    get_job,
    job_exists,
    get_pending_jobs,
    get_pending_jobs_count,
    get_next_pending_job,
    update_max_attempts,
    job_id,
    job_attempts,
    max_attempts,
    is_job_pending,
    is_job_running,
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
        
        let job_id = create_job(
            &mut jobs,
            option::some(b"Test job".to_string()),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::some(vector[1, 2, 3]),
            b"test_data",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        assert!(job_id == 1, 0);
        assert!(job_exists(&jobs, job_id), 1);
        assert!(get_pending_jobs_count(&jobs) == 1, 2);
        
        let job = get_job(&jobs, job_id);
        assert!(job_id(job) == 1, 3);
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
        
        let job_id = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            b"test_data",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // Start the job
        start_job(&mut jobs, job_id, &clock);
        
        let job = get_job(&jobs, job_id);
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
        
        let job_id = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            b"test_data",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        start_job(&mut jobs, job_id, &clock);
        // This should fail - trying to start an already running job
        start_job(&mut jobs, job_id, &clock);
        
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
        
        let job_id = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            b"test_data",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        start_job(&mut jobs, job_id, &clock);
        complete_job(&mut jobs, job_id, &clock);
        
        // Job should be deleted
        assert!(!job_exists(&jobs, job_id), 0);
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
        
        let job_id = create_job(
            &mut jobs,
            option::none(),
            b"developer1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            b"test_data",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        // First attempt
        start_job(&mut jobs, job_id, &clock);
        assert!(job_attempts(get_job(&jobs, job_id)) == 1, 0);
        
        // Fail the job - should go back to pending
        fail_job(&mut jobs, job_id, b"Error 1".to_string(), &clock);
        assert!(job_exists(&jobs, job_id), 1);
        assert!(is_job_pending(get_job(&jobs, job_id)), 2);
        assert!(get_pending_jobs_count(&jobs) == 1, 3);
        
        // Second attempt
        start_job(&mut jobs, job_id, &clock);
        assert!(job_attempts(get_job(&jobs, job_id)) == 2, 4);
        fail_job(&mut jobs, job_id, b"Error 2".to_string(), &clock);
        assert!(job_exists(&jobs, job_id), 5);
        
        // Third attempt
        start_job(&mut jobs, job_id, &clock);
        assert!(job_attempts(get_job(&jobs, job_id)) == 3, 6);
        
        // Fail after max attempts - job should be deleted
        fail_job(&mut jobs, job_id, b"Error 3".to_string(), &clock);
        assert!(!job_exists(&jobs, job_id), 7);
        assert!(get_pending_jobs_count(&jobs) == 0, 8);
        
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
        let _job_id1 = create_job(
            &mut jobs,
            option::some(b"Job 1".to_string()),
            b"dev1".to_string(),
            b"agent1".to_string(),
            b"method1".to_string(),
            b"app1".to_string(),
            b"instance1".to_string(),
            b"app_method1".to_string(),
            option::none(),
            b"data1",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let job_id2 = create_job(
            &mut jobs,
            option::some(b"Job 2".to_string()),
            b"dev2".to_string(),
            b"agent2".to_string(),
            b"method2".to_string(),
            b"app2".to_string(),
            b"instance2".to_string(),
            b"app_method2".to_string(),
            option::none(),
            b"data2",
            &clock,
            ts::ctx(&mut scenario),
        );
        
        let _job_id3 = create_job(
            &mut jobs,
            option::some(b"Job 3".to_string()),
            b"dev3".to_string(),
            b"agent3".to_string(),
            b"method3".to_string(),
            b"app3".to_string(),
            b"instance3".to_string(),
            b"app_method3".to_string(),
            option::none(),
            b"data3",
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
        start_job(&mut jobs, job_id2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 2, 3);
        
        // Complete one job
        complete_job(&mut jobs, job_id2, &clock);
        assert!(get_pending_jobs_count(&jobs) == 2, 4);
        assert!(!job_exists(&jobs, job_id2), 5);
        
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