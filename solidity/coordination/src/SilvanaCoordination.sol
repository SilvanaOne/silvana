// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/ICoordination.sol";
import "./interfaces/IJobManager.sol";
import "./interfaces/IAppInstanceManager.sol";
import "./interfaces/IProofManager.sol";
import "./interfaces/ISettlementManager.sol";
import "./interfaces/IStorageManager.sol";
import "./AccessControl.sol";

/**
 * @title SilvanaCoordination
 * @notice Main coordination contract for the Silvana Layer
 * @dev Routes calls to component contracts and manages system-wide operations
 */
contract SilvanaCoordination is ICoordination, AccessControl {
    // ============ Storage ============

    IJobManager public override jobManager;
    IAppInstanceManager public override appInstanceManager;
    IProofManager public proofManager;
    ISettlementManager public settlementManager;
    IStorageManager public storageManager;

    uint256 public override multicallNonce;
    bytes[] public pendingMulticalls;

    bool private initialized;
    bool private _paused;

    // ============ Modifiers ============

    modifier whenNotPaused() {
        require(!_paused, "SilvanaCoordination: paused");
        _;
    }

    modifier onlyInitialized() {
        require(initialized, "SilvanaCoordination: not initialized");
        _;
    }

    // ============ Constructor ============

    constructor() AccessControl() {}

    // ============ Initialization ============

    /**
     * @inheritdoc ICoordination
     */
    function initialize(
        address _admin,
        address _jobManager,
        address _appInstanceManager,
        address _proofManager,
        address _settlementManager,
        address _storageManager
    ) external override {
        require(!initialized, "SilvanaCoordination: already initialized");
        require(_admin != address(0), "SilvanaCoordination: admin is zero address");
        require(_jobManager != address(0), "SilvanaCoordination: jobManager is zero address");
        require(_appInstanceManager != address(0), "SilvanaCoordination: appInstanceManager is zero address");

        // Set admin
        if (_admin != msg.sender) {
            _revokeRole(ADMIN_ROLE, msg.sender);
            _grantRole(ADMIN_ROLE, _admin);
        }

        // Set component contracts
        jobManager = IJobManager(_jobManager);
        appInstanceManager = IAppInstanceManager(_appInstanceManager);
        if (_proofManager != address(0)) {
            proofManager = IProofManager(_proofManager);
        }
        if (_settlementManager != address(0)) {
            settlementManager = ISettlementManager(_settlementManager);
        }
        if (_storageManager != address(0)) {
            storageManager = IStorageManager(_storageManager);
        }

        initialized = true;

        emit Initialized(_admin, _jobManager, _appInstanceManager);
    }

    // ============ Component Updates ============

    /**
     * @inheritdoc ICoordination
     */
    function setJobManager(address _jobManager) external override onlyRole(ADMIN_ROLE) onlyInitialized {
        require(_jobManager != address(0), "SilvanaCoordination: jobManager is zero address");

        address oldManager = address(jobManager);
        jobManager = IJobManager(_jobManager);

        emit ComponentUpdated("JobManager", oldManager, _jobManager);
    }

    /**
     * @inheritdoc ICoordination
     */
    function setAppInstanceManager(address _appInstanceManager) external override onlyRole(ADMIN_ROLE) onlyInitialized {
        require(_appInstanceManager != address(0), "SilvanaCoordination: appInstanceManager is zero address");

        address oldManager = address(appInstanceManager);
        appInstanceManager = IAppInstanceManager(_appInstanceManager);

        emit ComponentUpdated("AppInstanceManager", oldManager, _appInstanceManager);
    }

    // ============ Emergency Functions ============

    /**
     * @inheritdoc ICoordination
     */
    function pause() external override onlyRole(PAUSER_ROLE) {
        _paused = true;
        emit Paused(msg.sender);
    }

    /**
     * @inheritdoc ICoordination
     */
    function unpause() external override onlyRole(PAUSER_ROLE) {
        _paused = false;
        emit Unpaused(msg.sender);
    }

    /**
     * @inheritdoc ICoordination
     */
    function paused() external view override returns (bool) {
        return _paused;
    }

    // ============ Multicall Support ============

    /**
     * @inheritdoc ICoordination
     */
    function multicall(
        address[] calldata targets,
        bytes[] calldata data
    ) external override whenNotPaused onlyRole(COORDINATOR_ROLE) onlyInitialized returns (bytes[] memory results) {
        require(targets.length == data.length, "SilvanaCoordination: arrays mismatch");
        require(targets.length > 0, "SilvanaCoordination: empty arrays");

        uint256 nonce = multicallNonce++;
        results = new bytes[](targets.length);

        for (uint256 i = 0; i < targets.length; i++) {
            require(targets[i] != address(0), "SilvanaCoordination: target is zero address");

            // Check target is a valid component
            require(
                targets[i] == address(jobManager) || targets[i] == address(appInstanceManager),
                "SilvanaCoordination: invalid target"
            );

            (bool success, bytes memory result) = targets[i].call(data[i]);
            require(success, string(abi.encodePacked("SilvanaCoordination: call failed at index ", _uint256ToString(i))));
            results[i] = result;
        }

        emit MulticallExecuted(nonce, targets, block.timestamp);

        return results;
    }

    // ============ Delegated Job Functions ============

    /**
     * @notice Create a job through delegation
     * @dev Delegates to JobManager
     */
    function createJob(DataTypes.JobInput calldata input) external whenNotPaused onlyInitialized returns (uint256) {
        return jobManager.createJob(input);
    }

    /**
     * @notice Take a job through delegation
     * @dev Delegates to JobManager
     */
    function takeJob(string calldata appInstance, uint256 jobId) external whenNotPaused onlyInitialized {
        jobManager.takeJob(appInstance, jobId);
    }

    /**
     * @notice Complete a job through delegation
     * @dev Delegates to JobManager
     */
    function completeJob(string calldata appInstance, uint256 jobId, bytes calldata output)
        external
        whenNotPaused
        onlyInitialized
    {
        jobManager.completeJob(appInstance, jobId, output);
    }

    /**
     * @notice Fail a job through delegation
     * @dev Delegates to JobManager
     */
    function failJob(string calldata appInstance, uint256 jobId, string calldata error)
        external
        whenNotPaused
        onlyInitialized
    {
        jobManager.failJob(appInstance, jobId, error);
    }

    // ============ Delegated App Instance Functions ============

    /**
     * @notice Create an app instance through delegation
     * @dev Delegates to AppInstanceManager
     */
    function createAppInstance(
        string calldata name,
        string calldata appName,
        string calldata developerName
    ) external whenNotPaused onlyInitialized returns (string memory) {
        return appInstanceManager.createAppInstance(name, appName, developerName);
    }

    /**
     * @notice Update sequence through delegation
     * @dev Delegates to AppInstanceManager
     */
    function updateSequence(
        string calldata instanceId,
        uint256 sequenceNumber,
        bytes32 actionsCommitment,
        bytes32 stateCommitment
    ) external whenNotPaused onlyInitialized {
        appInstanceManager.updateSequence(instanceId, sequenceNumber, actionsCommitment, stateCommitment);
    }

    /**
     * @notice Pause an app instance through delegation
     * @dev Delegates to AppInstanceManager
     */
    function pauseAppInstance(string calldata instanceId) external whenNotPaused onlyInitialized {
        appInstanceManager.pauseAppInstance(instanceId);
    }

    /**
     * @notice Unpause an app instance through delegation
     * @dev Delegates to AppInstanceManager
     */
    function unpauseAppInstance(string calldata instanceId) external whenNotPaused onlyInitialized {
        appInstanceManager.unpauseAppInstance(instanceId);
    }

    // ============ View Functions ============

    /**
     * @notice Get a job by ID
     * @dev Delegates to JobManager
     */
    function getJob(string calldata appInstance, uint256 jobId)
        external
        view
        onlyInitialized
        returns (DataTypes.Job memory)
    {
        return jobManager.getJob(appInstance, jobId);
    }

    /**
     * @notice Get pending jobs
     * @dev Delegates to JobManager
     */
    function getPendingJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        onlyInitialized
        returns (DataTypes.Job[] memory)
    {
        return jobManager.getPendingJobs(appInstance, limit, offset);
    }

    /**
     * @notice Get an app instance
     * @dev Delegates to AppInstanceManager
     */
    function getAppInstance(string calldata instanceId)
        external
        view
        onlyInitialized
        returns (DataTypes.AppInstance memory)
    {
        return appInstanceManager.getAppInstance(instanceId);
    }

    /**
     * @notice Get current sequence
     * @dev Delegates to AppInstanceManager
     */
    function getCurrentSequence(string calldata instanceId) external view onlyInitialized returns (uint256) {
        return appInstanceManager.getCurrentSequence(instanceId);
    }

    /**
     * @notice Check if app instance is paused
     * @dev Delegates to AppInstanceManager
     */
    function isAppInstancePaused(string calldata instanceId) external view onlyInitialized returns (bool) {
        return appInstanceManager.isAppInstancePaused(instanceId);
    }

    // ============ Proof Management (Delegation) ============

    /**
     * @notice Start proving a sequence range
     * @param appInstance The app instance identifier
     * @param blockNumber The block number
     * @param sequences Array of sequence numbers to prove
     * @param sequence1 Optional first sub-proof sequences
     * @param sequence2 Optional second sub-proof sequences
     * @return success True if proof was started successfully
     */
    function startProving(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences,
        uint64[] calldata sequence1,
        uint64[] calldata sequence2
    ) external whenNotPaused onlyInitialized returns (bool success) {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        return proofManager.startProving(appInstance, blockNumber, sequences, sequence1, sequence2);
    }

    /**
     * @notice Submit a calculated proof
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
    ) external whenNotPaused onlyInitialized returns (bool isBlockProof) {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        return proofManager.submitProof(
            appInstance,
            blockNumber,
            sequences,
            sequence1,
            sequence2,
            jobId,
            daHash,
            cpuCores,
            proverArchitecture,
            proverMemory,
            cpuTime
        );
    }

    /**
     * @notice Reject a proof as invalid
     */
    function rejectProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external whenNotPaused onlyInitialized {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        proofManager.rejectProof(appInstance, blockNumber, sequences);
    }

    /**
     * @notice Get a proof calculation
     */
    function getProofCalculation(
        string calldata appInstance,
        uint64 blockNumber
    ) external view onlyInitialized returns (
        uint64 blockNum,
        uint64 startSeq,
        uint64 endSeq,
        string memory blockProof,
        bool isFinished
    ) {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        return proofManager.getProofCalculation(appInstance, blockNumber);
    }

    /**
     * @notice Get a specific proof
     */
    function getProof(
        string calldata appInstance,
        uint64 blockNumber,
        uint64[] calldata sequences
    ) external view onlyInitialized returns (
        uint8 status,
        string memory daHash,
        uint64[] memory sequence1,
        uint64[] memory sequence2,
        uint16 rejectedCount,
        uint256 timestamp,
        address prover,
        address user,
        string memory jobId
    ) {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        return proofManager.getProof(appInstance, blockNumber, sequences);
    }

    /**
     * @notice Check if proof calculation is finished
     */
    function isProofCalculationFinished(
        string calldata appInstance,
        uint64 blockNumber
    ) external view onlyInitialized returns (bool finished) {
        require(address(proofManager) != address(0), "SilvanaCoordination: ProofManager not set");
        return proofManager.isProofCalculationFinished(appInstance, blockNumber);
    }

    // ============ Settlement Functions ============

    /**
     * @notice Create a new settlement for a chain
     */
    function createSettlement(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.createSettlement(appInstance, chain, settlementAddress);
    }

    /**
     * @notice Set or update block settlement transaction details
     */
    function setBlockSettlementTx(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash,
        bool txIncluded,
        uint64 sentAt,
        uint64 settledAt
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.setBlockSettlementTx(appInstance, chain, blockNumber, txHash, txIncluded, sentAt, settledAt);
    }

    /**
     * @notice Update block settlement tx hash
     */
    function updateBlockSettlementTxHash(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata txHash
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.updateBlockSettlementTxHash(appInstance, chain, blockNumber, txHash);
    }

    /**
     * @notice Update block settlement tx included status
     */
    function updateBlockSettlementTxIncluded(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.updateBlockSettlementTxIncluded(appInstance, chain, blockNumber);
    }

    /**
     * @notice Set the settlement address for a chain
     */
    function setSettlementAddress(
        string calldata appInstance,
        string calldata chain,
        string calldata settlementAddress
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.setSettlementAddress(appInstance, chain, settlementAddress);
    }

    /**
     * @notice Set the active settlement job for a chain
     */
    function setSettlementJob(
        string calldata appInstance,
        string calldata chain,
        uint64 jobId
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.setSettlementJob(appInstance, chain, jobId);
    }

    /**
     * @notice Get settlement info for a chain
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
    ) {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        return settlementManager.getSettlement(appInstance, chain);
    }

    /**
     * @notice Get block settlement details
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
    ) {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        return settlementManager.getBlockSettlement(appInstance, chain, blockNumber);
    }

    /**
     * @notice Get all chains that have settlements for an app instance
     */
    function getSettlementChains(
        string calldata appInstance
    ) external view returns (string[] memory chains) {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        return settlementManager.getSettlementChains(appInstance);
    }

    /**
     * @notice Get settlement address for a chain
     */
    function getSettlementAddress(
        string calldata appInstance,
        string calldata chain
    ) external view returns (string memory) {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        return settlementManager.getSettlementAddress(appInstance, chain);
    }

    /**
     * @notice Get active settlement job for a chain
     */
    function getSettlementJobForChain(
        string calldata appInstance,
        string calldata chain
    ) external view returns (uint64 jobId) {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        return settlementManager.getSettlementJobForChain(appInstance, chain);
    }

    /**
     * @notice Purge a block settlement (admin only)
     */
    function purgeBlockSettlement(
        string calldata appInstance,
        string calldata chain,
        uint64 blockNumber,
        string calldata reason
    ) external whenNotPaused onlyInitialized {
        require(address(settlementManager) != address(0), "SilvanaCoordination: SettlementManager not set");
        settlementManager.purgeBlockSettlement(appInstance, chain, blockNumber, reason);
    }

    // ============ Storage Functions ============

    /**
     * @notice Set a string value in KV storage
     */
    function setKvString(
        string calldata appInstance,
        string calldata key,
        string calldata value
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.setKvString(appInstance, key, value);
    }

    /**
     * @notice Get a string value from KV storage
     */
    function getKvString(
        string calldata appInstance,
        string calldata key
    ) external view returns (string memory value, bool exists) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.getKvString(appInstance, key);
    }

    /**
     * @notice Get all string key-value pairs
     */
    function getAllKvString(
        string calldata appInstance
    ) external view returns (string[] memory keys, string[] memory values) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.getAllKvString(appInstance);
    }

    /**
     * @notice List all string keys
     */
    function listKvStringKeys(
        string calldata appInstance
    ) external view returns (string[] memory keys) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.listKvStringKeys(appInstance);
    }

    /**
     * @notice Delete a string key-value pair
     */
    function deleteKvString(
        string calldata appInstance,
        string calldata key
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.deleteKvString(appInstance, key);
    }

    /**
     * @notice Set a binary value in KV storage
     */
    function setKvBinary(
        string calldata appInstance,
        bytes calldata key,
        bytes calldata value
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.setKvBinary(appInstance, key, value);
    }

    /**
     * @notice Get a binary value from KV storage
     */
    function getKvBinary(
        string calldata appInstance,
        bytes calldata key
    ) external view returns (bytes memory value, bool exists) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.getKvBinary(appInstance, key);
    }

    /**
     * @notice List all binary keys
     */
    function listKvBinaryKeys(
        string calldata appInstance
    ) external view returns (bytes[] memory keys) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.listKvBinaryKeys(appInstance);
    }

    /**
     * @notice Delete a binary key-value pair
     */
    function deleteKvBinary(
        string calldata appInstance,
        bytes calldata key
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.deleteKvBinary(appInstance, key);
    }

    /**
     * @notice Set metadata for an app instance
     */
    function setMetadata(
        string calldata appInstance,
        string calldata key,
        string calldata value
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.setMetadata(appInstance, key, value);
    }

    /**
     * @notice Get metadata for an app instance
     */
    function getMetadata(
        string calldata appInstance,
        string calldata key
    ) external view returns (string memory value, bool exists) {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        return storageManager.getMetadata(appInstance, key);
    }

    /**
     * @notice Delete metadata for an app instance
     */
    function deleteMetadata(
        string calldata appInstance,
        string calldata key
    ) external whenNotPaused onlyInitialized {
        require(address(storageManager) != address(0), "SilvanaCoordination: StorageManager not set");
        storageManager.deleteMetadata(appInstance, key);
    }

    // ============ Internal Functions ============

    /**
     * @dev Convert uint256 to string
     */
    function _uint256ToString(uint256 value) internal pure returns (string memory) {
        if (value == 0) {
            return "0";
        }

        uint256 temp = value;
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
}