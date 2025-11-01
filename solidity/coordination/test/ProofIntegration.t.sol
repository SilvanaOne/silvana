// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/SilvanaCoordination.sol";
import "../src/JobManager.sol";
import "../src/AppInstanceManager.sol";
import "../src/ProofManager.sol";
import "../src/libraries/DataTypes.sol";

contract ProofIntegrationTest is Test {
    SilvanaCoordination public coordination;
    JobManager public jobManager;
    AppInstanceManager public appInstanceManager;
    ProofManager public proofManager;

    address public admin = address(1);
    address public coordinator = address(2);
    address public operator = address(3);
    address public user = address(4);

    string public appInstance;
    string constant APP_NAME = "TestApp";
    string constant DEVELOPER_NAME = "TestDev";

    function setUp() public {
        vm.startPrank(admin);

        // Deploy all contracts
        jobManager = new JobManager();
        appInstanceManager = new AppInstanceManager();
        proofManager = new ProofManager();
        coordination = new SilvanaCoordination();

        // Initialize coordination with all components (no SettlementManager/StorageManager for this test)
        coordination.initialize(
            admin,
            address(jobManager),
            address(appInstanceManager),
            address(proofManager),
            address(0),
            address(0)
        );

        // Set ProofManager in AppInstanceManager
        appInstanceManager.setProofManager(address(proofManager));

        // Grant roles to coordination contract
        jobManager.grantRole(jobManager.COORDINATOR_ROLE(), address(coordination));
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), address(coordination));
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(coordination));

        // AppInstanceManager needs to call ProofManager.createProofCalculation
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), address(appInstanceManager));

        // Grant coordinator role to coordinator address
        coordination.grantRole(coordination.COORDINATOR_ROLE(), coordinator);
        appInstanceManager.grantRole(appInstanceManager.COORDINATOR_ROLE(), coordinator);
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), coordinator);

        vm.stopPrank();

        // Create app instance and capture the returned ID
        vm.prank(coordinator);
        appInstance = coordination.createAppInstance("test-app", APP_NAME, DEVELOPER_NAME);
    }

    function testBlockCreatesProofCalculation() public {
        // Create first block (sequences 1-10)
        vm.prank(coordinator);
        uint64 block1 = appInstanceManager.createBlock(
            appInstance,
            10, // endSequence
            keccak256("actions1"),
            keccak256("state1")
        );
        assertEq(block1, 1);

        // Verify proof calculation was created for block 2 (next block)
        (uint64 blockNum, uint64 startSeq, uint64 endSeq, string memory blockProof, bool isFinished) =
            proofManager.getProofCalculation(appInstance, 2);

        assertEq(blockNum, 2);
        assertEq(startSeq, 11); // Next block starts at 11
        assertEq(endSeq, 0); // End sequence unknown yet
        assertEq(bytes(blockProof).length, 0);
        assertFalse(isFinished);

        // Wait before creating second block (min 120s between blocks)
        vm.warp(block.timestamp + 121);

        // Create second block (sequences 11-20)
        vm.prank(coordinator);
        uint64 block2 = appInstanceManager.createBlock(
            appInstance,
            20, // endSequence
            keccak256("actions2"),
            keccak256("state2")
        );
        assertEq(block2, 2);

        // Verify proof calculation was created for block 3 (next block)
        (blockNum, startSeq, endSeq, blockProof, isFinished) =
            proofManager.getProofCalculation(appInstance, 3);

        assertEq(blockNum, 3);
        assertEq(startSeq, 21); // Next block starts at 21
        assertEq(endSeq, 0);
        assertEq(bytes(blockProof).length, 0);
        assertFalse(isFinished);
    }

    function testFullProofWorkflow() public {
        // Step 1: Create block 1 (sequences 1-10)
        // This will create a proof calculation for block 2 starting at sequence 11
        vm.prank(coordinator);
        uint64 block1 = appInstanceManager.createBlock(
            appInstance,
            10,
            keccak256("actions1"),
            keccak256("state1")
        );
        assertEq(block1, 1);

        // Step 2: Manually create proof calculation for block 1 (sequences 1-10)
        vm.prank(coordinator);
        proofManager.createProofCalculation(appInstance, 1, 1, 10);

        // Step 3: Start proving through coordination (full range)
        uint64[] memory proofSeq = new uint64[](2);
        proofSeq[0] = 1;
        proofSeq[1] = 10;

        uint64[] memory emptySeq = new uint64[](0);

        vm.prank(coordinator);
        bool startSuccess = coordination.startProving(appInstance, 1, proofSeq, emptySeq, emptySeq);
        assertTrue(startSuccess);

        // Verify proof is STARTED
        uint8 status = proofManager.getProofStatus(appInstance, 1, proofSeq);
        assertEq(status, proofManager.PROOF_STATUS_STARTED());

        // Step 4: Submit proof through coordination
        vm.prank(coordinator);
        bool isBlockProof = coordination.submitProof(
            appInstance,
            1,
            proofSeq,
            emptySeq,
            emptySeq,
            "job-1",
            "QmProofHash",
            8,
            "x86_64",
            16000,
            5000
        );
        assertTrue(isBlockProof); // Covers full block range

        // Verify proof calculation is finished
        bool finished = proofManager.isProofCalculationFinished(appInstance, 1);
        assertTrue(finished);

        // Verify block proof is set
        (,, , string memory blockProof,) = proofManager.getProofCalculation(appInstance, 1);
        assertEq(blockProof, "QmProofHash");
    }

    function testRecursiveProofWorkflow() public {
        // Create block 1 and manually create its proof calculation
        vm.prank(coordinator);
        appInstanceManager.createBlock(appInstance, 10, keccak256("actions1"), keccak256("state1"));

        vm.prank(coordinator);
        proofManager.createProofCalculation(appInstance, 1, 1, 10);

        uint64[] memory emptySeq = new uint64[](0);

        // Level 1: Create two base proofs for halves of the range
        uint64[] memory seq1_5 = new uint64[](2);
        seq1_5[0] = 1;
        seq1_5[1] = 5;

        uint64[] memory seq6_10 = new uint64[](2);
        seq6_10[0] = 6;
        seq6_10[1] = 10;

        vm.startPrank(coordinator);

        // Prove and submit first half
        coordination.startProving(appInstance, 1, seq1_5, emptySeq, emptySeq);
        coordination.submitProof(appInstance, 1, seq1_5, emptySeq, emptySeq, "job-1", "QmHash1", 4, "x86_64", 8000, 2000);

        // Prove and submit second half
        coordination.startProving(appInstance, 1, seq6_10, emptySeq, emptySeq);
        coordination.submitProof(appInstance, 1, seq6_10, emptySeq, emptySeq, "job-2", "QmHash2", 4, "x86_64", 8000, 2000);

        // Level 2: Merge proofs to cover full range
        uint64[] memory seqAll = new uint64[](2);
        seqAll[0] = 1;
        seqAll[1] = 10;

        bool startSuccess = coordination.startProving(appInstance, 1, seqAll, seq1_5, seq6_10);
        assertTrue(startSuccess);

        // Submit merged proof
        bool isBlockProof = coordination.submitProof(
            appInstance,
            1,
            seqAll,
            seq1_5,
            seq6_10,
            "job-3",
            "QmMergedHash",
            8,
            "x86_64",
            16000,
            4000
        );
        assertTrue(isBlockProof);

        vm.stopPrank();

        // Verify block proof calculation is finished
        bool finished = coordination.isProofCalculationFinished(appInstance, 1);
        assertTrue(finished);

        // Verify sub-proofs are USED
        uint8 status1 = proofManager.getProofStatus(appInstance, 1, seq1_5);
        uint8 status2 = proofManager.getProofStatus(appInstance, 1, seq6_10);
        assertEq(status1, proofManager.PROOF_STATUS_USED());
        assertEq(status2, proofManager.PROOF_STATUS_USED());
    }

    function testProofRejectionAndRetry() public {
        // Create block 1 and manually create its proof calculation
        vm.prank(coordinator);
        appInstanceManager.createBlock(appInstance, 10, keccak256("actions1"), keccak256("state1"));

        vm.prank(coordinator);
        proofManager.createProofCalculation(appInstance, 1, 1, 10);

        uint64[] memory sequences = new uint64[](2);
        sequences[0] = 1;
        sequences[1] = 10;

        uint64[] memory emptySeq = new uint64[](0);

        vm.startPrank(coordinator);

        // First attempt: Start and reject
        coordination.startProving(appInstance, 1, sequences, emptySeq, emptySeq);
        coordination.rejectProof(appInstance, 1, sequences);

        // Verify proof is REJECTED
        uint8 status1 = proofManager.getProofStatus(appInstance, 1, sequences);
        assertEq(status1, proofManager.PROOF_STATUS_REJECTED());

        // Verify rejection count
        (,,,, uint16 rejectedCount1,,,,) = proofManager.getProof(appInstance, 1, sequences);
        assertEq(rejectedCount1, 1);

        // Second attempt: Restart, reject again
        coordination.startProving(appInstance, 1, sequences, emptySeq, emptySeq);
        coordination.rejectProof(appInstance, 1, sequences);

        // Verify rejection count increased
        (,,,, uint16 rejectedCount2,,,,) = proofManager.getProof(appInstance, 1, sequences);
        assertEq(rejectedCount2, 2);

        // Third attempt: Restart and successfully submit
        coordination.startProving(appInstance, 1, sequences, emptySeq, emptySeq);
        bool isBlockProof = coordination.submitProof(
            appInstance,
            1,
            sequences,
            emptySeq,
            emptySeq,
            "job-final",
            "QmFinalHash",
            4,
            "x86_64",
            8000,
            1000
        );
        assertTrue(isBlockProof);

        vm.stopPrank();

        // Verify proof is CALCULATED and rejection count preserved
        uint8 finalStatus = proofManager.getProofStatus(appInstance, 1, sequences);
        assertEq(finalStatus, proofManager.PROOF_STATUS_CALCULATED());

        (,,,, uint16 finalRejectedCount,,,,) = proofManager.getProof(appInstance, 1, sequences);
        assertEq(finalRejectedCount, 2); // Still shows rejection history
    }

    function testProofDelegationAfterSettlement() public {
        // This test verifies that proofs can still be queried after a block is settled

        // Create block 1 and manually create its proof calculation
        vm.prank(coordinator);
        appInstanceManager.createBlock(appInstance, 10, keccak256("actions1"), keccak256("state1"));

        vm.prank(coordinator);
        proofManager.createProofCalculation(appInstance, 1, 1, 10);

        uint64[] memory sequences = new uint64[](2);
        sequences[0] = 1;
        sequences[1] = 10;

        uint64[] memory emptySeq = new uint64[](0);

        vm.startPrank(coordinator);
        coordination.startProving(appInstance, 1, sequences, emptySeq, emptySeq);
        coordination.submitProof(
            appInstance,
            1,
            sequences,
            emptySeq,
            emptySeq,
            "job-1",
            "QmProofHash",
            4,
            "x86_64",
            8000,
            1000
        );
        vm.stopPrank();

        // Verify proof calculation exists and is finished
        (uint64 blockNum, uint64 startSeq, uint64 endSeq, string memory blockProof, bool isFinished) =
            coordination.getProofCalculation(appInstance, 1);

        assertEq(blockNum, 1);
        assertEq(startSeq, 1);
        assertEq(endSeq, 10);
        assertEq(blockProof, "QmProofHash");
        assertTrue(isFinished);

        // Verify proof details are accessible
        (
            uint8 status,
            string memory daHash,
            uint64[] memory seq1,
            uint64[] memory seq2,
            uint16 rejectedCount,
            uint256 timestamp,
            address prover,
            ,
            string memory jobId
        ) = coordination.getProof(appInstance, 1, sequences);

        assertEq(status, proofManager.PROOF_STATUS_CALCULATED());
        assertEq(daHash, "QmProofHash");
        assertEq(seq1.length, 0);
        assertEq(seq2.length, 0);
        assertEq(rejectedCount, 0);
        assertGt(timestamp, 0);
        assertEq(prover, address(coordination)); // Prover is coordination contract when called through delegation
        assertEq(jobId, "job-1");

        // Verify proof status query
        uint8 proofStatus = proofManager.getProofStatus(appInstance, 1, sequences);
        assertEq(proofStatus, proofManager.PROOF_STATUS_CALCULATED());
    }
}
