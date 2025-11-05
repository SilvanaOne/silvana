// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/AddApp.sol";
import "coordination/interfaces/ICoordination.sol";
import "coordination/libraries/DataTypes.sol";

/**
 * @title AddAppTest
 * @notice Integration tests for AddApp example
 * @dev Tests require a running Anvil instance with deployed coordination contracts
 */
contract AddAppTest is Test {
    AddApp public addApp;
    ICoordination public coordination;
    string public appInstanceId;

    address public owner;
    uint256 public ownerPrivateKey;

    // Events to test
    event AppInitialized(
        string indexed appInstanceId,
        uint256 initialSum,
        bytes32 initialCommitment,
        uint256 timestamp
    );

    event ValueAdded(
        uint32 indexed index,
        uint256 oldValue,
        uint256 newValue,
        uint256 amountAdded,
        uint256 oldSum,
        uint256 newSum,
        bytes32 oldCommitment,
        bytes32 newCommitment,
        uint256 sequence,
        uint256 jobId
    );

    function setUp() public {
        // Use Anvil's first account
        ownerPrivateKey = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;
        owner = vm.addr(ownerPrivateKey);
        vm.startPrank(owner);

        // Get coordination address from environment or use default
        address coordinationAddress = vm.envOr(
            "SILVANA_COORDINATION_CONTRACT",
            address(0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00)
        );
        coordination = ICoordination(coordinationAddress);

        // Deploy AddApp
        addApp = new AddApp(coordinationAddress);

        // Generate Ed25519 public key as app instance ID
        string[] memory inputs = new string[](1);
        inputs[0] = "script/generate_ed25519_id.sh";
        bytes memory result = vm.ffi(inputs);
        // Convert bytes to 0x-prefixed hex string
        string memory generatedId = vm.toString(result);

        // Create app instance with the generated ID
        address appInstanceManagerAddr = address(coordination.appInstanceManager());
        (bool success, bytes memory data) = appInstanceManagerAddr.call(
            abi.encodeWithSignature(
                "createAppInstance(string,string,string,string)",
                generatedId,
                "test-add-app",
                "add-app",
                "silvana"
            )
        );
        require(success, "Failed to create app instance");
        appInstanceId = abi.decode(data, (string));

        // Initialize AddApp
        addApp.initialize(appInstanceId);

        vm.stopPrank();
    }

    function testInitialization() public view {
        assertEq(addApp.appInstanceId(), appInstanceId, "App instance ID should match");
        assertEq(addApp.sum(), 0, "Initial sum should be 0");
        assertEq(addApp.sequence(), 0, "Initial sequence should be 0");
        assertNotEq(addApp.stateCommitment(), bytes32(0), "State commitment should be set");
        assertEq(addApp.owner(), owner, "Owner should be set");
    }

    function testAddSingleValue() public {
        vm.startPrank(owner);

        uint32 index = 1;
        uint256 value = 50;

        // Expect ValueAdded event
        vm.expectEmit(true, false, false, false);
        emit ValueAdded(
            index,
            0,       // oldValue
            50,      // newValue
            50,      // amountAdded
            0,       // oldSum
            50,      // newSum
            bytes32(0), // oldCommitment (we don't check exact value)
            bytes32(0), // newCommitment (we don't check exact value)
            1,       // sequence
            0        // jobId (we don't check exact value)
        );

        uint256 jobId = addApp.add(index, value);

        // Verify state changes
        assertEq(addApp.getValue(index), 50, "Value at index 1 should be 50");
        assertEq(addApp.getSum(), 50, "Sum should be 50");
        assertEq(addApp.getSequence(), 1, "Sequence should be 1");
        assertTrue(jobId > 0, "Job ID should be greater than 0");

        vm.stopPrank();
    }

    function testAddMultipleValues() public {
        vm.startPrank(owner);

        // Add 10 at index 1
        addApp.add(1, 10);
        assertEq(addApp.getValue(1), 10, "Value at index 1 should be 10");
        assertEq(addApp.getSum(), 10, "Sum should be 10");
        assertEq(addApp.getSequence(), 1, "Sequence should be 1");

        // Add 20 at index 2
        addApp.add(2, 20);
        assertEq(addApp.getValue(2), 20, "Value at index 2 should be 20");
        assertEq(addApp.getSum(), 30, "Sum should be 30");
        assertEq(addApp.getSequence(), 2, "Sequence should be 2");

        // Add 15 more at index 1
        addApp.add(1, 15);
        assertEq(addApp.getValue(1), 25, "Value at index 1 should be 25");
        assertEq(addApp.getSum(), 45, "Sum should be 45");
        assertEq(addApp.getSequence(), 3, "Sequence should be 3");

        vm.stopPrank();
    }

    function testAddUpdatesStateCommitment() public {
        vm.startPrank(owner);

        bytes32 commitment1 = addApp.getStateCommitment();

        // Add a value
        addApp.add(1, 10);

        bytes32 commitment2 = addApp.getStateCommitment();
        assertTrue(commitment1 != commitment2, "State commitment should change after add");

        // Add another value
        addApp.add(2, 20);

        bytes32 commitment3 = addApp.getStateCommitment();
        assertTrue(commitment2 != commitment3, "State commitment should change again");

        vm.stopPrank();
    }

    function testRevertInvalidValue() public {
        vm.startPrank(owner);

        // Value >= 100 should revert
        vm.expectRevert(abi.encodeWithSelector(AddApp.InvalidValue.selector, 100));
        addApp.add(1, 100);

        vm.expectRevert(abi.encodeWithSelector(AddApp.InvalidValue.selector, 200));
        addApp.add(1, 200);

        vm.stopPrank();
    }

    function testRevertReservedIndex() public {
        vm.startPrank(owner);

        // Index 0 is reserved for sum
        vm.expectRevert(abi.encodeWithSelector(AddApp.ReservedIndex.selector, 0));
        addApp.add(0, 50);

        vm.stopPrank();
    }

    function testRevertInvalidIndex() public {
        vm.startPrank(owner);

        // Index >= MAX_INDEX should revert
        uint32 maxIndex = addApp.MAX_INDEX();
        vm.expectRevert(abi.encodeWithSelector(AddApp.InvalidIndex.selector, maxIndex));
        addApp.add(maxIndex, 50);

        vm.expectRevert(abi.encodeWithSelector(AddApp.InvalidIndex.selector, maxIndex + 1));
        addApp.add(maxIndex + 1, 50);

        vm.stopPrank();
    }

    function testGetValueUnsetIndex() public view {
        // Unset indexes should return 0
        assertEq(addApp.getValue(99), 0, "Unset index should return 0");
    }

    function testBoundaryValues() public {
        vm.startPrank(owner);

        // Test max valid value (99)
        addApp.add(1, 99);
        assertEq(addApp.getValue(1), 99, "Should accept value 99");
        assertEq(addApp.getSum(), 99, "Sum should be 99");

        // Test min valid value (0) - should work but not change anything
        addApp.add(2, 0);
        assertEq(addApp.getValue(2), 0, "Should accept value 0");
        assertEq(addApp.getSum(), 99, "Sum should still be 99");

        // Test min valid index (1)
        addApp.add(1, 1);
        assertEq(addApp.getValue(1), 100, "Should accept index 1");

        // Test max valid index - 1
        uint32 maxValidIndex = addApp.MAX_INDEX() - 1;
        addApp.add(maxValidIndex, 5);
        assertEq(addApp.getValue(maxValidIndex), 5, "Should accept max valid index");

        vm.stopPrank();
    }

    function testJobCreation() public {
        vm.startPrank(owner);

        // Record initial pending jobs count (there may be other jobs from setup)
        // Just verify that a new job is created

        uint256 jobId = addApp.add(1, 25);

        // Verify job exists and is pending
        // Note: This requires the coordination layer to be deployed and working
        assertTrue(jobId > 0, "Job should be created");

        vm.stopPrank();
    }

    function testSequenceIncrementation() public {
        vm.startPrank(owner);

        assertEq(addApp.getSequence(), 0, "Initial sequence should be 0");

        addApp.add(1, 10);
        assertEq(addApp.getSequence(), 1, "Sequence should be 1");

        addApp.add(2, 20);
        assertEq(addApp.getSequence(), 2, "Sequence should be 2");

        addApp.add(3, 30);
        assertEq(addApp.getSequence(), 3, "Sequence should be 3");

        vm.stopPrank();
    }

    function testAddFromNonOwner() public {
        // Anyone can call add, not just owner
        address user = address(0x123);
        vm.deal(user, 1 ether);
        vm.startPrank(user);

        addApp.add(1, 50);
        assertEq(addApp.getSum(), 50, "Non-owner should be able to add");

        vm.stopPrank();
    }

    function testRevertNotInitialized() public {
        vm.startPrank(owner);

        // Deploy a fresh AddApp without initializing
        AddApp uninitializedApp = new AddApp(address(coordination));

        vm.expectRevert(AddApp.NotInitialized.selector);
        uninitializedApp.add(1, 50);

        vm.stopPrank();
    }
}
