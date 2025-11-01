// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/AppInstanceManager.sol";
import "../src/libraries/DataTypes.sol";

contract AppInstanceManagerTest is Test {
    AppInstanceManager public appInstanceManager;

    address admin = address(0x1);
    address coordinator = address(0x2);
    address user = address(0x3);
    address user2 = address(0x4);

    string constant APP_NAME = "TestApp";
    string constant DEVELOPER_NAME = "TestDeveloper";

    function setUp() public {
        // Deploy AppInstanceManager
        vm.prank(admin);
        appInstanceManager = new AppInstanceManager();

        // Grant roles
        vm.startPrank(admin);
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator);
        vm.stopPrank();
    }

    function testCreateAppInstance() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Verify instance was created
        assertTrue(appInstanceManager.appInstanceExists(instanceId));

        DataTypes.AppInstance memory instance = appInstanceManager.getAppInstance(instanceId);
        assertEq(instance.id, instanceId);
        assertEq(instance.name, "test-instance");
        assertEq(instance.owner, user);
        assertEq(instance.silvanaAppName, APP_NAME);
        assertEq(instance.developerName, DEVELOPER_NAME);
        assertEq(instance.sequenceNumber, 0);
        assertFalse(instance.isPaused);
    }

    function testDeleteAppInstance() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Delete app instance
        vm.prank(user);
        appInstanceManager.deleteAppInstance(instanceId);

        // Verify instance was deleted
        assertFalse(appInstanceManager.appInstanceExists(instanceId));
    }

    function testUpdateSequence() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Update sequence
        bytes32 actionsCommitment = keccak256("actions");
        bytes32 stateCommitment = keccak256("state");

        vm.prank(user);
        appInstanceManager.updateSequence(instanceId, 1, actionsCommitment, stateCommitment);

        // Verify sequence was updated
        DataTypes.AppInstance memory instance = appInstanceManager.getAppInstance(instanceId);
        assertEq(instance.sequenceNumber, 1);
        assertEq(instance.actionsCommitment, actionsCommitment);
        assertEq(instance.stateCommitment, stateCommitment);

        DataTypes.SequenceState memory state = appInstanceManager.getSequenceState(instanceId);
        assertEq(state.sequenceNumber, 1);
        assertEq(state.actionsCommitment, actionsCommitment);
        assertEq(state.stateCommitment, stateCommitment);
    }

    function testPauseAndUnpauseAppInstance() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Pause app instance
        vm.prank(user);
        appInstanceManager.pauseAppInstance(instanceId);

        // Verify instance is paused
        assertTrue(appInstanceManager.isAppInstancePaused(instanceId));
        assertFalse(appInstanceManager.isAppInstanceActive(instanceId));

        // Unpause app instance
        vm.prank(user);
        appInstanceManager.unpauseAppInstance(instanceId);

        // Verify instance is active
        assertFalse(appInstanceManager.isAppInstancePaused(instanceId));
        assertTrue(appInstanceManager.isAppInstanceActive(instanceId));
    }

    function testTransferOwnership() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Transfer ownership
        vm.prank(user);
        appInstanceManager.transferOwnership(instanceId, user2);

        // Verify ownership was transferred
        DataTypes.AppInstance memory instance = appInstanceManager.getAppInstance(instanceId);
        assertEq(instance.owner, user2);
        assertEq(appInstanceManager.getAppInstanceOwner(instanceId), user2);

        // Verify old owner can't control it anymore
        vm.prank(user);
        vm.expectRevert("AppInstanceManager: not instance owner");
        appInstanceManager.pauseAppInstance(instanceId);

        // New owner can control it
        vm.prank(user2);
        appInstanceManager.pauseAppInstance(instanceId);
    }

    function testCreateBlock() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Update sequence first
        vm.prank(user);
        appInstanceManager.updateSequence(instanceId, 10, keccak256("actions1"), keccak256("state1"));

        // Create a block
        bytes32 actionsCommitment = keccak256("actions2");
        bytes32 stateCommitment = keccak256("state2");

        vm.prank(coordinator);
        uint64 blockNumber = appInstanceManager.createBlock(instanceId, 20, actionsCommitment, stateCommitment);

        // Verify block was created
        assertEq(blockNumber, 1);

        DataTypes.Block memory blockData = appInstanceManager.getBlock(instanceId, blockNumber);
        assertEq(blockData.blockNumber, 1);
        assertEq(blockData.startSequence, 1);
        assertEq(blockData.endSequence, 20);
        assertEq(blockData.actionsCommitment, actionsCommitment);
        assertEq(blockData.stateCommitment, stateCommitment);
        assertEq(blockData.numberOfTransactions, 20);
    }

    function testSetDataAvailability() public {
        // Create app instance and block
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        vm.prank(coordinator);
        uint64 blockNumber = appInstanceManager.createBlock(instanceId, 10, keccak256("actions"), keccak256("state"));

        // Set data availability
        string memory stateDA = "ipfs://state-hash";
        string memory proofDA = "ipfs://proof-hash";

        vm.prank(coordinator);
        appInstanceManager.setDataAvailability(instanceId, blockNumber, stateDA, proofDA);

        // Verify DA was set
        DataTypes.Block memory blockData = appInstanceManager.getBlock(instanceId, blockNumber);
        assertEq(blockData.stateDataAvailability, stateDA);
        assertEq(blockData.proofDataAvailability, proofDA);
        assertGt(blockData.stateCalculatedAt, 0);
        assertGt(blockData.provedAt, 0);
    }

    function testCapabilityManagement() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Create capability for user2
        vm.prank(user);
        bytes32 capId = appInstanceManager.createAppInstanceCap(instanceId, user2);

        // Verify capability
        assertTrue(appInstanceManager.verifyAppInstanceCap(capId, user2));
        assertFalse(appInstanceManager.verifyAppInstanceCap(capId, user));

        // Revoke capability
        vm.prank(user);
        appInstanceManager.revokeAppInstanceCap(capId);

        // Verify capability is revoked
        assertFalse(appInstanceManager.verifyAppInstanceCap(capId, user2));
    }

    function testGetAppInstances() public {
        // Create multiple instances
        vm.startPrank(user);
        string memory id1 = appInstanceManager.createAppInstance("instance-1", APP_NAME, DEVELOPER_NAME);
        string memory id2 = appInstanceManager.createAppInstance("instance-2", APP_NAME, DEVELOPER_NAME);
        string memory id3 = appInstanceManager.createAppInstance("instance-3", APP_NAME, DEVELOPER_NAME);
        vm.stopPrank();

        // Get all instances
        DataTypes.AppInstance[] memory instances = appInstanceManager.getAppInstances(10, 0);

        // Verify we got all 3 instances
        assertEq(instances.length, 3);
        assertEq(instances[0].id, id1);
        assertEq(instances[1].id, id2);
        assertEq(instances[2].id, id3);

        // Test pagination
        instances = appInstanceManager.getAppInstances(2, 1);
        assertEq(instances.length, 2);
        assertEq(instances[0].id, id2);
        assertEq(instances[1].id, id3);
    }

    function testGetAppInstancesByOwner() public {
        // Create instances for different owners
        vm.prank(user);
        string memory id1 = appInstanceManager.createAppInstance("user1-instance", APP_NAME, DEVELOPER_NAME);

        vm.prank(user2);
        string memory id2 = appInstanceManager.createAppInstance("user2-instance", APP_NAME, DEVELOPER_NAME);

        // Get instances by owner
        string[] memory userInstances = appInstanceManager.getAppInstancesByOwner(user);
        assertEq(userInstances.length, 1);
        assertEq(userInstances[0], id1);

        string[] memory user2Instances = appInstanceManager.getAppInstancesByOwner(user2);
        assertEq(user2Instances.length, 1);
        assertEq(user2Instances[0], id2);
    }

    function testGetBlocksRange() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Create multiple blocks
        vm.startPrank(coordinator);
        appInstanceManager.createBlock(instanceId, 10, keccak256("actions1"), keccak256("state1"));

        vm.warp(121); // Warp to absolute timestamp 121 (120 seconds after block 1 at timestamp 1)
        appInstanceManager.createBlock(instanceId, 20, keccak256("actions2"), keccak256("state2"));

        vm.warp(241); // Warp to absolute timestamp 241 (120 seconds after block 2 at timestamp 121)
        appInstanceManager.createBlock(instanceId, 30, keccak256("actions3"), keccak256("state3"));
        vm.stopPrank();

        // Get blocks range
        DataTypes.Block[] memory blocks = appInstanceManager.getBlocksRange(instanceId, 1, 3);

        // Verify we got all 3 blocks
        assertEq(blocks.length, 3);
        assertEq(blocks[0].blockNumber, 1);
        assertEq(blocks[1].blockNumber, 2);
        assertEq(blocks[2].blockNumber, 3);
    }

    function testMinTimeBetweenBlocks() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Create first block
        vm.prank(coordinator);
        appInstanceManager.createBlock(instanceId, 10, keccak256("actions1"), keccak256("state1"));

        // Try to create second block too soon
        vm.prank(coordinator);
        vm.expectRevert("AppInstanceManager: too soon for new block");
        appInstanceManager.createBlock(instanceId, 20, keccak256("actions2"), keccak256("state2"));

        // Wait and create second block
        vm.warp(121); // 120 seconds after first block at timestamp 1
        vm.prank(coordinator);
        uint64 blockNumber = appInstanceManager.createBlock(instanceId, 20, keccak256("actions2"), keccak256("state2"));
        assertEq(blockNumber, 2);

        // Set longer min time
        vm.prank(user);
        appInstanceManager.setMinTimeBetweenBlocks(instanceId, 120000); // 2 minutes

        // Try to create third block too soon
        vm.warp(181); // Only 60 seconds after second block at 121
        vm.prank(coordinator);
        vm.expectRevert("AppInstanceManager: too soon for new block");
        appInstanceManager.createBlock(instanceId, 30, keccak256("actions3"), keccak256("state3"));

        // Wait longer and create
        vm.warp(241); // 120 seconds after second block at 121
        vm.prank(coordinator);
        blockNumber = appInstanceManager.createBlock(instanceId, 30, keccak256("actions3"), keccak256("state3"));
        assertEq(blockNumber, 3);
    }

    function testAccessControl() public {
        // Create app instance
        vm.prank(user);
        string memory instanceId = appInstanceManager.createAppInstance("test-instance", APP_NAME, DEVELOPER_NAME);

        // Try to pause without being owner
        vm.prank(user2);
        vm.expectRevert("AppInstanceManager: not instance owner");
        appInstanceManager.pauseAppInstance(instanceId);

        // Try to create block without coordinator role
        vm.prank(user);
        vm.expectRevert("AppInstanceManager: not coordinator");
        appInstanceManager.createBlock(instanceId, 10, keccak256("actions"), keccak256("state"));

        // Admin can do owner operations
        vm.prank(admin);
        appInstanceManager.pauseAppInstance(instanceId);
    }
}