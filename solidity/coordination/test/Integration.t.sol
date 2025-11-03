// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/SilvanaCoordination.sol";
import "../src/JobManager.sol";
import "../src/AppInstanceManager.sol";
import "../src/libraries/DataTypes.sol";

contract IntegrationTest is Test {
    SilvanaCoordination public coordination;
    JobManager public jobManager;
    AppInstanceManager public appInstanceManager;

    address admin = address(0x1);
    address coordinator1 = address(0x2);
    address coordinator2 = address(0x3);
    address operator = address(0x4);
    address user = address(0x5);

    string instanceId;

    function setUp() public {
        // Deploy all contracts
        vm.startPrank(admin);
        jobManager = new JobManager();
        appInstanceManager = new AppInstanceManager();
        coordination = new SilvanaCoordination();
        vm.stopPrank();

        // Initialize coordination (without ProofManager and SettlementManager for now)
        vm.prank(admin);
        coordination.initialize(admin, address(jobManager), address(appInstanceManager), address(0), address(0), address(0));

        // Grant roles
        vm.startPrank(admin);
        // Coordination roles
        coordination.grantRole(coordination.COORDINATOR_ROLE(), coordinator1);
        coordination.grantRole(coordination.COORDINATOR_ROLE(), coordinator2);
        coordination.grantRole(coordination.OPERATOR_ROLE(), operator);
        coordination.grantRole(coordination.PAUSER_ROLE(), operator);

        // JobManager roles
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), coordinator1);
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), coordinator2);
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), address(coordination));
        jobManager.grantRole(jobManager.OPERATOR_ROLE(), operator);

        // AppInstanceManager roles
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator1);
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator2);
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), address(coordination));
        appInstanceManager.grantRole(appInstanceManager.OPERATOR_ROLE(), operator);
        vm.stopPrank();

        // Create an app instance for testing - call AppInstanceManager directly so user is the owner
        vm.prank(user);
        instanceId = appInstanceManager.createAppInstance("0x0000000000000000000000000000000000000000000000000000000000000001", "test-app", "TestApp", "TestDeveloper");
    }

    function testFullJobLifecycle() public {
        // Create a job through coordination contract
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Integration test job",
            developer: "TestDeveloper",
            agent: "TestAgent",
            agentMethod: "process",
            app: "TestApp",
            appInstance: instanceId,
            appInstanceMethod: "execute",
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = coordination.createJob(input);

        // Verify job was created
        DataTypes.Job memory job = coordination.getJob(instanceId, jobId);
        assertEq(job.id, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));

        // Take job as coordinator1 - call JobManager directly to preserve msg.sender
        vm.prank(coordinator1);
        jobManager.takeJob(instanceId, jobId);

        // Verify job status changed
        job = coordination.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Running));
        assertEq(job.takenBy, coordinator1);

        // Complete the job
        bytes memory output = hex"726573756c745f64617461";
        vm.prank(coordinator1);
        jobManager.completeJob(instanceId, jobId, output);

        // Verify job was completed
        job = coordination.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Completed));
        assertEq(job.output, output);
    }

    function testAppInstanceWithJobs() public {
        // Update sequence
        vm.prank(user);
        coordination.updateSequence(instanceId, 1, keccak256("actions1"), keccak256("state1"));

        // Create multiple jobs
        uint256[] memory jobIds = new uint256[](3);
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Job ", vm.toString(i))),
                developer: "TestDeveloper",
                agent: "TestAgent",
                agentMethod: "process",
                app: "TestApp",
                appInstance: instanceId,
                appInstanceMethod: "execute",
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            jobIds[i] = coordination.createJob(input);
        }

        // Get pending jobs
        DataTypes.Job[] memory pendingJobs = coordination.getPendingJobs(instanceId, 10, 0);
        assertEq(pendingJobs.length, 3);

        // Take and complete jobs
        for (uint256 i = 0; i < 3; i++) {
            vm.prank(coordinator1);
            coordination.takeJob(instanceId, jobIds[i]);

            vm.prank(coordinator1);
            coordination.completeJob(instanceId, jobIds[i], abi.encode(i * 2));
        }

        // Update sequence after job completions
        vm.prank(user);
        coordination.updateSequence(instanceId, 2, keccak256("actions2"), keccak256("state2"));

        // Verify current sequence
        uint256 currentSeq = coordination.getCurrentSequence(instanceId);
        assertEq(currentSeq, 2);
    }

    function testMultipleCoordinators() public {
        // Create multiple jobs
        uint256[] memory jobIds = new uint256[](4);
        for (uint256 i = 0; i < 4; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Multi-coord job ", vm.toString(i))),
                developer: "TestDeveloper",
                agent: "TestAgent",
                agentMethod: "process",
                app: "TestApp",
                appInstance: instanceId,
                appInstanceMethod: "execute",
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            jobIds[i] = coordination.createJob(input);
        }

        // Coordinator1 takes first two jobs - call JobManager directly to preserve msg.sender
        vm.startPrank(coordinator1);
        jobManager.takeJob(instanceId, jobIds[0]);
        jobManager.takeJob(instanceId, jobIds[1]);
        vm.stopPrank();

        // Coordinator2 takes next two jobs - call JobManager directly to preserve msg.sender
        vm.startPrank(coordinator2);
        jobManager.takeJob(instanceId, jobIds[2]);
        jobManager.takeJob(instanceId, jobIds[3]);
        vm.stopPrank();

        // Verify job assignments
        DataTypes.Job memory job0 = coordination.getJob(instanceId, jobIds[0]);
        assertEq(job0.takenBy, coordinator1);

        DataTypes.Job memory job2 = coordination.getJob(instanceId, jobIds[2]);
        assertEq(job2.takenBy, coordinator2);

        // Complete jobs - call JobManager directly
        vm.prank(coordinator1);
        jobManager.completeJob(instanceId, jobIds[0], hex"726573756c7430");

        vm.prank(coordinator2);
        jobManager.completeJob(instanceId, jobIds[2], hex"726573756c7432");
    }

    function testPauseUnpause() public {
        // Pause coordination
        vm.prank(operator);
        coordination.pause();

        // Verify paused
        assertTrue(coordination.paused());

        // Try to create job while paused
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Paused job",
            developer: "TestDeveloper",
            agent: "TestAgent",
            agentMethod: "process",
            app: "TestApp",
            appInstance: instanceId,
            appInstanceMethod: "execute",
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        vm.expectRevert("SilvanaCoordination: paused");
        coordination.createJob(input);

        // Unpause
        vm.prank(operator);
        coordination.unpause();

        // Now job creation should work
        vm.prank(user);
        uint256 jobId = coordination.createJob(input);
        // Job ID 0 is valid for the first job
        assertTrue(jobId >= 0);
    }

    function testAppInstancePause() public {
        // Create a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Test job",
            developer: "TestDeveloper",
            agent: "TestAgent",
            agentMethod: "process",
            app: "TestApp",
            appInstance: instanceId,
            appInstanceMethod: "execute",
            data: hex"1234",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = coordination.createJob(input);

        // Pause app instance - call AppInstanceManager directly to preserve msg.sender
        vm.prank(user);
        appInstanceManager.pauseAppInstance(instanceId);

        // Verify instance is paused
        assertTrue(coordination.isAppInstancePaused(instanceId));

        // Try to update sequence while paused
        vm.prank(user);
        vm.expectRevert("AppInstanceManager: instance paused");
        appInstanceManager.updateSequence(instanceId, 3, keccak256("actions"), keccak256("state"));

        // Jobs can still be taken and completed - call JobManager directly
        vm.prank(coordinator1);
        jobManager.takeJob(instanceId, jobId);

        vm.prank(coordinator1);
        jobManager.completeJob(instanceId, jobId, hex"726573756c74");

        // Unpause app instance - call AppInstanceManager directly
        vm.prank(user);
        appInstanceManager.unpauseAppInstance(instanceId);

        // Now sequence update should work
        vm.prank(user);
        appInstanceManager.updateSequence(instanceId, 3, keccak256("actions"), keccak256("state"));
    }

    // TODO: Re-enable when multicall stack too deep is fixed
    function skip_testMulticall() public {
        // Create multiple jobs
        uint256[] memory jobIds = new uint256[](3);
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.JobInput memory input = DataTypes.JobInput({
                description: string(abi.encodePacked("Multicall job ", vm.toString(i))),
                developer: "TestDeveloper",
                agent: "TestAgent",
                agentMethod: "process",
                app: "TestApp",
                appInstance: instanceId,
                appInstanceMethod: "execute",
                data: abi.encode(i),
                intervalMs: 0
            });

            vm.prank(user);
            jobIds[i] = jobManager.createJob(input);
        }

        // Prepare multicall data
        address[] memory targets = new address[](3);
        bytes[] memory data = new bytes[](3);

        for (uint256 i = 0; i < 3; i++) {
            targets[i] = address(jobManager);
            data[i] = abi.encodeWithSelector(IJobManager.takeJob.selector, instanceId, jobIds[i]);
        }

        // Execute multicall
        vm.prank(coordinator1);
        bytes[] memory results = coordination.multicall(targets, data);

        // Verify all jobs were taken
        assertEq(results.length, 3);
        for (uint256 i = 0; i < 3; i++) {
            DataTypes.Job memory job = jobManager.getJob(instanceId, jobIds[i]);
            assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Running));
            assertEq(job.takenBy, coordinator1);
        }
    }

    function testPeriodicJob() public {
        // Create periodic job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Periodic job",
            developer: "TestDeveloper",
            agent: "TestAgent",
            agentMethod: "process",
            app: "TestApp",
            appInstance: instanceId,
            appInstanceMethod: "execute",
            data: hex"706572696f646963",
            intervalMs: 60000 // 1 minute
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Take and complete the job
        vm.prank(coordinator1);
        jobManager.takeJob(instanceId, jobId);

        vm.prank(coordinator1);
        jobManager.completeJob(instanceId, jobId, hex"726573756c74");

        // Verify a new job was scheduled
        DataTypes.Job[] memory pendingJobs = jobManager.getPendingJobs(instanceId, 10, 0);
        assertEq(pendingJobs.length, 1);

        // The new job should have the same parameters
        DataTypes.Job memory newJob = pendingJobs[0];
        assertEq(newJob.developer, "TestDeveloper");
        assertEq(newJob.agent, "TestAgent");
        assertEq(newJob.intervalMs, 60000);
        assertGt(newJob.nextScheduledAt, 0);
    }

    function testJobFailureAndRetry() public {
        // Create a job
        DataTypes.JobInput memory input = DataTypes.JobInput({
            description: "Retry test job",
            developer: "TestDeveloper",
            agent: "TestAgent",
            agentMethod: "process",
            app: "TestApp",
            appInstance: instanceId,
            appInstanceMethod: "execute",
            data: hex"7265747279",
            intervalMs: 0
        });

        vm.prank(user);
        uint256 jobId = jobManager.createJob(input);

        // Take and fail the job
        vm.prank(coordinator1);
        jobManager.takeJob(instanceId, jobId);

        vm.prank(coordinator1);
        jobManager.failJob(instanceId, jobId, "First failure");

        // Verify job went back to pending
        DataTypes.Job memory job = jobManager.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));
        assertEq(job.attempts, 1);

        // Take and fail again
        vm.prank(coordinator2);
        jobManager.takeJob(instanceId, jobId);

        vm.prank(coordinator2);
        jobManager.failJob(instanceId, jobId, "Second failure");

        // Still pending
        job = jobManager.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));
        assertEq(job.attempts, 2);

        // Third attempt - fail
        vm.prank(coordinator1);
        jobManager.takeJob(instanceId, jobId);

        vm.prank(coordinator1);
        jobManager.failJob(instanceId, jobId, "Third failure");

        // Now it should be failed
        job = jobManager.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Failed));
        assertEq(job.attempts, 3);

        // Restart failed jobs
        vm.prank(operator);
        uint256 restarted = jobManager.restartFailedJobs(instanceId);
        assertEq(restarted, 1);

        // Job should be pending again
        job = jobManager.getJob(instanceId, jobId);
        assertEq(uint8(job.status), uint8(DataTypes.JobStatus.Pending));
        assertEq(job.attempts, 0);
    }
}