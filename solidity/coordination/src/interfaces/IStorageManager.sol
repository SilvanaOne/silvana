// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

/**
 * @title IStorageManager
 * @notice Interface for managing key-value storage and metadata for app instances
 * @dev Manages two separate storage systems:
 *      1. KV Storage: Key-value pairs for app instance data (string and binary)
 *      2. Metadata: App instance metadata key-value pairs
 */
interface IStorageManager {
    // ============ String KV Storage Functions ============

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
     * @notice Get all string key-value pairs for an app instance
     * @param appInstance The app instance identifier
     * @return keys Array of keys
     * @return values Array of values
     */
    function getAllKvString(
        string calldata appInstance
    ) external view returns (string[] memory keys, string[] memory values);

    /**
     * @notice List all string keys for an app instance
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

    // ============ Binary KV Storage Functions ============

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
     * @notice List all binary keys for an app instance
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

    // ============ Metadata Functions ============

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
}
