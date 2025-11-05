// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

/**
 * @title IAddApp
 * @notice Interface for the AddApp example contract
 */
interface IAddApp {
    // ============ Events ============

    event AppInitialized(
        string indexed appInstanceId,
        uint256 initialSum,
        bytes32 initialCommitment,
        uint256 timestamp
    );

    event ValueAdded(
        uint32 indexed index,
        uint256 oldValue,
        uint256 newValue,
        uint256 amountAdded,
        uint256 oldSum,
        uint256 newSum,
        bytes32 oldCommitment,
        bytes32 newCommitment,
        uint256 sequence,
        uint256 jobId
    );

    // ============ Errors ============

    error InvalidValue(uint256 value);
    error InvalidIndex(uint32 index);
    error ReservedIndex(uint32 index);
    error Unauthorized();
    error NotInitialized();

    // ============ Functions ============

    function initialize(string calldata appInstanceId) external;
    function add(uint32 index, uint256 value) external returns (uint256 jobId);
    function getValue(uint32 index) external view returns (uint256);
    function getSum() external view returns (uint256);
    function getStateCommitment() external view returns (bytes32);
    function getSequence() external view returns (uint256);
}
