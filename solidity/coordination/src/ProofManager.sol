// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {AccessControl} from "./AccessControl.sol";
import {DataTypes} from "./libraries/DataTypes.sol";
import {IProofManager} from "./interfaces/IProofManager.sol";

/**
 * @title ProofManager
 * @notice Manages proof calculations and submissions
 * @dev Maps to coordination::prover module
 */
contract ProofManager is IProofManager, AccessControl {
    // ============ Constants ============

    // Proof status constants
    uint8 public constant PROOF_STATUS_STARTED = 1;
    uint8 public constant PROOF_STATUS_CALCULATED = 2;
    uint8 public constant PROOF_STATUS_REJECTED = 3;
    uint8 public constant PROOF_STATUS_RESERVED = 4;
    uint8 public constant PROOF_STATUS_USED = 5;

    // Universal timeout for all proof statuses (5 minutes)
    uint256 public constant PROOF_TIMEOUT_MS = 300000;

    // ============ Storage ============

    // App instance => block number => ProofCalculation
    mapping(string => mapping(uint64 => DataTypes.ProofCalculation)) private proofCalculations;

    // App instance => block number => exists flag
    mapping(string => mapping(uint64 => bool)) private proofCalculationExists;

    // App instance => block number => proof count (for queries)
    mapping(string => mapping(uint64 => uint256)) private proofCounts;

    // ============ Modifiers ============

    modifier validAppInstance(string calldata appInstance) {
        require(bytes(appInstance).length > 0, "ProofManager: empty app instance");
        _;
    }

    modifier proofCalculationMustExist(string calldata appInstance, uint64 blockNumber) {
        require(
            proofCalculationExists[appInstance][blockNumber],
            "ProofManager: proof calculation not found"
        );
        _;
    }

    // ============ Write Functions ============

    /**
     * @inheritdoc IProofManager
     */
    function createProofCalculation(
        string calldata appInstance,
        uint64 blockNumber,
        uint64 startSequence,
        uint64 endSequence
    ) external override validAppInstance(appInstance) onlyRole(COORDINATOR_ROLE) {
        require(
            !proofCalculationExists[appInstance][blockNumber],
            "ProofManager: proof calculation already exists"
        );

        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        calc.blockNumber = blockNumber;
        calc.startSequence = startSequence;
        calc.endSequence = endSequence;
        calc.blockProof = "";
        calc.isFinished = blockNumber == 0; // Block 0 is auto-finished

        proofCalculationExists[appInstance][blockNumber] = true;

        emit DataTypes.ProofCalculationCreatedEvent(
            appInstance,
            blockNumber,
            startSequence,
            endSequence,
            block.timestamp
        );
    }

    /**
     * @inheritdoc IProofManager
     */
    function startProving(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2
    ) external override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      onlyRole(COORDINATOR_ROLE)
      returns (bool success) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];

        // Sort and validate all sequences
        uint64[] memory sortedSequences = _sortAndValidateSequences(
            sequences,
            calc.startSequence,
            calc.endSequence
        );

        // Sort and validate sub-sequences
        uint64[] memory sortedSequence1 = sequence1.length > 0
            ? _sortAndValidateSequences(sequence1, calc.startSequence, calc.endSequence)
            : new uint64[](0);

        uint64[] memory sortedSequence2 = sequence2.length > 0
            ? _sortAndValidateSequences(sequence2, calc.startSequence, calc.endSequence)
            : new uint64[](0);

        bytes32 seqHash = _hashSequences(sortedSequences);

        // Check if this is a block proof (full range)
        bool isBlockProof = calc.endSequence > 0 &&
            sortedSequences[0] == calc.startSequence &&
            sortedSequences[sortedSequences.length - 1] == calc.endSequence;

        // Check if proof already exists
        DataTypes.Proof storage existingProof = calc.proofs[seqHash];
        if (existingProof.timestamp > 0) {
            // Proof exists - check if it can be restarted
            bool isTimedOut = block.timestamp > existingProof.timestamp + (PROOF_TIMEOUT_MS / 1000);

            if (existingProof.status != PROOF_STATUS_REJECTED && !isTimedOut) {
                return false; // Cannot restart
            }

            // Can restart - preserve rejection count
            uint16 rejectedCount = existingProof.rejectedCount;
            uint64[] memory oldSeq1 = existingProof.sequence1;
            uint64[] memory oldSeq2 = existingProof.sequence2;

            // Reset proof
            existingProof.status = PROOF_STATUS_STARTED;
            existingProof.daHash = "";
            existingProof.sequence1 = sortedSequence1;
            existingProof.sequence2 = sortedSequence2;
            existingProof.timestamp = block.timestamp;
            existingProof.prover = msg.sender;
            existingProof.user = address(0);
            existingProof.jobId = "";
            existingProof.rejectedCount = rejectedCount;

            // Return old sub-proofs
            _returnSubProofs(appInstance, blockNumber, oldSeq1, oldSeq2);
        } else {
            // New proof - check if sub-proofs can be reserved before creating
            if (sortedSequence1.length > 0) {
                if (!_canReserveProof(calc, sortedSequence1, isBlockProof)) {
                    return false;
                }
            }
            if (sortedSequence2.length > 0) {
                if (!_canReserveProof(calc, sortedSequence2, isBlockProof)) {
                    return false;
                }
            }

            // Create new proof
            existingProof.status = PROOF_STATUS_STARTED;
            existingProof.daHash = "";
            existingProof.sequence1 = sortedSequence1;
            existingProof.sequence2 = sortedSequence2;
            existingProof.rejectedCount = 0;
            existingProof.timestamp = block.timestamp;
            existingProof.prover = msg.sender;
            existingProof.user = address(0);
            existingProof.jobId = "";

            proofCounts[appInstance][blockNumber]++;
        }

        // Reserve sub-proofs
        if (sortedSequence1.length > 0) {
            _reserveProof(calc, sortedSequence1, isBlockProof);
            emit DataTypes.ProofReservedEvent(
                appInstance,
                blockNumber,
                sortedSequence1,
                calc.proofs[_hashSequences(sortedSequence1)].daHash,
                block.timestamp,
                msg.sender
            );
        }
        if (sortedSequence2.length > 0) {
            _reserveProof(calc, sortedSequence2, isBlockProof);
            emit DataTypes.ProofReservedEvent(
                appInstance,
                blockNumber,
                sortedSequence2,
                calc.proofs[_hashSequences(sortedSequence2)].daHash,
                block.timestamp,
                msg.sender
            );
        }

        emit DataTypes.ProofStartedEvent(
            appInstance,
            blockNumber,
            sortedSequences,
            block.timestamp,
            msg.sender
        );

        return true;
    }

    /**
     * @inheritdoc IProofManager
     */
    function submitProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2,
        string calldata jobId,
        string calldata daHash,
        uint8 cpuCores,
        string calldata proverArchitecture,
        uint64 proverMemory,
        uint64 cpuTime
    ) external override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      onlyRole(COORDINATOR_ROLE)
      returns (bool isBlockProof) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];

        // Sort and validate sequences
        uint64[] memory sortedSequences = _sortAndValidateSequences(
            sequences,
            calc.startSequence,
            calc.endSequence
        );

        uint64[] memory sortedSequence1 = sequence1.length > 0
            ? _sortAndValidateSequences(sequence1, calc.startSequence, calc.endSequence)
            : new uint64[](0);

        uint64[] memory sortedSequence2 = sequence2.length > 0
            ? _sortAndValidateSequences(sequence2, calc.startSequence, calc.endSequence)
            : new uint64[](0);

        bytes32 seqHash = _hashSequences(sortedSequences);

        // Update or create proof
        DataTypes.Proof storage proof = calc.proofs[seqHash];
        uint16 rejectedCount = proof.rejectedCount;

        if (proof.timestamp == 0) {
            proofCounts[appInstance][blockNumber]++;
        }

        proof.status = PROOF_STATUS_CALCULATED;
        proof.daHash = daHash;
        proof.sequence1 = sortedSequence1;
        proof.sequence2 = sortedSequence2;
        proof.timestamp = block.timestamp;
        proof.prover = msg.sender;
        proof.user = address(0);
        proof.jobId = jobId;
        proof.rejectedCount = rejectedCount;

        emit DataTypes.ProofSubmittedEvent(
            appInstance,
            blockNumber,
            sortedSequences,
            daHash,
            block.timestamp,
            msg.sender,
            cpuTime,
            cpuCores,
            proverMemory,
            proverArchitecture
        );

        // Use sub-proofs
        if (sortedSequence1.length > 0) {
            _useProof(calc, sortedSequence1);
            emit DataTypes.ProofUsedEvent(
                appInstance,
                blockNumber,
                sortedSequence1,
                calc.proofs[_hashSequences(sortedSequence1)].daHash,
                block.timestamp,
                msg.sender
            );
        }
        if (sortedSequence2.length > 0) {
            _useProof(calc, sortedSequence2);
            emit DataTypes.ProofUsedEvent(
                appInstance,
                blockNumber,
                sortedSequence2,
                calc.proofs[_hashSequences(sortedSequence2)].daHash,
                block.timestamp,
                msg.sender
            );
        }

        // Check if this completes the block proof
        if (calc.endSequence > 0 &&
            sortedSequences[0] == calc.startSequence &&
            sortedSequences[sortedSequences.length - 1] == calc.endSequence
        ) {
            _finishProofCalculationInternal(calc, appInstance, blockNumber, calc.endSequence, daHash);
            return true;
        }

        return false;
    }

    /**
     * @inheritdoc IProofManager
     */
    function rejectProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      onlyRole(COORDINATOR_ROLE) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];

        uint64[] memory sortedSequences = _sortAndValidateSequences(
            sequences,
            calc.startSequence,
            calc.endSequence
        );

        bytes32 seqHash = _hashSequences(sortedSequences);
        DataTypes.Proof storage proof = calc.proofs[seqHash];

        require(proof.timestamp > 0, "ProofManager: proof not found");

        proof.status = PROOF_STATUS_REJECTED;
        proof.rejectedCount++;

        // Return sub-proofs
        _returnSubProofs(appInstance, blockNumber, proof.sequence1, proof.sequence2);

        emit DataTypes.ProofRejectedEvent(
            appInstance,
            blockNumber,
            sortedSequences,
            block.timestamp,
            msg.sender
        );
    }

    /**
     * @inheritdoc IProofManager
     */
    function finishProofCalculation(
        string calldata appInstance,
        uint64 blockNumber,
        uint64 endSequence,
        string calldata blockProof
    ) external override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      onlyRole(COORDINATOR_ROLE) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        _finishProofCalculationInternal(calc, appInstance, blockNumber, endSequence, blockProof);
    }

    /**
     * @inheritdoc IProofManager
     */
    function deleteProofCalculation(
        string calldata appInstance,
        uint64 blockNumber
    ) external override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      onlyRole(COORDINATOR_ROLE) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];

        emit DataTypes.ProofCalculationFinishedEvent(
            appInstance,
            blockNumber,
            calc.startSequence,
            calc.endSequence,
            calc.blockProof,
            calc.isFinished,
            proofCounts[appInstance][blockNumber],
            block.timestamp
        );

        // Clean up storage
        delete proofCalculations[appInstance][blockNumber];
        delete proofCalculationExists[appInstance][blockNumber];
        delete proofCounts[appInstance][blockNumber];
    }

    // ============ Read Functions ============

    /**
     * @inheritdoc IProofManager
     */
    function getProofCalculation(
        string calldata appInstance,
        uint64 blockNumber
    ) external view override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      returns (
        uint64 blockNum,
        uint64 startSeq,
        uint64 endSeq,
        string memory blockProof,
        bool isFinished
    ) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        return (
            calc.blockNumber,
            calc.startSequence,
            calc.endSequence,
            calc.blockProof,
            calc.isFinished
        );
    }

    /**
     * @inheritdoc IProofManager
     */
    function getProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external view override validAppInstance(appInstance)
      proofCalculationMustExist(appInstance, blockNumber)
      returns (
        uint8 status,
        string memory daHash,
        uint64[] memory sequence1,
        uint64[] memory sequence2,
        uint16 rejectedCount,
        uint256 timestamp,
        address prover,
        address user,
        string memory jobId
    ) {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        bytes32 seqHash = _hashSequences(sequences);
        DataTypes.Proof storage proof = calc.proofs[seqHash];

        return (
            proof.status,
            proof.daHash,
            proof.sequence1,
            proof.sequence2,
            proof.rejectedCount,
            proof.timestamp,
            proof.prover,
            proof.user,
            proof.jobId
        );
    }

    /**
     * @inheritdoc IProofManager
     */
    function getProofStatus(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external view override validAppInstance(appInstance) returns (uint8 status) {
        if (!proofCalculationExists[appInstance][blockNumber]) {
            return 0;
        }

        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        bytes32 seqHash = _hashSequences(sequences);
        return calc.proofs[seqHash].status;
    }

    /**
     * @inheritdoc IProofManager
     */
    function isProofCalculationFinished(
        string calldata appInstance,
        uint64 blockNumber
    ) external view override validAppInstance(appInstance) returns (bool finished) {
        if (!proofCalculationExists[appInstance][blockNumber]) {
            return false;
        }
        return proofCalculations[appInstance][blockNumber].isFinished;
    }

    /**
     * @inheritdoc IProofManager
     */
    function getProofCalculationsRange(
        string calldata appInstance,
        uint64 fromBlock,
        uint64 toBlock
    ) external view override validAppInstance(appInstance) returns (
        uint64[] memory blockNumbers,
        uint64[] memory startSequences,
        uint64[] memory endSequences,
        string[] memory blockProofs,
        bool[] memory isFinishedFlags
    ) {
        require(toBlock >= fromBlock, "ProofManager: invalid range");
        uint256 count = toBlock - fromBlock + 1;

        blockNumbers = new uint64[](count);
        startSequences = new uint64[](count);
        endSequences = new uint64[](count);
        blockProofs = new string[](count);
        isFinishedFlags = new bool[](count);

        for (uint64 i = 0; i < count; i++) {
            uint64 blockNum = fromBlock + i;
            if (proofCalculationExists[appInstance][blockNum]) {
                DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNum];
                blockNumbers[i] = calc.blockNumber;
                startSequences[i] = calc.startSequence;
                endSequences[i] = calc.endSequence;
                blockProofs[i] = calc.blockProof;
                isFinishedFlags[i] = calc.isFinished;
            }
        }

        return (blockNumbers, startSequences, endSequences, blockProofs, isFinishedFlags);
    }

    /**
     * @inheritdoc IProofManager
     */
    function canStartProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2
    ) external view override validAppInstance(appInstance) returns (bool canStart) {
        if (!proofCalculationExists[appInstance][blockNumber]) {
            return false;
        }

        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];
        bytes32 seqHash = _hashSequences(sequences);
        DataTypes.Proof storage existingProof = calc.proofs[seqHash];

        // Check if proof exists and can be restarted
        if (existingProof.timestamp > 0) {
            bool isTimedOut = block.timestamp > existingProof.timestamp + (PROOF_TIMEOUT_MS / 1000);
            if (existingProof.status != PROOF_STATUS_REJECTED && !isTimedOut) {
                return false;
            }
        }

        // Check if this is a block proof
        bool isBlockProof = calc.endSequence > 0 &&
            sequences.length > 0 &&
            sequences[0] == calc.startSequence &&
            sequences[sequences.length - 1] == calc.endSequence;

        // Check if sub-proofs can be reserved
        if (sequence1.length > 0) {
            if (!_canReserveProof(calc, sequence1, isBlockProof)) {
                return false;
            }
        }
        if (sequence2.length > 0) {
            if (!_canReserveProof(calc, sequence2, isBlockProof)) {
                return false;
            }
        }

        return true;
    }

    // ============ Internal Functions ============

    /**
     * @dev Sort and validate sequence array
     */
    function _sortAndValidateSequences(
        uint64[] calldata sequences,
        uint64 startSeq,
        uint64 endSeq
    ) internal pure returns (uint64[] memory sorted) {
        require(sequences.length > 0, "ProofManager: empty sequences");

        // Copy to memory
        sorted = new uint64[](sequences.length);
        for (uint256 i = 0; i < sequences.length; i++) {
            sorted[i] = sequences[i];
        }

        // Bubble sort
        for (uint256 i = 0; i < sorted.length; i++) {
            for (uint256 j = 0; j < sorted.length - i - 1; j++) {
                if (sorted[j] > sorted[j + 1]) {
                    uint64 temp = sorted[j];
                    sorted[j] = sorted[j + 1];
                    sorted[j + 1] = temp;
                }
            }
        }

        // Validate range
        require(sorted[0] >= startSeq, "ProofManager: sequence below start");
        if (endSeq > 0) {
            require(sorted[sorted.length - 1] <= endSeq, "ProofManager: sequence above end");
        }

        return sorted;
    }

    /**
     * @dev Hash sequence array for mapping key
     */
    function _hashSequences(uint64[] memory sequences) internal pure returns (bytes32) {
        return keccak256(abi.encode(sequences));
    }

    /**
     * @dev Check if a proof can be reserved
     */
    function _canReserveProof(
        DataTypes.ProofCalculation storage calc,
        uint64[] memory sequences,
        bool isBlockProof
    ) internal view returns (bool) {
        if (sequences.length == 0) return true;

        bytes32 seqHash = _hashSequences(sequences);
        DataTypes.Proof storage proof = calc.proofs[seqHash];

        if (proof.timestamp == 0) return false; // Proof doesn't exist

        uint256 currentTime = block.timestamp;
        bool isTimedOut = currentTime > proof.timestamp + (PROOF_TIMEOUT_MS / 1000);

        return proof.status == PROOF_STATUS_CALCULATED ||
               isTimedOut ||
               (isBlockProof && (proof.status == PROOF_STATUS_USED || proof.status == PROOF_STATUS_RESERVED));
    }

    /**
     * @dev Reserve a proof for composition
     */
    function _reserveProof(
        DataTypes.ProofCalculation storage calc,
        uint64[] memory sequences,
        bool isBlockProof
    ) internal {
        if (sequences.length == 0) return;

        bytes32 seqHash = _hashSequences(sequences);
        DataTypes.Proof storage proof = calc.proofs[seqHash];

        proof.status = PROOF_STATUS_RESERVED;
        proof.timestamp = block.timestamp;
        proof.user = msg.sender;
    }

    /**
     * @dev Mark a proof as used
     */
    function _useProof(
        DataTypes.ProofCalculation storage calc,
        uint64[] memory sequences
    ) internal {
        if (sequences.length == 0) return;

        bytes32 seqHash = _hashSequences(sequences);
        DataTypes.Proof storage proof = calc.proofs[seqHash];

        proof.status = PROOF_STATUS_USED;
        proof.timestamp = block.timestamp;
        proof.user = msg.sender;
    }

    /**
     * @dev Return sub-proofs to calculated state
     */
    function _returnSubProofs(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] memory sequence1,
        uint64[] memory sequence2
    ) internal {
        DataTypes.ProofCalculation storage calc = proofCalculations[appInstance][blockNumber];

        if (sequence1.length > 0) {
            bytes32 seq1Hash = _hashSequences(sequence1);
            if (calc.proofs[seq1Hash].timestamp > 0) {
                calc.proofs[seq1Hash].status = PROOF_STATUS_CALCULATED;
                calc.proofs[seq1Hash].timestamp = block.timestamp;
                calc.proofs[seq1Hash].user = address(0);

                emit DataTypes.ProofReturnedEvent(
                    appInstance,
                    blockNumber,
                    sequence1,
                    block.timestamp
                );
            }
        }

        if (sequence2.length > 0) {
            bytes32 seq2Hash = _hashSequences(sequence2);
            if (calc.proofs[seq2Hash].timestamp > 0) {
                calc.proofs[seq2Hash].status = PROOF_STATUS_CALCULATED;
                calc.proofs[seq2Hash].timestamp = block.timestamp;
                calc.proofs[seq2Hash].user = address(0);

                emit DataTypes.ProofReturnedEvent(
                    appInstance,
                    blockNumber,
                    sequence2,
                    block.timestamp
                );
            }
        }
    }

    /**
     * @dev Internal function to finish proof calculation
     */
    function _finishProofCalculationInternal(
        DataTypes.ProofCalculation storage calc,
        string calldata appInstance,
        uint64 blockNumber,
        uint64 endSequence,
        string memory blockProof
    ) internal {
        calc.blockProof = blockProof;
        calc.endSequence = endSequence;
        calc.isFinished = true;

        emit DataTypes.BlockProofEvent(
            appInstance,
            blockNumber,
            calc.startSequence,
            endSequence,
            blockProof,
            block.timestamp
        );
    }
}
