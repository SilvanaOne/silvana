// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/ISettlementManager.sol";
import "./libraries/DataTypes.sol";
import "./AccessControl.sol";

/**
 * @title SettlementManager
 * @notice Manages multi-chain settlement tracking for app instances
 * @dev Tracks settlement state across multiple chains with automatic cleanup
 */
contract SettlementManager is ISettlementManager, AccessControl {
    // ============ Storage ============

    // App instance => chain => Settlement
    mapping(string => mapping(string => DataTypes.Settlement)) private settlements;

    // App instance => chain => block number => BlockSettlement
    mapping(string => mapping(string => mapping(uint64 => DataTypes.BlockSettlement))) private blockSettlements;

    // App instance => array of chains
    mapping(string => string[]) private appChains;

    // App instance => chain => exists flag
    mapping(string => mapping(string => bool)) private settlementExists;

    // ============ Modifiers ============

    modifier settlementMustExist(string calldata appInstance, string calldata chain) {
        if (!settlementExists[appInstance][chain]) {
            revert DataTypes.SettlementNotFound(appInstance, chain);
        }
        _;
    }

    // ============ Constructor ============

    constructor() {}

    // ============ External Functions - Write ============

    /**
     * @inheritdoc ISettlementManager
     */
    function createSettlement(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(bytes(appInstance).length > 0, "SettlementManager: empty app instance");
        require(bytes(chain).length > 0, "SettlementManager: empty chain");

        // Check if settlement already exists
        if (settlementExists[appInstance][chain]) {
            return; // Already exists, no-op
        }

        // Create settlement
        settlements[appInstance][chain] = DataTypes.Settlement({
            chain: chain,
            lastSettledBlockNumber: 0,
            settlementAddress: settlementAddress,
            settlementJob: 0,
            exists: true
        });

        // Add chain to app chains list
        appChains[appInstance].push(chain);
        settlementExists[appInstance][chain] = true;

        emit DataTypes.SettlementCreatedEvent(
            appInstance,
            chain,
            settlementAddress,
            block.timestamp
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function setBlockSettlementTx(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash,
        bool txIncluded,
        uint64 sentAt,
        uint64 settledAt
    ) external override onlyRole(COORDINATOR_ROLE) settlementMustExist(appInstance, chain) {
        DataTypes.Settlement storage settlement = settlements[appInstance][chain];

        // Create or update block settlement
        DataTypes.BlockSettlement storage blockSettlement = blockSettlements[appInstance][chain][blockNumber];
        blockSettlement.blockNumber = blockNumber;
        blockSettlement.settlementTxHash = txHash;
        blockSettlement.settlementTxIncludedInBlock = txIncluded;
        blockSettlement.sentToSettlementAt = sentAt;
        blockSettlement.settledAt = settledAt;
        blockSettlement.exists = true;

        // If transaction is included, handle settlement logic
        if (txIncluded) {
            emit DataTypes.BlockSettledEvent(
                appInstance,
                chain,
                blockNumber,
                txHash,
                sentAt,
                settledAt,
                block.timestamp
            );

            // Process sequential settlement and cleanup
            _processSequentialSettlement(appInstance, chain, blockNumber);
        }

        emit DataTypes.BlockSettlementTransactionEvent(
            appInstance,
            chain,
            blockNumber,
            txHash,
            txIncluded,
            sentAt,
            settledAt,
            block.timestamp
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function updateBlockSettlementTxHash(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash
    ) external override onlyRole(COORDINATOR_ROLE) settlementMustExist(appInstance, chain) {
        DataTypes.BlockSettlement storage blockSettlement = blockSettlements[appInstance][chain][blockNumber];

        if (!blockSettlement.exists) {
            // Create new block settlement if it doesn't exist
            blockSettlement.blockNumber = blockNumber;
            blockSettlement.exists = true;
        }

        blockSettlement.settlementTxHash = txHash;

        emit DataTypes.BlockSettlementTransactionEvent(
            appInstance,
            chain,
            blockNumber,
            txHash,
            blockSettlement.settlementTxIncludedInBlock,
            blockSettlement.sentToSettlementAt,
            blockSettlement.settledAt,
            block.timestamp
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function updateBlockSettlementTxIncluded(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external override onlyRole(COORDINATOR_ROLE) settlementMustExist(appInstance, chain) {
        DataTypes.BlockSettlement storage blockSettlement = blockSettlements[appInstance][chain][blockNumber];

        if (!blockSettlement.exists) {
            revert DataTypes.BlockSettlementNotFound(appInstance, chain, blockNumber);
        }

        blockSettlement.settlementTxIncludedInBlock = true;
        blockSettlement.settledAt = uint64(block.timestamp);

        emit DataTypes.BlockSettledEvent(
            appInstance,
            chain,
            blockNumber,
            blockSettlement.settlementTxHash,
            blockSettlement.sentToSettlementAt,
            blockSettlement.settledAt,
            block.timestamp
        );

        // Process sequential settlement and cleanup
        _processSequentialSettlement(appInstance, chain, blockNumber);

        emit DataTypes.BlockSettlementTransactionEvent(
            appInstance,
            chain,
            blockNumber,
            blockSettlement.settlementTxHash,
            true,
            blockSettlement.sentToSettlementAt,
            blockSettlement.settledAt,
            block.timestamp
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function setSettlementAddress(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external override onlyRole(COORDINATOR_ROLE) settlementMustExist(appInstance, chain) {
        settlements[appInstance][chain].settlementAddress = settlementAddress;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function setSettlementJob(
        string calldata appInstance,
        string calldata chain,
        uint64 jobId
    ) external override onlyRole(COORDINATOR_ROLE) settlementMustExist(appInstance, chain) {
        settlements[appInstance][chain].settlementJob = jobId;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function purgeBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata reason
    ) external override onlyRole(ADMIN_ROLE) settlementMustExist(appInstance, chain) {
        DataTypes.BlockSettlement storage blockSettlement = blockSettlements[appInstance][chain][blockNumber];

        if (!blockSettlement.exists) {
            revert DataTypes.BlockSettlementNotFound(appInstance, chain, blockNumber);
        }

        // Delete block settlement
        delete blockSettlements[appInstance][chain][blockNumber];

        emit DataTypes.BlockSettlementPurgedEvent(
            appInstance,
            chain,
            blockNumber,
            reason,
            block.timestamp
        );
    }

    // ============ External Functions - Read ============

    /**
     * @inheritdoc ISettlementManager
     */
    function getSettlement(
        string calldata appInstance,
        string calldata chain
    ) external view override returns (
        string memory,
        uint64 lastSettledBlockNumber,
        string memory settlementAddress,
        uint64 settlementJob,
        bool exists
    ) {
        DataTypes.Settlement storage settlement = settlements[appInstance][chain];
        return (
            settlement.chain,
            settlement.lastSettledBlockNumber,
            settlement.settlementAddress,
            settlement.settlementJob,
            settlement.exists
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function getBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external view override returns (
        uint64,
        string memory txHash,
        bool txIncluded,
        uint64 sentAt,
        uint64 settledAt,
        bool exists
    ) {
        DataTypes.BlockSettlement storage blockSettlement = blockSettlements[appInstance][chain][blockNumber];
        return (
            blockSettlement.blockNumber,
            blockSettlement.settlementTxHash,
            blockSettlement.settlementTxIncludedInBlock,
            blockSettlement.sentToSettlementAt,
            blockSettlement.settledAt,
            blockSettlement.exists
        );
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function getSettlementChains(
        string calldata appInstance
    ) external view override returns (string[] memory chains) {
        return appChains[appInstance];
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function getSettlementAddress(
        string calldata appInstance,
        string calldata chain
    ) external view override returns (string memory) {
        return settlements[appInstance][chain].settlementAddress;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function getSettlementJobForChain(
        string calldata appInstance,
        string calldata chain
    ) external view override returns (uint64 jobId) {
        return settlements[appInstance][chain].settlementJob;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function hasBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external view override returns (bool exists) {
        return blockSettlements[appInstance][chain][blockNumber].exists;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function getLastSettledBlock(
        string calldata appInstance,
        string calldata chain
    ) external view override returns (uint64 blockNumber) {
        return settlements[appInstance][chain].lastSettledBlockNumber;
    }

    /**
     * @inheritdoc ISettlementManager
     */
    function hasSettlement(
        string calldata appInstance,
        string calldata chain
    ) external view override returns (bool exists) {
        return settlementExists[appInstance][chain];
    }

    // ============ Internal Functions ============

    /**
     * @notice Process sequential settlement and cleanup old blocks
     * @dev Advances lastSettledBlockNumber and purges settled blocks
     * @param appInstance The app instance ID
     * @param chain The chain identifier
     * @param blockNumber The block number that was just settled
     */
    function _processSequentialSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) internal {
        DataTypes.Settlement storage settlement = settlements[appInstance][chain];
        uint64 lastSettled = settlement.lastSettledBlockNumber;

        // Process sequential settlement from lastSettled+1 to blockNumber
        while (lastSettled < blockNumber) {
            uint64 nextBlock = lastSettled + 1;
            DataTypes.BlockSettlement storage nextBlockSettlement = blockSettlements[appInstance][chain][nextBlock];

            // Check if next block is settled
            if (nextBlockSettlement.exists && nextBlockSettlement.settlementTxIncludedInBlock) {
                // Clean up the previous block if it exists (save gas by deleting old data)
                if (lastSettled > 0) {
                    DataTypes.BlockSettlement storage prevBlockSettlement = blockSettlements[appInstance][chain][lastSettled];
                    if (prevBlockSettlement.exists) {
                        delete blockSettlements[appInstance][chain][lastSettled];

                        emit DataTypes.BlockSettlementPurgedEvent(
                            appInstance,
                            chain,
                            lastSettled,
                            "Block already settled and finalized",
                            block.timestamp
                        );
                    }
                }

                lastSettled = nextBlock;
            } else {
                // Next block not settled yet, stop processing
                break;
            }
        }

        // Update last settled block number
        settlement.lastSettledBlockNumber = lastSettled;
    }
}
