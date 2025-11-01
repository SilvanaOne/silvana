// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "forge-std/Test.sol";
import "../src/ProofManager.sol";
import "../src/libraries/DataTypes.sol";

contract ProofManagerTest is Test {
    ProofManager public proofManager;

    address public admin = address(1);
    address public coordinator = address(2);
    address public user1 = address(3);
    address public user2 = address(4);

    string constant APP_INSTANCE = "test-app";
    uint64 constant BLOCK_NUMBER = 1;
    uint64 constant START_SEQUENCE = 100;
    uint64 constant END_SEQUENCE = 200;

    function setUp() public {
        vm.startPrank(admin);

        proofManager = new ProofManager();
        proofManager.grantRole(proofManager.COORDINATOR_ROLE(), coordinator);

        vm.stopPrank();
    }

    function testCreateProofCalculation() public {
        vm.prank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        (uint64 blockNum, uint64 startSeq, uint64 endSeq, string memory blockProof, bool isFinished) =
            proofManager.getProofCalculation(APP_INSTANCE, BLOCK_NUMBER);

        assertEq(blockNum, BLOCK_NUMBER);
        assertEq(startSeq, START_SEQUENCE);
        assertEq(endSeq, END_SEQUENCE);
        assertEq(bytes(blockProof).length, 0);
        assertFalse(isFinished);
    }

    function testStartProving() public {
        // Setup: Create proof calculation
        vm.prank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        // Start proving a single sequence proof (no sub-proofs)
        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        vm.prank(coordinator);
        bool success = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        assertTrue(success);

        // Verify proof status
        (uint8 status,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(status, proofManager.PROOF_STATUS_STARTED());
    }

    function testStartProvingWithSubProofs() public {
        // Setup: Create proof calculation
        vm.prank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        // First, create and submit two base proofs
        uint64[] memory seq1 = new uint64[](2);
        seq1[0] = 100;
        seq1[1] = 110;

        uint64[] memory seq2 = new uint64[](2);
        seq2[0] = 111;
        seq2[1] = 120;

        uint64[] memory emptySeq = new uint64[](0);

        // Start and submit first proof
        vm.startPrank(coordinator);
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq1, emptySeq, emptySeq);
        proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            seq1,
            emptySeq,
            emptySeq,
            "job1",
            "hash1",
            4,
            "x86_64",
            8000,
            1000
        );

        // Start and submit second proof
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq2, emptySeq, emptySeq);
        proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            seq2,
            emptySeq,
            emptySeq,
            "job2",
            "hash2",
            4,
            "x86_64",
            8000,
            1000
        );
        vm.stopPrank();

        // Now create a merged proof using the two base proofs as sub-proofs
        uint64[] memory mergedSeq = new uint64[](4);
        mergedSeq[0] = 100;
        mergedSeq[1] = 110;
        mergedSeq[2] = 111;
        mergedSeq[3] = 120;

        vm.prank(coordinator);
        bool success = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, mergedSeq, seq1, seq2);

        assertTrue(success);

        // Verify merged proof status
        (uint8 status,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, mergedSeq);
        assertEq(status, proofManager.PROOF_STATUS_STARTED());

        // Verify sub-proofs are now RESERVED
        (uint8 status1,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq1);
        (uint8 status2,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq2);
        assertEq(status1, proofManager.PROOF_STATUS_RESERVED());
        assertEq(status2, proofManager.PROOF_STATUS_RESERVED());
    }

    function testSubmitProof() public {
        // Setup: Create proof calculation and start proving
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        // Submit proof
        bool isBlockProof = proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            sequences,
            emptySeq,
            emptySeq,
            "test-job-id",
            "QmTestHash123",
            8,
            "arm64",
            16000,
            5000
        );
        vm.stopPrank();

        assertFalse(isBlockProof); // Single sequence doesn't complete the block

        // Verify proof details
        (
            uint8 status,
            string memory daHash,
            uint64[] memory seq1,
            uint64[] memory seq2,
            uint16 rejectedCount,
            uint256 timestamp,
            address prover,
            address userAddr,
            string memory jobId
        ) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);

        assertEq(status, proofManager.PROOF_STATUS_CALCULATED());
        assertEq(daHash, "QmTestHash123");
        assertEq(seq1.length, 0);
        assertEq(seq2.length, 0);
        assertEq(rejectedCount, 0);
        assertEq(prover, coordinator);
        assertEq(jobId, "test-job-id");
    }

    function testSubmitBlockProof() public {
        // Setup: Create proof calculation
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        // Create a proof that covers the entire block range
        uint64[] memory fullRange = new uint64[](2);
        fullRange[0] = START_SEQUENCE;
        fullRange[1] = END_SEQUENCE;

        uint64[] memory emptySeq = new uint64[](0);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, fullRange, emptySeq, emptySeq);

        // Submit the block proof
        bool isBlockProof = proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            fullRange,
            emptySeq,
            emptySeq,
            "block-job-id",
            "QmBlockHash456",
            16,
            "x86_64",
            32000,
            10000
        );
        vm.stopPrank();

        assertTrue(isBlockProof); // Covers full range, so it's a block proof

        // Verify proof calculation is finished
        (,,,, bool isFinished) = proofManager.getProofCalculation(APP_INSTANCE, BLOCK_NUMBER);
        assertTrue(isFinished);

        // Verify the block proof hash is set
        (,, , string memory blockProof,) = proofManager.getProofCalculation(APP_INSTANCE, BLOCK_NUMBER);
        assertEq(blockProof, "QmBlockHash456");
    }

    function testRejectProof() public {
        // Setup: Create proof calculation and start proving
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        // Reject proof
        proofManager.rejectProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        vm.stopPrank();

        // Verify proof status and rejection count
        (uint8 status,,,, uint16 rejectedCount,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);

        assertEq(status, proofManager.PROOF_STATUS_REJECTED());
        assertEq(rejectedCount, 1);

        // Verify can restart the proof
        vm.prank(coordinator);
        bool success = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        assertTrue(success);

        // Verify rejection count is preserved
        (,,,, uint16 newRejectedCount,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(newRejectedCount, 1);
    }

    function testRejectProofReturnsSubProofs() public {
        // Setup: Create base proofs
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory seq1 = new uint64[](1);
        seq1[0] = 100;

        uint64[] memory seq2 = new uint64[](1);
        seq2[0] = 110;

        uint64[] memory emptySeq = new uint64[](0);

        // Create and submit base proofs
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq1, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq1, emptySeq, emptySeq, "job1", "hash1", 4, "x86_64", 8000, 1000);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq2, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq2, emptySeq, emptySeq, "job2", "hash2", 4, "x86_64", 8000, 1000);

        // Create merged proof
        uint64[] memory mergedSeq = new uint64[](2);
        mergedSeq[0] = 100;
        mergedSeq[1] = 110;

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, mergedSeq, seq1, seq2);

        // Verify sub-proofs are RESERVED
        (uint8 status1Before,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq1);
        (uint8 status2Before,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq2);
        assertEq(status1Before, proofManager.PROOF_STATUS_RESERVED());
        assertEq(status2Before, proofManager.PROOF_STATUS_RESERVED());

        // Reject merged proof
        proofManager.rejectProof(APP_INSTANCE, BLOCK_NUMBER, mergedSeq);
        vm.stopPrank();

        // Verify sub-proofs are returned to CALCULATED status
        (uint8 status1After,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq1);
        (uint8 status2After,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq2);
        assertEq(status1After, proofManager.PROOF_STATUS_CALCULATED());
        assertEq(status2After, proofManager.PROOF_STATUS_CALCULATED());
    }

    function testProofTimeout() public {
        // Setup: Create proof calculation and start proving
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        vm.stopPrank();

        // Verify proof is STARTED
        (uint8 statusBefore,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(statusBefore, proofManager.PROOF_STATUS_STARTED());

        // Try to start again immediately - should fail
        vm.prank(coordinator);
        bool successBefore = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        assertFalse(successBefore);

        // Advance time past timeout (5 minutes + 1 second)
        vm.warp(block.timestamp + 301);

        // Now should be able to restart
        vm.prank(coordinator);
        bool successAfter = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        assertTrue(successAfter);

        // Verify proof was restarted (status still STARTED, but timestamp updated)
        (uint8 statusAfter,,,, uint16 rejectedCount, uint256 timestamp,,,) =
            proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(statusAfter, proofManager.PROOF_STATUS_STARTED());
        assertEq(rejectedCount, 0);
        assertGt(timestamp, 0);
    }

    function testCannotStartTwice() public {
        // Setup: Create proof calculation
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        // Start first time - should succeed
        bool success1 = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        assertTrue(success1);

        // Try to start second time - should fail
        bool success2 = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        assertFalse(success2);
        vm.stopPrank();
    }

    function testRecursiveProofComposition() public {
        // Test a 3-level proof tree:
        // Level 1: [100], [110], [120], [130]
        // Level 2: [100,110], [120,130]
        // Level 3: [100,110,120,130]

        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory emptySeq = new uint64[](0);

        // Level 1: Create 4 base proofs
        uint64[] memory seq100 = new uint64[](1);
        seq100[0] = 100;
        uint64[] memory seq110 = new uint64[](1);
        seq110[0] = 110;
        uint64[] memory seq120 = new uint64[](1);
        seq120[0] = 120;
        uint64[] memory seq130 = new uint64[](1);
        seq130[0] = 130;

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq100, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq100, emptySeq, emptySeq, "job1", "hash1", 4, "x86_64", 8000, 1000);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq110, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq110, emptySeq, emptySeq, "job2", "hash2", 4, "x86_64", 8000, 1000);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq120, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq120, emptySeq, emptySeq, "job3", "hash3", 4, "x86_64", 8000, 1000);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq130, emptySeq, emptySeq);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq130, emptySeq, emptySeq, "job4", "hash4", 4, "x86_64", 8000, 1000);

        // Level 2: Merge into 2 proofs
        uint64[] memory seq100_110 = new uint64[](2);
        seq100_110[0] = 100;
        seq100_110[1] = 110;

        uint64[] memory seq120_130 = new uint64[](2);
        seq120_130[0] = 120;
        seq120_130[1] = 130;

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq100_110, seq100, seq110);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq100_110, seq100, seq110, "job5", "hash5", 4, "x86_64", 8000, 2000);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seq120_130, seq120, seq130);
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seq120_130, seq120, seq130, "job6", "hash6", 4, "x86_64", 8000, 2000);

        // Level 3: Final merge
        uint64[] memory seqAll = new uint64[](4);
        seqAll[0] = 100;
        seqAll[1] = 110;
        seqAll[2] = 120;
        seqAll[3] = 130;

        bool success = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, seqAll, seq100_110, seq120_130);
        assertTrue(success);

        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, seqAll, seq100_110, seq120_130, "job7", "hash7", 8, "x86_64", 16000, 4000);
        vm.stopPrank();

        // Verify final proof is CALCULATED
        (uint8 finalStatus,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seqAll);
        assertEq(finalStatus, proofManager.PROOF_STATUS_CALCULATED());

        // Verify level 2 proofs are USED
        (uint8 status1,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq100_110);
        (uint8 status2,,,,,,,,) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, seq120_130);
        assertEq(status1, proofManager.PROOF_STATUS_USED());
        assertEq(status2, proofManager.PROOF_STATUS_USED());
    }

    function testSequenceValidation() public {
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory emptySeq = new uint64[](0);

        // Test 1: Empty sequences should revert
        uint64[] memory emptySequences = new uint64[](0);
        vm.expectRevert();
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, emptySequences, emptySeq, emptySeq);

        // Test 2: Sequence below range should revert
        uint64[] memory belowRange = new uint64[](1);
        belowRange[0] = START_SEQUENCE - 1;
        vm.expectRevert();
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, belowRange, emptySeq, emptySeq);

        // Test 3: Sequence above range should revert
        uint64[] memory aboveRange = new uint64[](1);
        aboveRange[0] = END_SEQUENCE + 1;
        vm.expectRevert();
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, aboveRange, emptySeq, emptySeq);

        // Test 4: Valid sequence should succeed
        uint64[] memory validSeq = new uint64[](1);
        validSeq[0] = 150;
        bool success = proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, validSeq, emptySeq, emptySeq);
        assertTrue(success);

        vm.stopPrank();
    }

    function testGetProof() public {
        // Setup and create a proof with all fields populated
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        uint256 submitTime = block.timestamp;
        proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            sequences,
            emptySeq,
            emptySeq,
            "test-job-123",
            "QmTestHash",
            8,
            "arm64",
            16000,
            5000
        );
        vm.stopPrank();

        // Get proof and verify all fields
        (
            uint8 status,
            string memory daHash,
            uint64[] memory seq1,
            uint64[] memory seq2,
            uint16 rejectedCount,
            uint256 timestamp,
            address prover,
            address userAddr,
            string memory jobId
        ) = proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);

        assertEq(status, proofManager.PROOF_STATUS_CALCULATED());
        assertEq(daHash, "QmTestHash");
        assertEq(seq1.length, 0);
        assertEq(seq2.length, 0);
        assertEq(rejectedCount, 0);
        assertEq(timestamp, submitTime);
        assertEq(prover, coordinator);
        assertEq(userAddr, address(0)); // Not set in this test
        assertEq(jobId, "test-job-123");
    }

    function testGetProofStatus() public {
        vm.startPrank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        // Initially, non-existent proof has status 0
        uint8 statusBefore = proofManager.getProofStatus(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(statusBefore, 0);

        // After starting, status is STARTED
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);
        uint8 statusStarted = proofManager.getProofStatus(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(statusStarted, proofManager.PROOF_STATUS_STARTED());

        // After submitting, status is CALCULATED
        proofManager.submitProof(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq, "job1", "hash1", 4, "x86_64", 8000, 1000);
        uint8 statusCalculated = proofManager.getProofStatus(APP_INSTANCE, BLOCK_NUMBER, sequences);
        assertEq(statusCalculated, proofManager.PROOF_STATUS_CALCULATED());

        vm.stopPrank();
    }

    function testGetProofCalculationsRange() public {
        vm.startPrank(coordinator);

        // Create 3 proof calculations
        proofManager.createProofCalculation(APP_INSTANCE, 1, 100, 200);
        proofManager.createProofCalculation(APP_INSTANCE, 2, 201, 300);
        proofManager.createProofCalculation(APP_INSTANCE, 3, 301, 400);

        vm.stopPrank();

        // Query range
        (
            uint64[] memory blockNumbers,
            uint64[] memory startSequences,
            uint64[] memory endSequences,
            string[] memory blockProofs,
            bool[] memory isFinishedFlags
        ) = proofManager.getProofCalculationsRange(APP_INSTANCE, 1, 3);

        assertEq(blockNumbers.length, 3);
        assertEq(blockNumbers[0], 1);
        assertEq(blockNumbers[1], 2);
        assertEq(blockNumbers[2], 3);

        assertEq(startSequences[0], 100);
        assertEq(startSequences[1], 201);
        assertEq(startSequences[2], 301);

        assertEq(endSequences[0], 200);
        assertEq(endSequences[1], 300);
        assertEq(endSequences[2], 400);

        assertEq(bytes(blockProofs[0]).length, 0);
        assertEq(bytes(blockProofs[1]).length, 0);
        assertEq(bytes(blockProofs[2]).length, 0);

        assertFalse(isFinishedFlags[0]);
        assertFalse(isFinishedFlags[1]);
        assertFalse(isFinishedFlags[2]);
    }

    function testAccessControl() public {
        address unauthorized = address(99);

        // Test that non-coordinator cannot create proof calculation
        vm.prank(unauthorized);
        vm.expectRevert();
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        // Setup for other tests
        vm.prank(coordinator);
        proofManager.createProofCalculation(APP_INSTANCE, BLOCK_NUMBER, START_SEQUENCE, END_SEQUENCE);

        uint64[] memory sequences = new uint64[](1);
        sequences[0] = 150;

        uint64[] memory emptySeq = new uint64[](0);

        // Test that non-coordinator cannot start proving
        vm.prank(unauthorized);
        vm.expectRevert();
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        // Coordinator starts proving
        vm.prank(coordinator);
        proofManager.startProving(APP_INSTANCE, BLOCK_NUMBER, sequences, emptySeq, emptySeq);

        // Test that non-coordinator cannot submit proof
        vm.prank(unauthorized);
        vm.expectRevert();
        proofManager.submitProof(
            APP_INSTANCE,
            BLOCK_NUMBER,
            sequences,
            emptySeq,
            emptySeq,
            "job1",
            "hash1",
            4,
            "x86_64",
            8000,
            1000
        );

        // Test that non-coordinator cannot reject proof
        vm.prank(unauthorized);
        vm.expectRevert();
        proofManager.rejectProof(APP_INSTANCE, BLOCK_NUMBER, sequences);

        // View functions should work for anyone
        proofManager.getProofCalculation(APP_INSTANCE, BLOCK_NUMBER);
        proofManager.getProof(APP_INSTANCE, BLOCK_NUMBER, sequences);
        proofManager.getProofStatus(APP_INSTANCE, BLOCK_NUMBER, sequences);
        proofManager.isProofCalculationFinished(APP_INSTANCE, BLOCK_NUMBER);
    }
}
