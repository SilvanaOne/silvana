// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/IStorageManager.sol";
import "./AccessControl.sol";
import "./libraries/DataTypes.sol";

/**
 * @title StorageManager
 * @notice Manages key-value storage and metadata for app instances
 * @dev Implements two separate storage systems:
 *      1. KV Storage: Separate string and binary storage with key enumeration
 *      2. Metadata: String-based metadata storage
 */
contract StorageManager is AccessControl, IStorageManager {
    // Maximum key length to prevent gas attacks
    uint256 public constant MAX_KEY_LENGTH = 256;

    // ============ Storage Structures ============

    // App instance => key => string value
    mapping(string => mapping(string => string)) private kvString;

    // App instance => key => exists flag
    mapping(string => mapping(string => bool)) private kvStringExists;

    // App instance => array of string keys
    mapping(string => string[]) private kvStringKeys;

    // App instance => key => index in keys array
    mapping(string => mapping(string => uint256)) private kvStringKeyIndex;

    // App instance => keccak256(key) => binary value
    mapping(string => mapping(bytes32 => bytes)) private kvBinary;

    // App instance => keccak256(key) => exists flag
    mapping(string => mapping(bytes32 => bool)) private kvBinaryExists;

    // App instance => keccak256(key) => original key (for enumeration)
    mapping(string => mapping(bytes32 => bytes)) private kvBinaryKeyData;

    // App instance => array of key hashes
    mapping(string => bytes32[]) private kvBinaryKeyHashes;

    // App instance => keccak256(key) => index in key hashes array
    mapping(string => mapping(bytes32 => uint256)) private kvBinaryKeyIndex;

    // App instance => key => metadata value
    mapping(string => mapping(string => string)) private metadata;

    // App instance => key => exists flag
    mapping(string => mapping(string => bool)) private metadataExists;

    // ============ Constructor ============

    constructor() {}

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
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(bytes(key).length > 0 && bytes(key).length <= MAX_KEY_LENGTH, "StorageManager: Invalid key length");

        bool exists = kvStringExists[appInstance][key];

        // Store value
        kvString[appInstance][key] = value;

        if (!exists) {
            // Add to exists flag
            kvStringExists[appInstance][key] = true;

            // Add key to array
            kvStringKeys[appInstance].push(key);
            kvStringKeyIndex[appInstance][key] = kvStringKeys[appInstance].length - 1;
        }

        // Emit event
        emit DataTypes.KvStringSetEvent(
            appInstance,
            key,
            value,
            block.timestamp
        );
    }

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
    ) external view override returns (string memory value, bool exists) {
        exists = kvStringExists[appInstance][key];
        if (exists) {
            value = kvString[appInstance][key];
        }
    }

    /**
     * @notice Get all string key-value pairs for an app instance
     * @param appInstance The app instance identifier
     * @return keys Array of keys
     * @return values Array of values
     */
    function getAllKvString(
        string calldata appInstance
    ) external view override returns (string[] memory keys, string[] memory values) {
        keys = kvStringKeys[appInstance];
        values = new string[](keys.length);

        for (uint256 i = 0; i < keys.length; i++) {
            values[i] = kvString[appInstance][keys[i]];
        }
    }

    /**
     * @notice List all string keys for an app instance
     * @param appInstance The app instance identifier
     * @return keys Array of keys
     */
    function listKvStringKeys(
        string calldata appInstance
    ) external view override returns (string[] memory keys) {
        return kvStringKeys[appInstance];
    }

    /**
     * @notice Delete a string key-value pair
     * @param appInstance The app instance identifier
     * @param key The storage key to delete
     */
    function deleteKvString(
        string calldata appInstance,
        string calldata key
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(kvStringExists[appInstance][key], "StorageManager: Key not found");

        // Delete value
        delete kvString[appInstance][key];
        delete kvStringExists[appInstance][key];

        // Remove key from array (swap with last and pop)
        uint256 index = kvStringKeyIndex[appInstance][key];
        uint256 lastIndex = kvStringKeys[appInstance].length - 1;

        if (index != lastIndex) {
            string memory lastKey = kvStringKeys[appInstance][lastIndex];
            kvStringKeys[appInstance][index] = lastKey;
            kvStringKeyIndex[appInstance][lastKey] = index;
        }

        kvStringKeys[appInstance].pop();
        delete kvStringKeyIndex[appInstance][key];

        // Emit event
        emit DataTypes.KvStringDeletedEvent(
            appInstance,
            key,
            block.timestamp
        );
    }

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
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(key.length > 0 && key.length <= MAX_KEY_LENGTH, "StorageManager: Invalid key length");

        bytes32 keyHash = keccak256(key);
        bool exists = kvBinaryExists[appInstance][keyHash];

        // Store value
        kvBinary[appInstance][keyHash] = value;

        if (!exists) {
            // Add to exists flag
            kvBinaryExists[appInstance][keyHash] = true;

            // Store original key for enumeration
            kvBinaryKeyData[appInstance][keyHash] = key;

            // Add key hash to array
            kvBinaryKeyHashes[appInstance].push(keyHash);
            kvBinaryKeyIndex[appInstance][keyHash] = kvBinaryKeyHashes[appInstance].length - 1;
        }

        // Emit event
        emit DataTypes.KvBinarySetEvent(
            appInstance,
            key,
            value,
            block.timestamp
        );
    }

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
    ) external view override returns (bytes memory value, bool exists) {
        bytes32 keyHash = keccak256(key);
        exists = kvBinaryExists[appInstance][keyHash];
        if (exists) {
            value = kvBinary[appInstance][keyHash];
        }
    }

    /**
     * @notice List all binary keys for an app instance
     * @param appInstance The app instance identifier
     * @return keys Array of binary keys
     */
    function listKvBinaryKeys(
        string calldata appInstance
    ) external view override returns (bytes[] memory keys) {
        bytes32[] memory keyHashes = kvBinaryKeyHashes[appInstance];
        keys = new bytes[](keyHashes.length);

        for (uint256 i = 0; i < keyHashes.length; i++) {
            keys[i] = kvBinaryKeyData[appInstance][keyHashes[i]];
        }
    }

    /**
     * @notice Delete a binary key-value pair
     * @param appInstance The app instance identifier
     * @param key The storage key to delete (arbitrary bytes)
     */
    function deleteKvBinary(
        string calldata appInstance,
        bytes calldata key
    ) external override onlyRole(COORDINATOR_ROLE) {
        bytes32 keyHash = keccak256(key);
        require(kvBinaryExists[appInstance][keyHash], "StorageManager: Key not found");

        // Delete value and key data
        delete kvBinary[appInstance][keyHash];
        delete kvBinaryExists[appInstance][keyHash];
        delete kvBinaryKeyData[appInstance][keyHash];

        // Remove key hash from array (swap with last and pop)
        uint256 index = kvBinaryKeyIndex[appInstance][keyHash];
        uint256 lastIndex = kvBinaryKeyHashes[appInstance].length - 1;

        if (index != lastIndex) {
            bytes32 lastKeyHash = kvBinaryKeyHashes[appInstance][lastIndex];
            kvBinaryKeyHashes[appInstance][index] = lastKeyHash;
            kvBinaryKeyIndex[appInstance][lastKeyHash] = index;
        }

        kvBinaryKeyHashes[appInstance].pop();
        delete kvBinaryKeyIndex[appInstance][keyHash];

        // Emit event
        emit DataTypes.KvBinaryDeletedEvent(
            appInstance,
            key,
            block.timestamp
        );
    }

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
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(bytes(key).length > 0 && bytes(key).length <= MAX_KEY_LENGTH, "StorageManager: Invalid key length");

        metadata[appInstance][key] = value;
        metadataExists[appInstance][key] = true;

        // Note: Metadata doesn't have enumeration like KV storage
        // This is intentional to save gas for metadata operations
    }

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
    ) external view override returns (string memory value, bool exists) {
        exists = metadataExists[appInstance][key];
        if (exists) {
            value = metadata[appInstance][key];
        }
    }

    /**
     * @notice Delete metadata for an app instance
     * @param appInstance The app instance identifier
     * @param key The metadata key to delete
     */
    function deleteMetadata(
        string calldata appInstance,
        string calldata key
    ) external override onlyRole(COORDINATOR_ROLE) {
        require(metadataExists[appInstance][key], "StorageManager: Key not found");

        delete metadata[appInstance][key];
        delete metadataExists[appInstance][key];
    }
}
