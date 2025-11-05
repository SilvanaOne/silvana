// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./IJobManager.sol";
import "./IAppInstanceManager.sol";
import "./IAccessControl.sol";

/**
 * @title ICoordination
 * @notice Main interface for the Silvana Coordination Layer
 * @dev Aggregates all coordination functionality interfaces
 */
interface ICoordination is IAccessControl {
    // ============ Component Getters ============

    /**
     * @notice Get the JobManager contract address
     * @return manager The JobManager contract
     */
    function jobManager() external view returns (IJobManager manager);

    /**
     * @notice Get the AppInstanceManager contract address
     * @return manager The AppInstanceManager contract
     */
    function appInstanceManager() external view returns (IAppInstanceManager manager);

    // ============ Initialization ============

    /**
     * @notice Initialize the coordination contract
     * @param _admin The admin address
     * @param _jobManager The JobManager contract address
     * @param _appInstanceManager The AppInstanceManager contract address
     * @param _proofManager The ProofManager contract address
     * @param _settlementManager The SettlementManager contract address
     * @param _storageManager The StorageManager contract address
     */
    function initialize(
        address _admin,
        address _jobManager,
        address _appInstanceManager,
        address _proofManager,
        address _settlementManager,
        address _storageManager
    ) external;

    // ============ Emergency Functions ============

    /**
     * @notice Pause all operations
     * @dev Only callable by PAUSER_ROLE
     */
    function pause() external;

    /**
     * @notice Unpause all operations
     * @dev Only callable by PAUSER_ROLE
     */
    function unpause() external;

    /**
     * @notice Check if contract is paused
     * @return paused True if paused
     */
    function paused() external view returns (bool paused);

    // ============ Multicall Support ============

    /**
     * @notice Execute multiple calls in a single transaction
     * @param targets Array of target addresses
     * @param data Array of call data
     * @return results Array of call results
     */
    function multicall(
        address[] calldata targets,
        bytes[] calldata data
    ) external returns (bytes[] memory results);

    /**
     * @notice Get the current multicall nonce
     * @return nonce The current nonce
     */
    function multicallNonce() external view returns (uint256 nonce);

    // ============ Component Updates ============

    /**
     * @notice Update the JobManager contract address
     * @param _jobManager The new JobManager contract address
     */
    function setJobManager(address _jobManager) external;

    /**
     * @notice Update the AppInstanceManager contract address
     * @param _appInstanceManager The new AppInstanceManager contract address
     */
    function setAppInstanceManager(address _appInstanceManager) external;

    // ============ Settlement Functions ============

    /**
     * @notice Create a new settlement for an app instance on a specific chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier (e.g., "ethereum", "polygon")
     * @param settlementAddress The settlement contract address on the target chain
     */
    function createSettlement(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external;

    /**
     * @notice Record a block settlement transaction
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param blockNumber The block number being settled
     * @param txHash The transaction hash on the settlement chain
     * @param txIncluded Whether the transaction has been included in a block
     * @param sentAt Timestamp when transaction was sent
     * @param settledAt Timestamp when block was settled (0 if pending)
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
     * @notice Update the transaction hash for a block settlement
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param blockNumber The block number
     * @param txHash The new transaction hash
     */
    function updateBlockSettlementTxHash(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash
    ) external;

    /**
     * @notice Update the transaction inclusion status for a block settlement
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param blockNumber The block number
     */
    function updateBlockSettlementTxIncluded(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external;

    /**
     * @notice Update the settlement address for a chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param settlementAddress The new settlement contract address
     */
    function setSettlementAddress(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external;

    /**
     * @notice Set the settlement job ID for a chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param jobId The settlement job identifier
     */
    function setSettlementJob(
        string calldata appInstance,
        string calldata chain,
        uint64 jobId
    ) external;

    /**
     * @notice Get settlement information for a specific chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @return chainName The chain identifier
     * @return lastSettledBlockNumber The last settled block number
     * @return settlementAddress The settlement contract address
     * @return settlementJob The active settlement job ID
     * @return exists Whether the settlement exists
     */
    function getSettlement(
        string calldata appInstance,
        string calldata chain
    ) external view returns (
        string memory chainName,
        uint64 lastSettledBlockNumber,
        string memory settlementAddress,
        uint64 settlementJob,
        bool exists
    );

    /**
     * @notice Get block settlement information
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param blockNumber The block number
     * @return block The block number
     * @return settlementTxHash The settlement transaction hash
     * @return settlementTxIncludedInBlock Whether transaction is included
     * @return sentToSettlementAt When transaction was sent
     * @return settledAt When block was settled
     * @return exists Whether the block settlement exists
     */
    function getBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external view returns (
        uint64 block,
        string memory settlementTxHash,
        bool settlementTxIncludedInBlock,
        uint64 sentToSettlementAt,
        uint64 settledAt,
        bool exists
    );

    /**
     * @notice Get all chains with settlements for an app instance
     * @param appInstance The app instance identifier
     * @return chains Array of chain identifiers
     */
    function getSettlementChains(
        string calldata appInstance
    ) external view returns (string[] memory chains);

    /**
     * @notice Get the settlement address for a specific chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @return settlementAddress The settlement contract address
     */
    function getSettlementAddress(
        string calldata appInstance,
        string calldata chain
    ) external view returns (string memory settlementAddress);

    /**
     * @notice Get the active settlement job ID for a chain
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @return jobId The settlement job identifier
     */
    function getSettlementJobForChain(
        string calldata appInstance,
        string calldata chain
    ) external view returns (uint64 jobId);

    /**
     * @notice Purge a block settlement record (admin only)
     * @param appInstance The app instance identifier
     * @param chain The blockchain identifier
     * @param blockNumber The block number to purge
     * @param reason Reason for purging
     */
    function purgeBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata reason
    ) external;

    // ============ Storage Functions ============

    /**
     * @notice Set a string value in KV storage
     * @param appInstance The app instance identifier
     * @param key The storage key
     * @param value The string value to store
     */
    function setKvString(
        string calldata appInstance,
        string calldata key,
        string calldata value
    ) external;

    /**
     * @notice Get a string value from KV storage
     * @param appInstance The app instance identifier
     * @param key The storage key
     * @return value The stored string value
     * @return exists Whether the key exists
     */
    function getKvString(
        string calldata appInstance,
        string calldata key
    ) external view returns (string memory value, bool exists);

    /**
     * @notice Get all string key-value pairs
     * @param appInstance The app instance identifier
     * @return keys Array of keys
     * @return values Array of values
     */
    function getAllKvString(
        string calldata appInstance
    ) external view returns (string[] memory keys, string[] memory values);

    /**
     * @notice List all string keys
     * @param appInstance The app instance identifier
     * @return keys Array of keys
     */
    function listKvStringKeys(
        string calldata appInstance
    ) external view returns (string[] memory keys);

    /**
     * @notice Delete a string key-value pair
     * @param appInstance The app instance identifier
     * @param key The storage key to delete
     */
    function deleteKvString(
        string calldata appInstance,
        string calldata key
    ) external;

    /**
     * @notice Set a binary value in KV storage
     * @param appInstance The app instance identifier
     * @param key The storage key (arbitrary bytes)
     * @param value The binary value to store
     */
    function setKvBinary(
        string calldata appInstance,
        bytes calldata key,
        bytes calldata value
    ) external;

    /**
     * @notice Get a binary value from KV storage
     * @param appInstance The app instance identifier
     * @param key The storage key (arbitrary bytes)
     * @return value The stored binary value
     * @return exists Whether the key exists
     */
    function getKvBinary(
        string calldata appInstance,
        bytes calldata key
    ) external view returns (bytes memory value, bool exists);

    /**
     * @notice List all binary keys
     * @param appInstance The app instance identifier
     * @return keys Array of binary keys
     */
    function listKvBinaryKeys(
        string calldata appInstance
    ) external view returns (bytes[] memory keys);

    /**
     * @notice Delete a binary key-value pair
     * @param appInstance The app instance identifier
     * @param key The storage key to delete (arbitrary bytes)
     */
    function deleteKvBinary(
        string calldata appInstance,
        bytes calldata key
    ) external;

    /**
     * @notice Set metadata for an app instance
     * @param appInstance The app instance identifier
     * @param key The metadata key
     * @param value The metadata value
     */
    function setMetadata(
        string calldata appInstance,
        string calldata key,
        string calldata value
    ) external;

    /**
     * @notice Get metadata for an app instance
     * @param appInstance The app instance identifier
     * @param key The metadata key
     * @return value The metadata value
     * @return exists Whether the key exists
     */
    function getMetadata(
        string calldata appInstance,
        string calldata key
    ) external view returns (string memory value, bool exists);

    /**
     * @notice Delete metadata for an app instance
     * @param appInstance The app instance identifier
     * @param key The metadata key to delete
     */
    function deleteMetadata(
        string calldata appInstance,
        string calldata key
    ) external;

    // ============ Events ============

    /**
     * @notice Emitted when the contract is initialized
     */
    event Initialized(address admin, address jobManager, address appInstanceManager);

    /**
     * @notice Emitted when the contract is paused
     */
    event Paused(address account);

    /**
     * @notice Emitted when the contract is unpaused
     */
    event Unpaused(address account);

    /**
     * @notice Emitted when a component is updated
     */
    event ComponentUpdated(string componentType, address oldAddress, address newAddress);

    /**
     * @notice Emitted when a multicall is executed
     */
    event MulticallExecuted(uint256 nonce, address[] targets, uint256 timestamp);
}