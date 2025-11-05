// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import {DataTypes} from "../libraries/DataTypes.sol";

/**
 * @title IProofManager
 * @notice Interface for proof management
 * @dev Maps to coordination::prover functionality
 */
interface IProofManager {
    // ============ Write Functions ============

    /**
     * @notice Create a new proof calculation for a block
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param startSequence The starting sequence number
     * @param endSequence The ending sequence number (0 if unknown)
     */
    function createProofCalculation(
        string calldata appInstance,
        uint64 blockNumber,
        uint64 startSequence,
        uint64 endSequence
    ) external;

    /**
     * @notice Start proving a sequence range
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers to prove
     * @param sequence1 Optional first sub-proof sequences (for recursive proof)
     * @param sequence2 Optional second sub-proof sequences (for recursive proof)
     * @return success True if proof was started successfully
     */
    function startProving(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2
    ) external returns (bool success);

    /**
     * @notice Submit a calculated proof
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers proved
     * @param sequence1 Optional first sub-proof sequences used
     * @param sequence2 Optional second sub-proof sequences used
     * @param jobId Associated job ID
     * @param daHash Data availability hash
     * @param cpuCores Number of CPU cores used
     * @param proverArchitecture Prover architecture description
     * @param proverMemory Prover memory in bytes
     * @param cpuTime CPU time used in milliseconds
     * @return isBlockProof True if this completes the full block proof
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
    ) external returns (bool isBlockProof);

    /**
     * @notice Reject a proof as invalid
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers to reject
     */
    function rejectProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external;

    /**
     * @notice Finish a proof calculation with the block proof
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param endSequence The ending sequence number
     * @param blockProof The block proof DA hash
     */
    function finishProofCalculation(
        string calldata appInstance,
        uint64 blockNumber,
        uint64 endSequence,
        string calldata blockProof
    ) external;

    /**
     * @notice Delete a proof calculation after settlement
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     */
    function deleteProofCalculation(
        string calldata appInstance,
        uint64 blockNumber
    ) external;

    // ============ Read Functions ============

    /**
     * @notice Get a proof calculation
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @return blockNum Block number
     * @return startSeq Starting sequence
     * @return endSeq Ending sequence
     * @return blockProof Block proof hash
     * @return isFinished Whether calculation is finished
     */
    function getProofCalculation(
        string calldata appInstance,
        uint64 blockNumber
    ) external view returns (
        uint64 blockNum,
        uint64 startSeq,
        uint64 endSeq,
        string memory blockProof,
        bool isFinished
    );

    /**
     * @notice Get a specific proof
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers
     * @return status Proof status
     * @return daHash Data availability hash
     * @return sequence1 First sub-proof sequences
     * @return sequence2 Second sub-proof sequences
     * @return rejectedCount Number of rejections
     * @return timestamp Proof timestamp
     * @return prover Prover address
     * @return user User address
     * @return jobId Job ID
     */
    function getProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external view returns (
        uint8 status,
        string memory daHash,
        uint64[] memory sequence1,
        uint64[] memory sequence2,
        uint16 rejectedCount,
        uint256 timestamp,
        address prover,
        address user,
        string memory jobId
    );

    /**
     * @notice Get the status of a proof
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers
     * @return status The proof status (0 if not found)
     */
    function getProofStatus(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external view returns (uint8 status);

    /**
     * @notice Check if proof calculation is finished
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @return finished True if finished
     */
    function isProofCalculationFinished(
        string calldata appInstance,
        uint64 blockNumber
    ) external view returns (bool finished);

    /**
     * @notice Get a range of proof calculations
     * @param appInstance The app instance identifier
     * @param fromBlock Starting block number
     * @param toBlock Ending block number
     * @return blockNumbers Array of block numbers
     * @return startSequences Array of start sequences
     * @return endSequences Array of end sequences
     * @return blockProofs Array of block proof hashes
     * @return isFinishedFlags Array of finished flags
     */
    function getProofCalculationsRange(
        string calldata appInstance,
        uint64 fromBlock,
        uint64 toBlock
    ) external view returns (
        uint64[] memory blockNumbers,
        uint64[] memory startSequences,
        uint64[] memory endSequences,
        string[] memory blockProofs,
        bool[] memory isFinishedFlags
    );

    /**
     * @notice Check if a proof can be started
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers
     * @param sequence1 Optional first sub-proof sequences
     * @param sequence2 Optional second sub-proof sequences
     * @return canStart True if proof can be started
     */
    function canStartProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2
    ) external view returns (bool canStart);
}
