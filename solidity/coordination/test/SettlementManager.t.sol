// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/SettlementManager.sol";
import "../src/libraries/DataTypes.sol";

contract SettlementManagerTest is Test {
    SettlementManager public settlementManager;

    address public admin = address(1);
    address public coordinator = address(2);
    address public unauthorized = address(99);

    string constant APP_INSTANCE = "test-app";
    string constant CHAIN_ETH = "ethereum";
    string constant CHAIN_POLYGON = "polygon";
    string constant SETTLEMENT_ADDR = "0x1234567890abcdef";

    function setUp() public {
        vm.startPrank(admin);

        settlementManager = new SettlementManager();
        settlementManager.grantRole(settlementManager.COORDINATOR_ROLE(), coordinator);

        vm.stopPrank();
    }

    function testCreateSettlement() public {
        vm.prank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Verify settlement was created
        (
            string memory chain,
            uint64 lastSettled,
            string memory settlementAddr,
            uint64 settlementJob,
            bool exists
        ) = settlementManager.getSettlement(APP_INSTANCE, CHAIN_ETH);

        assertEq(chain, CHAIN_ETH);
        assertEq(lastSettled, 0);
        assertEq(settlementAddr, SETTLEMENT_ADDR);
        assertEq(settlementJob, 0);
        assertTrue(exists);

        // Verify it appears in chains list
        string[] memory chains = settlementManager.getSettlementChains(APP_INSTANCE);
        assertEq(chains.length, 1);
        assertEq(chains[0], CHAIN_ETH);
    }

    function testSetBlockSettlementTx() public {
        // Setup: create settlement
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Set block settlement transaction
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1, // blockNumber
            "0xtxhash123",
            false, // not included yet
            uint64(block.timestamp),
            0 // not settled yet
        );
        vm.stopPrank();

        // Verify block settlement
        (
            uint64 blockNum,
            string memory txHash,
            bool txIncluded,
            uint64 sentAt,
            uint64 settledAt,
            bool exists
        ) = settlementManager.getBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1);

        assertEq(blockNum, 1);
        assertEq(txHash, "0xtxhash123");
        assertFalse(txIncluded);
        assertEq(sentAt, uint64(block.timestamp));
        assertEq(settledAt, 0);
        assertTrue(exists);
    }

    function testBlockSettledEvent() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Set block settlement with txIncluded=true
        vm.expectEmit(true, true, true, true);
        emit DataTypes.BlockSettledEvent(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xtxhash123",
            uint64(block.timestamp),
            uint64(block.timestamp + 100),
            block.timestamp
        );

        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xtxhash123",
            true, // txIncluded
            uint64(block.timestamp),
            uint64(block.timestamp + 100)
        );
        vm.stopPrank();

        // Verify lastSettledBlockNumber was updated
        uint64 lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 1);
    }

    function testSequentialSettlement() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Settle blocks 1, 2, 3 in order
        for (uint64 i = 1; i <= 3; i++) {
            settlementManager.setBlockSettlementTx(
                APP_INSTANCE,
                CHAIN_ETH,
                i,
                string(abi.encodePacked("0xtxhash", i)),
                true,
                uint64(block.timestamp),
                uint64(block.timestamp + i * 100)
            );
        }
        vm.stopPrank();

        // Verify last settled block is 3
        uint64 lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 3);

        // Verify blocks 1 and 2 were purged (cleanup)
        bool exists1 = settlementManager.hasBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1);
        bool exists2 = settlementManager.hasBlockSettlement(APP_INSTANCE, CHAIN_ETH, 2);
        bool exists3 = settlementManager.hasBlockSettlement(APP_INSTANCE, CHAIN_ETH, 3);

        assertFalse(exists1); // Purged
        assertFalse(exists2); // Purged
        assertTrue(exists3);  // Current latest
    }

    function testSettlementPurging() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Settle blocks 1, 2, 3
        for (uint64 i = 1; i <= 3; i++) {
            settlementManager.setBlockSettlementTx(
                APP_INSTANCE,
                CHAIN_ETH,
                i,
                string(abi.encodePacked("0xtxhash", i)),
                true,
                uint64(block.timestamp),
                uint64(block.timestamp + i * 100)
            );
        }

        // Now settle block 5 (skipping 4)
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            5,
            "0xtxhash5",
            true,
            uint64(block.timestamp),
            uint64(block.timestamp + 500)
        );
        vm.stopPrank();

        // Last settled should still be 3 (can't skip block 4)
        uint64 lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 3);

        // Now settle block 4
        vm.prank(coordinator);
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            4,
            "0xtxhash4",
            true,
            uint64(block.timestamp),
            uint64(block.timestamp + 400)
        );

        // After settling block 4, the sequential settlement logic advances to 4
        // It stops at 4 because block 5 was already settled earlier
        lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 4);

        // To get to block 5, we need to trigger settlement logic again
        // We can do this by updating the existing block 5 settlement
        vm.prank(coordinator);
        settlementManager.updateBlockSettlementTxIncluded(APP_INSTANCE, CHAIN_ETH, 5);

        // Now last settled should be 5
        lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 5);
    }

    function testUpdateBlockSettlementTxHash() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Update tx hash (will create block settlement if doesn't exist)
        settlementManager.updateBlockSettlementTxHash(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xupdatedtxhash"
        );
        vm.stopPrank();

        // Verify
        (, string memory txHash,,,,) = settlementManager.getBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1);
        assertEq(txHash, "0xupdatedtxhash");
    }

    function testUpdateBlockSettlementTxIncluded() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // First create block settlement without inclusion
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xtxhash123",
            false,
            uint64(block.timestamp),
            0
        );

        // Now mark it as included
        settlementManager.updateBlockSettlementTxIncluded(APP_INSTANCE, CHAIN_ETH, 1);
        vm.stopPrank();

        // Verify it's now included
        (,, bool txIncluded,,,) = settlementManager.getBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1);
        assertTrue(txIncluded);

        // Verify lastSettled was updated
        uint64 lastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        assertEq(lastSettled, 1);
    }

    function testSetSettlementAddress() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Update settlement address
        string memory newAddr = "0xnewaddress";
        settlementManager.setSettlementAddress(APP_INSTANCE, CHAIN_ETH, newAddr);
        vm.stopPrank();

        // Verify
        string memory addr = settlementManager.getSettlementAddress(APP_INSTANCE, CHAIN_ETH);
        assertEq(addr, newAddr);
    }

    function testSetSettlementJob() public {
        vm.startPrank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Set settlement job
        uint64 jobId = 12345;
        settlementManager.setSettlementJob(APP_INSTANCE, CHAIN_ETH, jobId);
        vm.stopPrank();

        // Verify
        uint64 retrievedJobId = settlementManager.getSettlementJobForChain(APP_INSTANCE, CHAIN_ETH);
        assertEq(retrievedJobId, jobId);
    }

    function testMultipleChains() public {
        vm.startPrank(coordinator);

        // Create settlements for two chains
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, "0xeth");
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_POLYGON, "0xpoly");

        // Settle block 1 on ethereum
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xeth-tx",
            true,
            uint64(block.timestamp),
            uint64(block.timestamp + 100)
        );

        // Settle block 1 on polygon
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_POLYGON,
            1,
            "0xpoly-tx",
            true,
            uint64(block.timestamp),
            uint64(block.timestamp + 50)
        );

        vm.stopPrank();

        // Verify both chains are tracked
        string[] memory chains = settlementManager.getSettlementChains(APP_INSTANCE);
        assertEq(chains.length, 2);

        // Verify both have lastSettled = 1
        uint64 ethLastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_ETH);
        uint64 polyLastSettled = settlementManager.getLastSettledBlock(APP_INSTANCE, CHAIN_POLYGON);
        assertEq(ethLastSettled, 1);
        assertEq(polyLastSettled, 1);

        // Verify settlement addresses are different
        string memory ethAddr = settlementManager.getSettlementAddress(APP_INSTANCE, CHAIN_ETH);
        string memory polyAddr = settlementManager.getSettlementAddress(APP_INSTANCE, CHAIN_POLYGON);
        assertEq(ethAddr, "0xeth");
        assertEq(polyAddr, "0xpoly");
    }

    function testGetSettlementChains() public {
        vm.startPrank(coordinator);

        // Create 3 settlements
        settlementManager.createSettlement(APP_INSTANCE, "ethereum", "");
        settlementManager.createSettlement(APP_INSTANCE, "polygon", "");
        settlementManager.createSettlement(APP_INSTANCE, "arbitrum", "");

        vm.stopPrank();

        // Get all chains
        string[] memory chains = settlementManager.getSettlementChains(APP_INSTANCE);
        assertEq(chains.length, 3);
    }

    function testAccessControl() public {
        // Test that unauthorized user cannot create settlement
        vm.prank(unauthorized);
        vm.expectRevert();
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Create settlement as coordinator
        vm.prank(coordinator);
        settlementManager.createSettlement(APP_INSTANCE, CHAIN_ETH, SETTLEMENT_ADDR);

        // Test that unauthorized user cannot set block settlement
        vm.prank(unauthorized);
        vm.expectRevert();
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xtx",
            false,
            uint64(block.timestamp),
            0
        );

        // Test that unauthorized user cannot update settlement address
        vm.prank(unauthorized);
        vm.expectRevert();
        settlementManager.setSettlementAddress(APP_INSTANCE, CHAIN_ETH, "0xnew");

        // Test that unauthorized user cannot update settlement job
        vm.prank(unauthorized);
        vm.expectRevert();
        settlementManager.setSettlementJob(APP_INSTANCE, CHAIN_ETH, 123);

        // Test that only admin can purge
        vm.prank(coordinator); // coordinator is not admin
        vm.expectRevert();
        settlementManager.purgeBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1, "test");

        // Admin can purge - first create block settlement as coordinator
        vm.prank(coordinator);
        settlementManager.setBlockSettlementTx(
            APP_INSTANCE,
            CHAIN_ETH,
            1,
            "0xtx",
            false,
            uint64(block.timestamp),
            0
        );

        // Then admin purges it
        vm.prank(admin);
        settlementManager.purgeBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1, "admin cleanup");

        // Verify purged
        bool exists = settlementManager.hasBlockSettlement(APP_INSTANCE, CHAIN_ETH, 1);
        assertFalse(exists);

        // View functions should work for anyone
        settlementManager.getSettlement(APP_INSTANCE, CHAIN_ETH);
        settlementManager.getSettlementChains(APP_INSTANCE);
        settlementManager.hasSettlement(APP_INSTANCE, CHAIN_ETH);
    }
}
