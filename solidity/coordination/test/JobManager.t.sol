// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/JobManager.sol";
import "../src/libraries/DataTypes.sol";

contract JobManagerTest is Test {
    JobManager public jobManager;

    address admin = address(0x1);
    address coordinator = address(0x2);
    address operator = address(0x3);
    address user = address(0x4);

    string constant APP_INSTANCE = "test-app-instance";
    string constant DEVELOPER = "test-developer";
    string constant AGENT = "test-agent";
    string constant AGENT_METHOD = "test-method";
    string constant APP = "test-app";
    string constant APP_INSTANCE_METHOD = "test-instance-method";

    function setUp() public {
        // Deploy JobManager
        vm.prank(admin);
        jobManager = new JobManager();

        // Grant roles
        vm.startPrank(admin);
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), coordinator);
        jobManager.grantRole(jobManager.OPERATOR_ROLE(), operator);
        vm.stopPrank();
    }

    function testCreateJob() public {
        // Prepare job input
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Test job",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        // Create job
        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Verify job was created
        assertEq(jobId, 0);

        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(job.id, jobId);
        assertEq(job.developer, DEVELOPER);
        assertEq(job.agent, AGENT);
        assertEq(job.agentMethod, AGENT_METHOD);
        assertEq(job.data, hex"1234");
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));
        assertEq(job.attempts, 0);
    }

    function testCreatePeriodicJob() public {
        // Prepare periodic job input
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Periodic job",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"5678",
            intervalMs: 60000 // 1 minute
        });

        // Create job
        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Verify job was created with interval
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(job.intervalMs, 60000);
        assertGt(job.nextScheduledAt, 0);
    }

    function testTakeJob() public {
        // Create a job first
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Job to take",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"abcd",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Take the job as coordinator
        vm.prank(coordinator);
        jobManager.takeJob(APP_INSTANCE, jobId);

        // Verify job status changed
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Running));
        assertEq(job.takenBy, coordinator);
        assertEq(job.attempts, 1);
    }

    function testCompleteJob() public {
        // Create and take a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Job to complete",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        vm.prank(coordinator);
        jobManager.takeJob(APP_INSTANCE, jobId);

        // Complete the job
        bytes memory output = hex"726573756c74";
        vm.prank(coordinator);
        jobManager.completeJob(APP_INSTANCE, jobId, output);

        // Verify job was completed
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Completed));
        assertEq(job.output, output);
    }

    function testJobFailRetry() public {
        // Create and take a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Job to fail",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        vm.prank(coordinator);
        jobManager.takeJob(APP_INSTANCE, jobId);

        // Fail the job
        vm.prank(coordinator);
        jobManager.failJob(APP_INSTANCE, jobId, "Test error");

        // Verify job went back to pending (not max attempts yet)
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));
        assertEq(job.attempts, 1);
        assertEq(job.takenBy, address(0));
    }

    function testJobFailsAfterMaxAttempts() public {
        // Create a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Job to fail permanently",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Take and fail the job 3 times (default max attempts)
        for (uint8 i = 0; i < 3; i++) {
            vm.prank(coordinator);
            jobManager.takeJob(APP_INSTANCE, jobId);

            vm.prank(coordinator);
            jobManager.failJob(APP_INSTANCE, jobId, "Test error");
        }

        // Verify job is now failed
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Failed));
        assertEq(job.attempts, 3);
    }

    function testTerminateJob() public {
        // Create a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Job to terminate",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Terminate the job as admin
        vm.prank(admin);
        jobManager.terminateJob(APP_INSTANCE, jobId);

        // Verify job was terminated
        DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Failed));
    }

    function testGetPendingJobs() public {
        // Create multiple jobs
        for (uint256 i = 0; i < 5; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Job ", vm.toString(i))),
                developer: DEVELOPER,
                agent: AGENT,
                agentMethod: AGENT_METHOD,
                app: APP,
                appInstance: APP_INSTANCE,
                appInstanceMethod: APP_INSTANCE_METHOD,
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            jobManager.createJob(input);
        }

        // Get pending jobs
        DataTypes.Job[] memory pendingJobs = jobManager.getPendingJobs(APP_INSTANCE, 10, 0);

        // Verify we got all 5 jobs
        assertEq(pendingJobs.length, 5);
        for (uint256 i = 0; i < 5; i++) {
            assertEq(uint8(pendingJobs[i].status), uint8(DataTypes.JobStatus.Pending));
        }
    }

    function testRestartFailedJobs() public {
        // Create and fail multiple jobs
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Job ", vm.toString(i))),
                developer: DEVELOPER,
                agent: AGENT,
                agentMethod: AGENT_METHOD,
                app: APP,
                appInstance: APP_INSTANCE,
                appInstanceMethod: APP_INSTANCE_METHOD,
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            uint256 jobId = jobManager.createJob(input);

            // Take and fail 3 times to mark as failed
            for (uint8 j = 0; j < 3; j++) {
                vm.prank(coordinator);
                jobManager.takeJob(APP_INSTANCE, jobId);

                vm.prank(coordinator);
                jobManager.failJob(APP_INSTANCE, jobId, "Error");
            }
        }

        // Verify failed job count
        assertEq(jobManager.getFailedJobCount(APP_INSTANCE), 3);

        // Restart failed jobs
        vm.prank(operator);
        uint256 restarted = jobManager.restartFailedJobs(APP_INSTANCE);

        // Verify jobs were restarted
        assertEq(restarted, 3);
        assertEq(jobManager.getFailedJobCount(APP_INSTANCE), 0);
        assertEq(jobManager.getPendingJobCount(APP_INSTANCE), 3);
    }

    function testAccessControl() public {
        // Try to take job without coordinator role
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Test job",
            developer: DEVELOPER,
            agent: AGENT,
            agentMethod: AGENT_METHOD,
            app: APP,
            appInstance: APP_INSTANCE,
            appInstanceMethod: APP_INSTANCE_METHOD,
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Should fail - user doesn't have coordinator role
        vm.prank(user);
        vm.expectRevert("JobManager: caller is not coordinator");
        jobManager.takeJob(APP_INSTANCE, jobId);

        // Should succeed with coordinator role
        vm.prank(coordinator);
        jobManager.takeJob(APP_INSTANCE, jobId);
    }

    function testMulticallJobs() public {
        // Create multiple jobs
        uint256[] memory jobIds = new uint256[](3);
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Job ", vm.toString(i))),
                developer: DEVELOPER,
                agent: AGENT,
                agentMethod: AGENT_METHOD,
                app: APP,
                appInstance: APP_INSTANCE,
                appInstanceMethod: APP_INSTANCE_METHOD,
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            jobIds[i] = jobManager.createJob(input);
        }

        // Take all jobs
        uint256[] memory takeJobs = new uint256[](3);
        for (uint256 i = 0; i < 3; i++) {
            takeJobs[i] = jobIds[i];
        }

        // Execute multicall to take all jobs
        vm.prank(coordinator);
        jobManager.multicallJobs(
            new uint256[](0), // completeJobs
            new bytes[](0), // completeOutputs
            new uint256[](0), // failJobs
            new string[](0), // failErrors
            new uint256[](0), // terminateJobs
            takeJobs, // takeJobs
            APP_INSTANCE
        );

        // Verify all jobs were taken
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.Job memory job = jobManager.getJob(APP_INSTANCE, jobIds[i]);
            assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Running));
            assertEq(job.takenBy, coordinator);
        }
    }
}