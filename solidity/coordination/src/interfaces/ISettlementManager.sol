// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

/**
 * @title ISettlementManager
 * @notice Interface for managing multi-chain settlement tracking
 * @dev Handles settlement state for blocks across different chains
 */
interface ISettlementManager {
    // ============ Write Functions ============

    /**
     * @notice Create a new settlement for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier (e.g., "ethereum", "polygon")
     * @param settlementAddress Optional settlement contract address on the chain
     */
    function createSettlement(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external;

    /**
     * @notice Set or update block settlement transaction details
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number being settled
     * @param txHash The settlement transaction hash (empty if not sent yet)
     * @param txIncluded Whether the transaction is included in a block
     * @param sentAt Timestamp when sent to settlement (0 if not sent)
     * @param settledAt Timestamp when settled (0 if not settled)
     */
    function setBlockSettlementTx(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash,
        bool txIncluded,
        uint64 sentAt,
        uint64 settledAt
    ) external;

    /**
     * @notice Update block settlement tx hash
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number
     * @param txHash The settlement transaction hash
     */
    function updateBlockSettlementTxHash(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash
    ) external;

    /**
     * @notice Update block settlement tx included status
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number
     */
    function updateBlockSettlementTxIncluded(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external;

    /**
     * @notice Set the settlement address for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param settlementAddress The settlement contract address
     */
    function setSettlementAddress(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external;

    /**
     * @notice Set the active settlement job for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param jobId The job ID (0 to clear)
     */
    function setSettlementJob(
        string calldata appInstance,
        string calldata chain,
        uint64 jobId
    ) external;

    /**
     * @notice Purge a block settlement (admin cleanup function)
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number to purge
     * @param reason Reason for purging
     */
    function purgeBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata reason
    ) external;

    // ============ Read Functions ============

    /**
     * @notice Get settlement info for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @return chain The chain identifier
     * @return lastSettledBlockNumber Last block number that was settled
     * @return settlementAddress Settlement contract address
     * @return settlementJob Active settlement job ID (0 if none)
     * @return exists Whether the settlement exists
     */
    function getSettlement(
        string calldata appInstance,
        string calldata chain
    ) external view returns (
        string memory,
        uint64 lastSettledBlockNumber,
        string memory settlementAddress,
        uint64 settlementJob,
        bool exists
    );

    /**
     * @notice Get block settlement details
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number
     * @return blockNumber The block number
     * @return txHash Settlement transaction hash
     * @return txIncluded Whether tx is included in a block
     * @return sentAt Timestamp when sent
     * @return settledAt Timestamp when settled
     * @return exists Whether the block settlement exists
     */
    function getBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external view returns (
        uint64,
        string memory txHash,
        bool txIncluded,
        uint64 sentAt,
        uint64 settledAt,
        bool exists
    );

    /**
     * @notice Get all chains that have settlements for an app instance
     * @param appInstance The app instance ID
     * @return chains Array of chain identifiers
     */
    function getSettlementChains(
        string calldata appInstance
    ) external view returns (string[] memory chains);

    /**
     * @notice Get settlement address for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @return address The settlement contract address
     */
    function getSettlementAddress(
        string calldata appInstance,
        string calldata chain
    ) external view returns (string memory);

    /**
     * @notice Get active settlement job for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @return jobId The active settlement job ID (0 if none)
     */
    function getSettlementJobForChain(
        string calldata appInstance,
        string calldata chain
    ) external view returns (uint64 jobId);

    /**
     * @notice Check if block settlement exists
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number
     * @return exists Whether the block settlement exists
     */
    function hasBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external view returns (bool exists);

    /**
     * @notice Get last settled block number for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @return blockNumber The last settled block number (0 if none)
     */
    function getLastSettledBlock(
        string calldata appInstance,
        string calldata chain
    ) external view returns (uint64 blockNumber);

    /**
     * @notice Check if settlement exists for a chain
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @return exists Whether the settlement exists
     */
    function hasSettlement(
        string calldata appInstance,
        string calldata chain
    ) external view returns (bool exists);
}
