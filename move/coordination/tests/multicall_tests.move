#[test_only]
module coordination::multicall_tests {
    use coordination::jobs::{
        create_jobs, create_job, start_job,
        multicall_job_operations, get_job, is_job_pending, 
        is_job_running, is_job_failed,
        get_pending_jobs_count, job_exists
    };
    use std::string;
    use sui::test_scenario as ts;
    use sui::clock::{Self, Clock};

    const TEST_ADDR: address = @0x42;

    fun setup_test(): (ts::Scenario, Clock) {
        let mut scenario = ts::begin(TEST_ADDR);
        let mut clock = clock::create_for_testing(ts::ctx(&mut scenario));
        clock::set_for_testing(&mut clock, 1000);
        (scenario, clock)
    }

    #[test]
    fun test_multicall_basic_operations() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Create multiple jobs for testing
            let (success1, job1) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method1".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method1".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data1",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success2, job2) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method2".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method2".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data2",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success3, job3) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method3".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method3".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data3",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success4, job4) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method4".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method4".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data4",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            assert!(success1, 100);
            assert!(success2, 101);
            assert!(success3, 102);
            assert!(success4, 103);
            
            // Start jobs to make them active
            assert!(start_job(&mut jobs, job1, &clock, ts::ctx(&mut scenario)), 1);
            assert!(start_job(&mut jobs, job2, &clock, ts::ctx(&mut scenario)), 2);
            assert!(start_job(&mut jobs, job3, &clock, ts::ctx(&mut scenario)), 3);
            
            // Prepare multicall arguments
            let mut complete_jobs = vector::empty<u64>();
            vector::push_back(&mut complete_jobs, job1);
            
            let mut fail_jobs = vector::empty<u64>();
            vector::push_back(&mut fail_jobs, job2);
            
            let mut fail_errors = vector::empty<string::String>();
            vector::push_back(&mut fail_errors, b"Test error".to_string());
            
            let mut terminate_jobs = vector::empty<u64>();
            vector::push_back(&mut terminate_jobs, job3);
            
            let mut start_jobs = vector::empty<u64>();
            vector::push_back(&mut start_jobs, job4);
            
            let mut start_memory_requirements = vector::empty<u64>();
            vector::push_back(&mut start_memory_requirements, 100); // 100 units of memory for job4
            
            let available_memory = 1000u64; // Plenty of memory available
            
            // Execute multicall
            multicall_job_operations(
                &mut jobs,
                complete_jobs,
                fail_jobs,
                fail_errors,
                terminate_jobs,
                start_jobs,
                start_memory_requirements,
                available_memory,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // Verify results
            // Job1 completed - completed jobs are removed from the jobs table
            assert!(!job_exists(&jobs, job1), 10);
            assert!(is_job_failed(get_job(&jobs, job2)), 11);
            assert!(!job_exists(&jobs, job3), 12); // terminated jobs are removed
            assert!(is_job_running(get_job(&jobs, job4)), 13);
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }

    #[test]
    fun test_multicall_memory_management() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Create multiple jobs
            let (success1, job1) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method1".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method1".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data1",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success2, job2) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method2".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method2".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data2",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success3, job3) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method3".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method3".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data3",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            assert!(success1, 300);
            assert!(success2, 301);
            assert!(success3, 302);
            
            // Prepare start jobs with different memory requirements
            let mut start_jobs = vector::empty<u64>();
            vector::push_back(&mut start_jobs, job1);
            vector::push_back(&mut start_jobs, job2);
            vector::push_back(&mut start_jobs, job3);
            
            let mut start_memory_requirements = vector::empty<u64>();
            vector::push_back(&mut start_memory_requirements, 300); // job1 needs 300
            vector::push_back(&mut start_memory_requirements, 400); // job2 needs 400
            vector::push_back(&mut start_memory_requirements, 500); // job3 needs 500
            
            let available_memory = 750u64; // Only enough for job1 and job2
            
            // Execute multicall with limited memory
            multicall_job_operations(
                &mut jobs,
                vector::empty<u64>(), // no complete
                vector::empty<u64>(), // no fail
                vector::empty<string::String>(), // no fail errors
                vector::empty<u64>(), // no terminate
                start_jobs,
                start_memory_requirements,
                available_memory,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // Verify that only job1 and job2 were started (memory limit)
            assert!(is_job_running(get_job(&jobs, job1)), 1);
            assert!(is_job_running(get_job(&jobs, job2)), 2);
            assert!(is_job_pending(get_job(&jobs, job3)), 3); // Not enough memory for job3
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }

    #[test]
    fun test_multicall_empty_operations() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Execute multicall with all empty vectors
            multicall_job_operations(
                &mut jobs,
                vector::empty<u64>(),
                vector::empty<u64>(),
                vector::empty<string::String>(),
                vector::empty<u64>(),
                vector::empty<u64>(),
                vector::empty<u64>(),
                1000u64,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // Should complete without errors
            assert!(get_pending_jobs_count(&jobs) == 0, 1);
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }

    #[test]
    fun test_multicall_mixed_operations() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Create a mix of jobs in different states
            let (success1, job1) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method1".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method1".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data1",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success2, job2) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method2".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method2".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data2",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            
            let (success3, job3) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method3".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method3".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data3",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            assert!(success1, 200);
            assert!(success2, 201);
            assert!(success3, 202);
            
            // Start job1 and job2
            assert!(start_job(&mut jobs, job1, &clock, ts::ctx(&mut scenario)), 1);
            assert!(start_job(&mut jobs, job2, &clock, ts::ctx(&mut scenario)), 2);
            
            // Now complete job1 and fail job2 in the same multicall
            let mut complete_jobs = vector::empty<u64>();
            vector::push_back(&mut complete_jobs, job1);
            
            let mut fail_jobs = vector::empty<u64>();
            vector::push_back(&mut fail_jobs, job2);
            
            let mut fail_errors = vector::empty<string::String>();
            vector::push_back(&mut fail_errors, b"Mixed operation error".to_string());
            
            let mut start_jobs = vector::empty<u64>();
            vector::push_back(&mut start_jobs, job3);
            
            let mut start_memory_requirements = vector::empty<u64>();
            vector::push_back(&mut start_memory_requirements, 50);
            
            // Execute multicall
            multicall_job_operations(
                &mut jobs,
                complete_jobs,
                fail_jobs,
                fail_errors,
                vector::empty<u64>(), // no terminate
                start_jobs,
                start_memory_requirements,
                1000u64,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // Verify all operations succeeded
            // Job1 completed - completed jobs are removed from the jobs table
            assert!(!job_exists(&jobs, job1), 10);
            assert!(is_job_failed(get_job(&jobs, job2)), 11);
            assert!(is_job_running(get_job(&jobs, job3)), 12);
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }

    #[test]
    fun test_multicall_insufficient_memory() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Create a job
            let (success1, job1) = create_job(
                &mut jobs,
                option::none(),
                b"dev".to_string(),
                b"agent".to_string(),
                b"method1".to_string(),
                b"app".to_string(),
                b"instance".to_string(),
                b"app_method1".to_string(),
                option::none(),
                option::none(),
                option::none(),
                option::none(),
                b"data1",
                option::none(),
                option::none(),
                &clock,
                ts::ctx(&mut scenario),
            );
            assert!(success1, 400);
            
            // Try to start job with more memory than available
            let mut start_jobs = vector::empty<u64>();
            vector::push_back(&mut start_jobs, job1);
            
            let mut start_memory_requirements = vector::empty<u64>();
            vector::push_back(&mut start_memory_requirements, 1000);
            
            let available_memory = 500u64; // Not enough memory
            
            // Execute multicall
            multicall_job_operations(
                &mut jobs,
                vector::empty<u64>(),
                vector::empty<u64>(),
                vector::empty<string::String>(),
                vector::empty<u64>(),
                start_jobs,
                start_memory_requirements,
                available_memory,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // Job should remain pending (not started due to insufficient memory)
            assert!(is_job_pending(get_job(&jobs, job1)), 1);
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }

    #[test]
    fun test_multicall_partial_memory_allocation() {
        let (mut scenario, clock) = setup_test();
        
        ts::next_tx(&mut scenario, TEST_ADDR);
        {
            let mut jobs = create_jobs(option::some(1), ts::ctx(&mut scenario));
            
            // Create 5 jobs
            let mut all_jobs = vector::empty<u64>();
            let mut i = 0;
            while (i < 5) {
                let (success, job) = create_job(
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
                    b"data",
                    option::none(),
                    option::none(),
                    &clock,
                    ts::ctx(&mut scenario),
                );
                assert!(success, 500 + i);
                vector::push_back(&mut all_jobs, job);
                i = i + 1;
            };
            
            // Try to start all 5 jobs with varying memory requirements
            let mut start_memory_requirements = vector::empty<u64>();
            vector::push_back(&mut start_memory_requirements, 100); // job 0
            vector::push_back(&mut start_memory_requirements, 200); // job 1
            vector::push_back(&mut start_memory_requirements, 150); // job 2
            vector::push_back(&mut start_memory_requirements, 300); // job 3
            vector::push_back(&mut start_memory_requirements, 250); // job 4
            
            let available_memory = 500u64; // Only enough for first 3 jobs (100 + 200 + 150 = 450)
            
            // Execute multicall
            multicall_job_operations(
                &mut jobs,
                vector::empty<u64>(),
                vector::empty<u64>(),
                vector::empty<string::String>(),
                vector::empty<u64>(),
                all_jobs,
                start_memory_requirements,
                available_memory,
                &clock,
                ts::ctx(&mut scenario),
            );
            
            // First 3 jobs should be started, last 2 should remain pending
            assert!(is_job_running(get_job(&jobs, *vector::borrow(&all_jobs, 0))), 1);
            assert!(is_job_running(get_job(&jobs, *vector::borrow(&all_jobs, 1))), 2);
            assert!(is_job_running(get_job(&jobs, *vector::borrow(&all_jobs, 2))), 3);
            assert!(is_job_pending(get_job(&jobs, *vector::borrow(&all_jobs, 3))), 4);
            assert!(is_job_pending(get_job(&jobs, *vector::borrow(&all_jobs, 4))), 5);
            
            // 2 jobs should remain pending
            assert!(get_pending_jobs_count(&jobs) == 2, 6);
            
            transfer::public_share_object(jobs);
        };
        
        clock::destroy_for_testing(clock);
        ts::end(scenario);
    }
}