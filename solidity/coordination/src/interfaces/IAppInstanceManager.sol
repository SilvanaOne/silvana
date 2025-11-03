// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "../libraries/DataTypes.sol";

/**
 * @title IAppInstanceManager
 * @notice Interface for app instance management operations
 * @dev Defines the interface for the AppInstanceManager contract
 */
interface IAppInstanceManager {
    // ============ Write Functions ============

    /**
     * @notice Create a new app instance
     * @param instanceId The unique identifier for the app instance (e.g., Ed25519 public key)
     * @param name The name of the app instance
     * @param appName The name of the Silvana app
     * @param developerName The name of the developer
     * @return instanceId The created instance identifier
     */
    function createAppInstance(
        string calldata instanceId,
        string calldata name,
        string calldata appName,
        string calldata developerName
    ) external returns (string memory);

    /**
     * @notice Delete an app instance
     * @param instanceId The instance identifier to delete
     */
    function deleteAppInstance(string calldata instanceId) external;

    /**
     * @notice Update the sequence state for an app instance
     * @param instanceId The instance identifier
     * @param sequenceNumber The new sequence number
     * @param actionsCommitment The actions commitment hash
     * @param stateCommitment The state commitment hash
     */
    function updateSequence(
        string calldata instanceId,
        uint256 sequenceNumber,
        bytes32 actionsCommitment,
        bytes32 stateCommitment
    ) external;

    /**
     * @notice Pause an app instance
     * @param instanceId The instance identifier to pause
     */
    function pauseAppInstance(string calldata instanceId) external;

    /**
     * @notice Unpause an app instance
     * @param instanceId The instance identifier to unpause
     */
    function unpauseAppInstance(string calldata instanceId) external;

    /**
     * @notice Transfer ownership of an app instance
     * @param instanceId The instance identifier
     * @param newOwner The new owner address
     */
    function transferOwnership(string calldata instanceId, address newOwner) external;

    /**
     * @notice Set the minimum time between blocks for an app instance
     * @param instanceId The instance identifier
     * @param minTime Minimum time in milliseconds
     */
    function setMinTimeBetweenBlocks(string calldata instanceId, uint64 minTime) external;

    /**
     * @notice Create a block for an app instance
     * @param instanceId The instance identifier
     * @param endSequence The ending sequence number for the block
     * @param actionsCommitment The actions commitment for the block
     * @param stateCommitment The state commitment for the block
     * @return blockNumber The created block number
     */
    function createBlock(
        string calldata instanceId,
        uint64 endSequence,
        bytes32 actionsCommitment,
        bytes32 stateCommitment
    ) external returns (uint64 blockNumber);

    // ============ Capability Management ============

    /**
     * @notice Create an app instance capability token
     * @param instanceId The instance identifier
     * @param recipient The recipient address for the capability
     * @return capId The capability identifier
     */
    function createAppInstanceCap(
        string calldata instanceId,
        address recipient
    ) external returns (bytes32 capId);

    /**
     * @notice Verify an app instance capability
     * @param capId The capability identifier
     * @param holder The address claiming to hold the capability
     * @return valid True if the capability is valid
     */
    function verifyAppInstanceCap(
        bytes32 capId,
        address holder
    ) external view returns (bool valid);

    /**
     * @notice Revoke an app instance capability
     * @param capId The capability identifier to revoke
     */
    function revokeAppInstanceCap(bytes32 capId) external;

    // ============ Read Functions ============

    /**
     * @notice Get an app instance by ID
     * @param instanceId The instance identifier
     * @return instance The app instance data
     */
    function getAppInstance(
        string calldata instanceId
    ) external view returns (DataTypes.AppInstance memory instance);

    /**
     * @notice Get all app instances
     * @param limit Maximum number of instances to return
     * @param offset Starting index for pagination
     * @return instances Array of app instances
     */
    function getAppInstances(
        uint256 limit,
        uint256 offset
    ) external view returns (DataTypes.AppInstance[] memory instances);

    /**
     * @notice Get app instances owned by an address
     * @param owner The owner address
     * @return instances Array of instance identifiers
     */
    function getAppInstancesByOwner(
        address owner
    ) external view returns (string[] memory instances);

    /**
     * @notice Get the current sequence state for an app instance
     * @param instanceId The instance identifier
     * @return state The current sequence state
     */
    function getSequenceState(
        string calldata instanceId
    ) external view returns (DataTypes.SequenceState memory state);

    /**
     * @notice Get the current sequence number for an app instance
     * @param instanceId The instance identifier
     * @return sequenceNumber The current sequence number
     */
    function getCurrentSequence(
        string calldata instanceId
    ) external view returns (uint256 sequenceNumber);

    /**
     * @notice Get detailed sequence information
     * @param instanceId The instance identifier
     * @param sequenceNumber The sequence number to query
     * @return state The sequence state at the given number
     */
    function getSequenceDetails(
        string calldata instanceId,
        uint256 sequenceNumber
    ) external view returns (DataTypes.SequenceState memory state);

    /**
     * @notice Check if an app instance exists
     * @param instanceId The instance identifier
     * @return exists True if the instance exists
     */
    function appInstanceExists(
        string memory instanceId
    ) external view returns (bool exists);

    /**
     * @notice Check if an app instance is active
     * @param instanceId The instance identifier
     * @return active True if the instance is active (not paused)
     */
    function isAppInstanceActive(
        string calldata instanceId
    ) external view returns (bool active);

    /**
     * @notice Check if an app instance is paused
     * @param instanceId The instance identifier
     * @return paused True if the instance is paused
     */
    function isAppInstancePaused(
        string calldata instanceId
    ) external view returns (bool paused);

    /**
     * @notice Get the owner of an app instance
     * @param instanceId The instance identifier
     * @return owner The owner address
     */
    function getAppInstanceOwner(
        string calldata instanceId
    ) external view returns (address owner);

    /**
     * @notice Get the admin of an app instance (alias for owner)
     * @param instanceId The instance identifier
     * @return admin The admin address
     */
    function getAppInstanceAdmin(
        string calldata instanceId
    ) external view returns (address admin);

    /**
     * @notice Get the total number of app instances
     * @return count Total number of instances
     */
    function getAppInstanceCount() external view returns (uint256 count);

    /**
     * @notice Get block information for an app instance
     * @param instanceId The instance identifier
     * @param blockNumber The block number
     * @return block The block data
     */
    function getBlock(
        string calldata instanceId,
        uint64 blockNumber
    ) external view returns (DataTypes.Block memory block);

    /**
     * @notice Get the latest block for an app instance
     * @param instanceId The instance identifier
     * @return block The latest block data
     */
    function getLatestBlock(
        string calldata instanceId
    ) external view returns (DataTypes.Block memory block);

    /**
     * @notice Get blocks in a range for an app instance
     * @param instanceId The instance identifier
     * @param fromBlock Starting block number
     * @param toBlock Ending block number
     * @return blocks Array of blocks in the range
     */
    function getBlocksRange(
        string calldata instanceId,
        uint64 fromBlock,
        uint64 toBlock
    ) external view returns (DataTypes.Block[] memory blocks);

    /**
     * @notice Get block by sequence number
     * @param instanceId The instance identifier
     * @param sequenceNumber The sequence number
     * @return block The block containing the sequence
     */
    function getBlockBySequence(
        string calldata instanceId,
        uint256 sequenceNumber
    ) external view returns (DataTypes.Block memory block);

    /**
     * @notice Set data availability for a block
     * @param instanceId The instance identifier
     * @param blockNumber The block number
     * @param stateDataAvailability State DA reference
     * @param proofDataAvailability Proof DA reference
     */
    function setDataAvailability(
        string calldata instanceId,
        uint64 blockNumber,
        string calldata stateDataAvailability,
        string calldata proofDataAvailability
    ) external;
}