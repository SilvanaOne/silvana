// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "coordination/interfaces/ICoordination.sol";
import "coordination/libraries/DataTypes.sol";

/**
 * @title AddApp
 * @notice Simple example app that demonstrates Silvana coordination layer integration
 * @dev Maintains indexed uint256 values and a running sum, creates jobs for each add operation
 *
 * This is the Ethereum Solidity equivalent of the Sui Move add example.
 *
 * Features:
 * - Index 0: Stores the total sum of all values
 * - Indexes 1-999999: Store individual values
 * - Values must be < 100
 * - Each add operation creates a job in the coordination layer
 * - State commitments track the app state
 * - Sequence numbers track operation order
 */
contract AddApp {
    // ============ State Variables ============

    /// @notice Reference to Silvana coordination contract
    ICoordination public immutable coordination;

    /// @notice App instance identifier in the coordination layer
    string public appInstanceId;

    /// @notice Total sum of all values (stored at conceptual index 0)
    uint256 public sum;

    /// @notice Indexed values storage (index 1 onwards)
    mapping(uint32 => uint256) public values;

    /// @notice Current sequence number
    uint256 public sequence;

    /// @notice Current state commitment hash
    bytes32 public stateCommitment;

    /// @notice App owner address
    address public owner;

    // ============ Constants ============

    uint32 public constant SUM_INDEX = 0;
    uint32 public constant MAX_INDEX = 1_000_000;
    uint256 public constant MAX_VALUE = 100;

    // ============ Errors ============

    error InvalidValue(uint256 value);
    error InvalidIndex(uint32 index);
    error ReservedIndex(uint32 index);
    error Unauthorized();
    error NotInitialized();

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

    // ============ Structs ============

    /// @notice Transition data for job processing (matches Sui Move TransitionData)
    struct TransitionData {
        uint64 blockNumber;
        uint256 sequence;
        string method;
        uint32 index;
        uint256 value;
        uint256 oldValue;
        uint256 oldSum;
        uint256 newSum;
        bytes32 oldCommitment;
        bytes32 newCommitment;
    }

    // ============ Modifiers ============

    modifier onlyOwner() {
        if (msg.sender != owner) revert Unauthorized();
        _;
    }

    modifier initialized() {
        if (bytes(appInstanceId).length == 0) revert NotInitialized();
        _;
    }

    // ============ Constructor ============

    /**
     * @notice Initialize the AddApp contract
     * @param _coordination Address of the SilvanaCoordination contract
     */
    constructor(address _coordination) {
        require(_coordination != address(0), "AddApp: coordination is zero address");
        coordination = ICoordination(_coordination);
        owner = msg.sender;
    }

    // ============ Initialization ============

    /**
     * @notice Initialize the app with an app instance in the coordination layer
     * @param _appInstanceId The app instance ID created via coordination.createAppInstance()
     */
    function initialize(string calldata _appInstanceId) external onlyOwner {
        require(bytes(appInstanceId).length == 0, "AddApp: already initialized");
        require(bytes(_appInstanceId).length > 0, "AddApp: empty app instance ID");

        appInstanceId = _appInstanceId;
        sum = 0;
        sequence = 0;

        // Compute initial state commitment
        stateCommitment = _computeStateCommitment();

        emit AppInitialized(
            appInstanceId,
            sum,
            stateCommitment,
            block.timestamp
        );
    }

    // ============ Main Functions ============

    /**
     * @notice Add a value to the indexed storage
     * @param index The index to add to (must be > 0 and < MAX_INDEX)
     * @param value The value to add (must be < MAX_VALUE)
     */
    function add(uint32 index, uint256 value) external initialized returns (uint256 jobId) {
        // Validation
        if (index <= SUM_INDEX) revert ReservedIndex(index);
        if (index >= MAX_INDEX) revert InvalidIndex(index);
        if (value >= MAX_VALUE) revert InvalidValue(value);

        // Get old values
        uint256 oldValue = values[index];
        uint256 oldSum = sum;
        bytes32 oldCommitment = stateCommitment;

        // Calculate new values
        uint256 newValue = oldValue + value;
        uint256 newSum = oldSum + value;

        // Update state
        values[index] = newValue;
        sum = newSum;
        sequence++;

        // Compute new state commitment
        bytes32 newCommitment = _computeStateCommitment();
        stateCommitment = newCommitment;

        // Create transition data
        TransitionData memory transitionData = TransitionData({
            blockNumber: uint64(block.number),
            sequence: sequence,
            method: "add",
            index: index,
            value: value,
            oldValue: oldValue,
            oldSum: oldSum,
            newSum: newSum,
            oldCommitment: oldCommitment,
            newCommitment: newCommitment
        });

        // Create job in coordination layer
        jobId = _createJob(transitionData);

        // Emit event
        emit ValueAdded(
            index,
            oldValue,
            newValue,
            value,
            oldSum,
            newSum,
            oldCommitment,
            newCommitment,
            sequence,
            jobId
        );

        return jobId;
    }

    // ============ View Functions ============

    /**
     * @notice Get the value at a specific index
     * @param index The index to query
     * @return The value at the index (0 if not set)
     */
    function getValue(uint32 index) external view returns (uint256) {
        if (index <= SUM_INDEX) revert ReservedIndex(index);
        if (index >= MAX_INDEX) revert InvalidIndex(index);
        return values[index];
    }

    /**
     * @notice Get the current sum of all values
     * @return The total sum
     */
    function getSum() external view returns (uint256) {
        return sum;
    }

    /**
     * @notice Get the current state commitment
     * @return The current state commitment hash
     */
    function getStateCommitment() external view returns (bytes32) {
        return stateCommitment;
    }

    /**
     * @notice Get the current sequence number
     * @return The current sequence
     */
    function getSequence() external view returns (uint256) {
        return sequence;
    }

    // ============ Internal Functions ============

    /**
     * @notice Compute the state commitment hash
     * @dev Uses keccak256 hash of sum, sequence, and timestamp
     * @return The computed state commitment
     */
    function _computeStateCommitment() internal view returns (bytes32) {
        return keccak256(abi.encodePacked(
            sum,
            sequence,
            block.timestamp
        ));
    }

    /**
     * @notice Create a job in the coordination layer
     * @param transitionData The transition data for the job
     * @return jobId The created job ID
     */
    function _createJob(TransitionData memory transitionData) internal returns (uint256 jobId) {
        // Encode transition data
        bytes memory data = abi.encode(transitionData);

        // Create job input
        DataTypes.JobInput memory jobInput = DataTypes.JobInput({
            description: "Add operation job",
            developer: "AddEthereumDeveloper",
            agent: "AddEthereumAgent",
            agentMethod: "prove",
            app: "add_ethereum_app",
            appInstance: appInstanceId,
            appInstanceMethod: "add",
            data: data,
            intervalMs: 0  // one-time job, not periodic
        });

        // Create job through coordination contract
        jobId = coordination.jobManager().createJob(jobInput);

        return jobId;
    }
}
