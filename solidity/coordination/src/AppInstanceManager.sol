// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/IAppInstanceManager.sol";
import "./interfaces/IProofManager.sol";
import "./libraries/DataTypes.sol";
import "./AccessControl.sol";

/**
 * @title AppInstanceManager
 * @notice Manages app instances and their sequence states
 * @dev Implements app instance lifecycle and state management
 */
contract AppInstanceManager is IAppInstanceManager, AccessControl {
    // ============ Constants ============

    uint64 private constant MIN_TIME_BETWEEN_BLOCKS = 60000; // 1 minute minimum

    // ============ Storage ============

    // Instance ID => AppInstance
    mapping(string => DataTypes.AppInstance) private appInstances;

    // Instance ID => SequenceState
    mapping(string => DataTypes.SequenceState) private sequenceStates;

    // Instance ID => sequence number => SequenceState history
    mapping(string => mapping(uint256 => DataTypes.SequenceState)) private sequenceHistory;

    // Owner => array of instance IDs
    mapping(address => string[]) private ownerInstances;

    // Instance ID => block number => Block
    mapping(string => mapping(uint64 => DataTypes.Block)) private blocks;

    // Instance ID => current block number
    mapping(string => uint64) private currentBlockNumber;

    // Instance ID => array of block numbers
    mapping(string => uint64[]) private blockNumbers;

    // Capability management
    mapping(bytes32 => address) private capabilities;
    mapping(bytes32 => string) private capabilityInstances;

    // Total number of app instances
    uint256 private totalInstances;

    // Array of all instance IDs for enumeration
    string[] private allInstanceIds;

    // ProofManager reference (optional)
    IProofManager public proofManager;

    // Pause state
    bool private _paused;

    // ============ Modifiers ============

    modifier whenNotPaused() {
        require(!_paused, "AppInstanceManager: paused");
        _;
    }

    modifier instanceExists(string calldata instanceId) {
        require(appInstanceExists(instanceId), "AppInstanceManager: instance not found");
        _;
    }

    modifier instanceActive(string calldata instanceId) {
        require(!appInstances[instanceId].isPaused, "AppInstanceManager: instance paused");
        _;
    }

    modifier onlyInstanceOwner(string calldata instanceId) {
        require(
            appInstances[instanceId].owner == msg.sender || hasRole(ADMIN_ROLE, msg.sender),
            "AppInstanceManager: not instance owner"
        );
        _;
    }

    // ============ Constructor ============

    constructor() AccessControl() {}

    // ============ External Functions - Write ============

    /**
     * @inheritdoc IAppInstanceManager
     */
    function createAppInstance(
        string calldata name,
        string calldata appName,
        string calldata developerName
    ) external override whenNotPaused returns (string memory instanceId) {
        require(bytes(name).length > 0, "AppInstanceManager: empty name");
        require(bytes(appName).length > 0, "AppInstanceManager: empty app name");
        require(bytes(developerName).length > 0, "AppInstanceManager: empty developer name");

        // Generate unique instance ID
        instanceId = _generateInstanceId(name, msg.sender);

        // Ensure ID is unique
        require(!appInstanceExists(instanceId), "AppInstanceManager: instance already exists");

        // Create app instance
        DataTypes.AppInstance storage instance = appInstances[instanceId];
        instance.id = instanceId;
        instance.name = name;
        instance.owner = msg.sender;
        instance.silvanaAppName = appName;
        instance.developerName = developerName;
        instance.sequenceNumber = 0;
        instance.blockNumber = 0;
        instance.isPaused = false;
        instance.minTimeBetweenBlocks = MIN_TIME_BETWEEN_BLOCKS;
        instance.createdAt = block.timestamp;
        instance.updatedAt = block.timestamp;

        // Initialize sequence state
        sequenceStates[instanceId] = DataTypes.SequenceState({
            sequenceNumber: 0,
            actionsCommitment: bytes32(0),
            stateCommitment: bytes32(0),
            blockNumber: 0,
            timestamp: block.timestamp
        });

        // Add to owner's instances
        ownerInstances[msg.sender].push(instanceId);

        // Add to all instances
        allInstanceIds.push(instanceId);
        totalInstances++;

        // Emit event
        emit DataTypes.AppInstanceCreated(instanceId, msg.sender, appName, developerName, block.timestamp);

        return instanceId;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function deleteAppInstance(string calldata instanceId)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
    {
        // Remove from owner's instances
        address owner = appInstances[instanceId].owner;
        string[] storage instances = ownerInstances[owner];
        for (uint256 i = 0; i < instances.length; i++) {
            if (keccak256(bytes(instances[i])) == keccak256(bytes(instanceId))) {
                instances[i] = instances[instances.length - 1];
                instances.pop();
                break;
            }
        }

        // Remove from all instances
        for (uint256 i = 0; i < allInstanceIds.length; i++) {
            if (keccak256(bytes(allInstanceIds[i])) == keccak256(bytes(instanceId))) {
                allInstanceIds[i] = allInstanceIds[allInstanceIds.length - 1];
                allInstanceIds.pop();
                break;
            }
        }

        // Delete app instance
        delete appInstances[instanceId];
        delete sequenceStates[instanceId];

        // Clean up blocks
        uint64[] storage blockNums = blockNumbers[instanceId];
        for (uint256 i = 0; i < blockNums.length; i++) {
            delete blocks[instanceId][blockNums[i]];
        }
        delete blockNumbers[instanceId];
        delete currentBlockNumber[instanceId];

        totalInstances--;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function updateSequence(
        string calldata instanceId,
        uint256 sequenceNumber,
        bytes32 actionsCommitment,
        bytes32 stateCommitment
    ) external override whenNotPaused instanceExists(instanceId) instanceActive(instanceId) {
        // Check authorization - either instance owner or coordinator
        require(
            appInstances[instanceId].owner == msg.sender ||
                hasRole(COORDINATOR_ROLE, msg.sender) ||
                hasRole(ADMIN_ROLE, msg.sender),
            "AppInstanceManager: not authorized"
        );

        DataTypes.AppInstance storage instance = appInstances[instanceId];

        // Validate sequence number is incrementing
        require(sequenceNumber > instance.sequenceNumber, "AppInstanceManager: invalid sequence number");

        // Update instance
        instance.sequenceNumber = sequenceNumber;
        instance.stateCommitment = stateCommitment;
        instance.actionsCommitment = actionsCommitment;
        instance.updatedAt = block.timestamp;

        // Update sequence state
        DataTypes.SequenceState memory newState = DataTypes.SequenceState({
            sequenceNumber: sequenceNumber,
            actionsCommitment: actionsCommitment,
            stateCommitment: stateCommitment,
            blockNumber: instance.blockNumber,
            timestamp: block.timestamp
        });

        sequenceStates[instanceId] = newState;
        sequenceHistory[instanceId][sequenceNumber] = newState;

        // Emit event
        emit DataTypes.SequenceUpdated(instanceId, sequenceNumber, stateCommitment, actionsCommitment, block.timestamp);
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function pauseAppInstance(string calldata instanceId)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
    {
        DataTypes.AppInstance storage instance = appInstances[instanceId];
        require(!instance.isPaused, "AppInstanceManager: already paused");

        instance.isPaused = true;
        instance.updatedAt = block.timestamp;

        emit DataTypes.AppInstancePaused(instanceId, msg.sender, block.timestamp);
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function unpauseAppInstance(string calldata instanceId)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
    {
        DataTypes.AppInstance storage instance = appInstances[instanceId];
        require(instance.isPaused, "AppInstanceManager: not paused");

        instance.isPaused = false;
        instance.updatedAt = block.timestamp;

        emit DataTypes.AppInstanceUnpaused(instanceId, msg.sender, block.timestamp);
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function transferOwnership(string calldata instanceId, address newOwner)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
    {
        require(newOwner != address(0), "AppInstanceManager: new owner is zero address");

        DataTypes.AppInstance storage instance = appInstances[instanceId];
        address oldOwner = instance.owner;

        // Update owner
        instance.owner = newOwner;
        instance.updatedAt = block.timestamp;

        // Update owner instances mappings
        // Remove from old owner
        string[] storage oldInstances = ownerInstances[oldOwner];
        for (uint256 i = 0; i < oldInstances.length; i++) {
            if (keccak256(bytes(oldInstances[i])) == keccak256(bytes(instanceId))) {
                oldInstances[i] = oldInstances[oldInstances.length - 1];
                oldInstances.pop();
                break;
            }
        }

        // Add to new owner
        ownerInstances[newOwner].push(instanceId);
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function setMinTimeBetweenBlocks(string calldata instanceId, uint64 minTime)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
    {
        require(minTime >= MIN_TIME_BETWEEN_BLOCKS, "AppInstanceManager: time too short");

        DataTypes.AppInstance storage instance = appInstances[instanceId];
        instance.minTimeBetweenBlocks = minTime;
        instance.updatedAt = block.timestamp;
    }

    /**
     * @notice Set the proof manager contract address
     * @param _proofManager Address of the proof manager contract
     */
    function setProofManager(address _proofManager) external onlyRole(ADMIN_ROLE) {
        proofManager = IProofManager(_proofManager);
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function createBlock(
        string calldata instanceId,
        uint64 endSequence,
        bytes32 actionsCommitment,
        bytes32 stateCommitment
    ) external override whenNotPaused instanceExists(instanceId) instanceActive(instanceId) returns (uint64 blockNum) {
        require(
            hasRole(COORDINATOR_ROLE, msg.sender) || hasRole(ADMIN_ROLE, msg.sender),
            "AppInstanceManager: not coordinator"
        );

        DataTypes.AppInstance storage instance = appInstances[instanceId];

        // Check minimum time between blocks
        if (instance.previousBlockTimestamp > 0) {
            require(
                block.timestamp >= instance.previousBlockTimestamp + (instance.minTimeBetweenBlocks / 1000),
                "AppInstanceManager: too soon for new block"
            );
        }

        // Get next block number
        blockNum = currentBlockNumber[instanceId] + 1;
        currentBlockNumber[instanceId] = blockNum;

        // Calculate start sequence
        uint64 startSequence = instance.previousBlockLastSequence + 1;

        // Create block
        DataTypes.Block storage newBlock = blocks[instanceId][blockNum];
        newBlock.blockNumber = blockNum;
        newBlock.name = string(abi.encodePacked(instanceId, "-block-", _uint64ToString(blockNum)));
        newBlock.startSequence = startSequence;
        newBlock.endSequence = endSequence;
        newBlock.actionsCommitment = actionsCommitment;
        newBlock.stateCommitment = stateCommitment;
        newBlock.timeSinceLastBlock = uint64(block.timestamp) - instance.previousBlockTimestamp;
        newBlock.numberOfTransactions = endSequence - startSequence + 1;
        newBlock.startActionsCommitment = instance.actionsCommitment;
        newBlock.endActionsCommitment = actionsCommitment;
        newBlock.createdAt = block.timestamp;

        // Update instance
        instance.blockNumber = blockNum;
        instance.previousBlockTimestamp = uint64(block.timestamp);
        instance.previousBlockLastSequence = endSequence;
        instance.stateCommitment = stateCommitment;
        instance.actionsCommitment = actionsCommitment;
        instance.updatedAt = block.timestamp;

        // Add to block numbers array
        blockNumbers[instanceId].push(blockNum);

        // Create proof calculation for next block (if ProofManager is set)
        if (address(proofManager) != address(0) && blockNum > 0) {
            proofManager.createProofCalculation(
                instanceId,
                blockNum + 1,
                endSequence + 1,  // Next block starts at endSequence + 1
                0  // End sequence unknown yet
            );
        }

        return blockNum;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function setDataAvailability(
        string calldata instanceId,
        uint64 blockNumber,
        string calldata stateDataAvailability,
        string calldata proofDataAvailability
    ) external override whenNotPaused instanceExists(instanceId) {
        require(
            hasRole(COORDINATOR_ROLE, msg.sender) || hasRole(ADMIN_ROLE, msg.sender),
            "AppInstanceManager: not coordinator"
        );

        DataTypes.Block storage blockData = blocks[instanceId][blockNumber];
        require(blockData.blockNumber == blockNumber, "AppInstanceManager: block not found");

        blockData.stateDataAvailability = stateDataAvailability;
        blockData.proofDataAvailability = proofDataAvailability;
        blockData.stateCalculatedAt = block.timestamp;
        blockData.provedAt = block.timestamp;
    }

    // ============ Capability Management ============

    /**
     * @inheritdoc IAppInstanceManager
     */
    function createAppInstanceCap(string calldata instanceId, address recipient)
        external
        override
        whenNotPaused
        instanceExists(instanceId)
        onlyInstanceOwner(instanceId)
        returns (bytes32 capId)
    {
        require(recipient != address(0), "AppInstanceManager: recipient is zero address");

        // Generate capability ID
        capId = keccak256(abi.encodePacked(instanceId, recipient, block.timestamp, block.number));

        // Store capability
        capabilities[capId] = recipient;
        capabilityInstances[capId] = instanceId;

        return capId;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function verifyAppInstanceCap(bytes32 capId, address holder) external view override returns (bool) {
        return capabilities[capId] == holder;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function revokeAppInstanceCap(bytes32 capId) external override whenNotPaused {
        string memory instanceId = capabilityInstances[capId];
        require(
            appInstances[instanceId].owner == msg.sender || hasRole(ADMIN_ROLE, msg.sender),
            "AppInstanceManager: not authorized"
        );

        delete capabilities[capId];
        delete capabilityInstances[capId];
    }

    // ============ External Functions - Read ============

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstance(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.AppInstance memory)
    {
        return appInstances[instanceId];
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstances(uint256 limit, uint256 offset)
        external
        view
        override
        returns (DataTypes.AppInstance[] memory)
    {
        uint256 total = allInstanceIds.length;

        if (offset >= total) {
            return new DataTypes.AppInstance[](0);
        }

        uint256 end = offset + limit;
        if (end > total) {
            end = total;
        }

        uint256 count = end - offset;
        DataTypes.AppInstance[] memory result = new DataTypes.AppInstance[](count);

        for (uint256 i = 0; i < count; i++) {
            result[i] = appInstances[allInstanceIds[offset + i]];
        }

        return result;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstancesByOwner(address owner) external view override returns (string[] memory) {
        return ownerInstances[owner];
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getSequenceState(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.SequenceState memory)
    {
        return sequenceStates[instanceId];
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getCurrentSequence(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (uint256)
    {
        return appInstances[instanceId].sequenceNumber;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getSequenceDetails(string calldata instanceId, uint256 sequenceNumber)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.SequenceState memory)
    {
        DataTypes.SequenceState memory state = sequenceHistory[instanceId][sequenceNumber];
        require(state.sequenceNumber == sequenceNumber, "AppInstanceManager: sequence not found");
        return state;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function appInstanceExists(string memory instanceId) public view override returns (bool) {
        return bytes(appInstances[instanceId].id).length > 0;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function isAppInstanceActive(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (bool)
    {
        return !appInstances[instanceId].isPaused;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function isAppInstancePaused(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (bool)
    {
        return appInstances[instanceId].isPaused;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstanceOwner(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (address)
    {
        return appInstances[instanceId].owner;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstanceAdmin(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (address)
    {
        return appInstances[instanceId].owner;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getAppInstanceCount() external view override returns (uint256) {
        return totalInstances;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getBlock(string calldata instanceId, uint64 blockNumber)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.Block memory)
    {
        DataTypes.Block memory blockData = blocks[instanceId][blockNumber];
        require(blockData.blockNumber == blockNumber, "AppInstanceManager: block not found");
        return blockData;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getLatestBlock(string calldata instanceId)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.Block memory)
    {
        uint64 latestBlockNum = currentBlockNumber[instanceId];
        require(latestBlockNum > 0, "AppInstanceManager: no blocks");
        return blocks[instanceId][latestBlockNum];
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getBlocksRange(string calldata instanceId, uint64 fromBlock, uint64 toBlock)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.Block[] memory)
    {
        require(fromBlock <= toBlock, "AppInstanceManager: invalid range");

        uint256 count = toBlock - fromBlock + 1;
        DataTypes.Block[] memory result = new DataTypes.Block[](count);

        for (uint256 i = 0; i < count; i++) {
            uint64 blockNum = fromBlock + uint64(i);
            DataTypes.Block memory blockData = blocks[instanceId][blockNum];
            if (blockData.blockNumber == blockNum) {
                result[i] = blockData;
            }
        }

        return result;
    }

    /**
     * @inheritdoc IAppInstanceManager
     */
    function getBlockBySequence(string calldata instanceId, uint256 sequenceNumber)
        external
        view
        override
        instanceExists(instanceId)
        returns (DataTypes.Block memory)
    {
        uint64[] storage blockNums = blockNumbers[instanceId];

        for (uint256 i = 0; i < blockNums.length; i++) {
            DataTypes.Block memory blockData = blocks[instanceId][blockNums[i]];
            if (blockData.startSequence <= sequenceNumber && sequenceNumber <= blockData.endSequence) {
                return blockData;
            }
        }

        revert("AppInstanceManager: block not found for sequence");
    }

    /**
     * @notice Get proof calculation for a specific block
     * @param instanceId The app instance identifier
     * @param blockNumber The block number
     * @return blockNum Block number
     * @return startSeq Starting sequence
     * @return endSeq Ending sequence
     * @return blockProof Block proof hash
     * @return isFinished Whether calculation is finished
     */
    function getProofCalculationForBlock(
        string calldata instanceId,
        uint64 blockNumber
    ) external view instanceExists(instanceId) returns (
        uint64 blockNum,
        uint64 startSeq,
        uint64 endSeq,
        string memory blockProof,
        bool isFinished
    ) {
        require(address(proofManager) != address(0), "AppInstanceManager: ProofManager not set");
        return proofManager.getProofCalculation(instanceId, blockNumber);
    }

    // ============ Internal Functions ============

    /**
     * @dev Generate a unique instance ID
     */
    function _generateInstanceId(string memory name, address owner) internal view returns (string memory) {
        bytes32 hash = keccak256(abi.encodePacked(name, owner, block.timestamp, block.number));
        return string(abi.encodePacked(name, "-", _bytes32ToString(hash)));
    }

    /**
     * @dev Convert bytes32 to string
     */
    function _bytes32ToString(bytes32 data) internal pure returns (string memory) {
        bytes memory alphabet = "0123456789abcdef";
        bytes memory str = new bytes(8);

        for (uint256 i = 0; i < 4; i++) {
            str[i * 2] = alphabet[uint256(uint8(data[i] >> 4))];
            str[i * 2 + 1] = alphabet[uint256(uint8(data[i] & 0x0f))];
        }

        return string(str);
    }

    /**
     * @dev Convert uint64 to string
     */
    function _uint64ToString(uint64 value) internal pure returns (string memory) {
        if (value == 0) {
            return "0";
        }

        uint64 temp = value;
        uint256 digits;

        while (temp != 0) {
            digits++;
            temp /= 10;
        }

        bytes memory buffer = new bytes(digits);
        while (value != 0) {
            digits -= 1;
            buffer[digits] = bytes1(uint8(48 + uint256(value % 10)));
            value /= 10;
        }

        return string(buffer);
    }

    // ============ Admin Functions ============

    /**
     * @notice Pause the contract
     */
    function pause() external onlyRole(PAUSER_ROLE) {
        _paused = true;
    }

    /**
     * @notice Unpause the contract
     */
    function unpause() external onlyRole(PAUSER_ROLE) {
        _paused = false;
    }

    /**
     * @notice Check if the contract is paused
     */
    function paused() external view returns (bool) {
        return _paused;
    }
}