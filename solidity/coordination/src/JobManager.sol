// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "./interfaces/IJobManager.sol";
import "./libraries/DataTypes.sol";
import "./AccessControl.sol";

/**
 * @title JobManager
 * @notice Manages job lifecycle for the Silvana Coordination Layer
 * @dev Implements job creation, execution, and completion tracking
 */
contract JobManager is IJobManager, AccessControl {
    using DataTypes for DataTypes.Job;

    // ============ Constants ============

    uint8 private constant DEFAULT_MAX_ATTEMPTS = 3;
    uint64 private constant MIN_INTERVAL_MS = 60000; // 1 minute minimum for periodic jobs

    // ============ Storage ============

    // appInstance => jobId => Job
    mapping(string => mapping(uint256 => DataTypes.Job)) private jobs;

    // appInstance => next job ID
    mapping(string => uint256) private nextJobId;

    // appInstance => next job sequence
    mapping(string => uint64) private nextJobSequence;

    // appInstance => array of pending job IDs
    mapping(string => uint256[]) private pendingJobs;

    // appInstance => array of active job IDs
    mapping(string => uint256[]) private activeJobs;

    // appInstance => array of failed job IDs
    mapping(string => uint256[]) private failedJobs;

    // appInstance => array of completed job IDs
    mapping(string => uint256[]) private completedJobs;

    // appInstance => agent => array of job IDs
    mapping(string => mapping(address => uint256[])) private agentJobs;

    // appInstance => max attempts
    mapping(string => uint8) private maxAttempts;

    // Coordination contract address (for access control)
    address public coordination;

    // Pause state
    bool private _paused;

    // ============ Modifiers ============

    modifier whenNotPaused() {
        require(!_paused, "JobManager: paused");
        _;
    }

    modifier onlyCoordinator() {
        require(
            hasRole(COORDINATOR_ROLE, msg.sender) || hasRole(ADMIN_ROLE, msg.sender),
            "JobManager: caller is not coordinator"
        );
        _;
    }

    modifier validAppInstance(string calldata appInstance) {
        require(bytes(appInstance).length > 0, "JobManager: empty app instance");
        _;
    }

    // ============ Constructor ============

    constructor() AccessControl() {
        coordination = msg.sender;
    }

    // ============ External Functions - Write ============

    /**
     * @inheritdoc IJobManager
     */
    function createJob(DataTypes.JobInput calldata input)
        external
        override
        whenNotPaused
        validAppInstance(input.appInstance)
        returns (uint256 jobId)
    {
        // Validate input
        require(bytes(input.developer).length > 0, "JobManager: empty developer");
        require(bytes(input.agent).length > 0, "JobManager: empty agent");
        require(bytes(input.agentMethod).length > 0, "JobManager: empty agent method");

        if (input.intervalMs > 0) {
            require(input.intervalMs >= MIN_INTERVAL_MS, "JobManager: interval too short");
        }

        // Get next IDs
        jobId = nextJobId[input.appInstance]++;
        uint64 sequence = nextJobSequence[input.appInstance]++;

        // Create job
        DataTypes.Job storage job = jobs[input.appInstance][jobId];
        job.id = jobId;
        job.jobSequence = sequence;
        job.description = input.description;
        job.developer = input.developer;
        job.agent = input.agent;
        job.agentMethod = input.agentMethod;
        job.app = input.app;
        job.appInstance = input.appInstance;
        job.appInstanceMethod = input.appInstanceMethod;
        job.data = input.data;
        job.status = DataTypes.JobStatus.Pending;
        job.attempts = 0;
        job.intervalMs = input.intervalMs;
        job.createdAt = block.timestamp;
        job.updatedAt = block.timestamp;

        // Set next scheduled time for periodic jobs
        if (input.intervalMs > 0) {
            job.nextScheduledAt = uint64(block.timestamp * 1000) + input.intervalMs;
        }

        // Add to pending jobs
        pendingJobs[input.appInstance].push(jobId);

        // Emit event
        emit DataTypes.JobCreated(
            input.appInstance,
            jobId,
            sequence,
            input.developer,
            input.agent,
            input.agentMethod,
            block.timestamp
        );

        return jobId;
    }

    /**
     * @inheritdoc IJobManager
     */
    function takeJob(string calldata appInstance, uint256 jobId)
        external
        override
        whenNotPaused
        onlyCoordinator
        validAppInstance(appInstance)
    {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists and is pending
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        if (job.status != DataTypes.JobStatus.Pending) {
            revert DataTypes.JobNotPending(appInstance, jobId);
        }

        // Update job status
        job.status = DataTypes.JobStatus.Running;
        job.takenBy = msg.sender;
        job.updatedAt = block.timestamp;
        job.attempts++;

        // Move from pending to active
        _removeFromPending(appInstance, jobId);
        activeJobs[appInstance].push(jobId);
        agentJobs[appInstance][msg.sender].push(jobId);

        // Emit event
        emit DataTypes.JobTaken(appInstance, jobId, msg.sender, block.timestamp);
    }

    /**
     * @inheritdoc IJobManager
     */
    function completeJob(string calldata appInstance, uint256 jobId, bytes calldata output)
        external
        override
        whenNotPaused
        onlyCoordinator
        validAppInstance(appInstance)
    {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists and is running
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        require(job.status == DataTypes.JobStatus.Running, "JobManager: job not running");
        require(job.takenBy == msg.sender, "JobManager: not job taker");

        // Update job
        job.status = DataTypes.JobStatus.Completed;
        job.output = output;
        job.updatedAt = block.timestamp;

        // Move from active to completed
        _removeFromActive(appInstance, jobId);
        completedJobs[appInstance].push(jobId);

        // Handle periodic job rescheduling
        if (job.intervalMs > 0) {
            _rescheduleJob(appInstance, jobId);
        }

        // Emit event
        emit DataTypes.JobCompleted(appInstance, jobId, msg.sender, output, block.timestamp);
    }

    /**
     * @inheritdoc IJobManager
     */
    function failJob(string calldata appInstance, uint256 jobId, string calldata error)
        external
        override
        whenNotPaused
        onlyCoordinator
        validAppInstance(appInstance)
    {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists and is running
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        require(job.status == DataTypes.JobStatus.Running, "JobManager: job not running");
        require(job.takenBy == msg.sender, "JobManager: not job taker");

        // Get max attempts for this app instance
        uint8 maxAttemptCount = maxAttempts[appInstance];
        if (maxAttemptCount == 0) {
            maxAttemptCount = DEFAULT_MAX_ATTEMPTS;
        }

        // Update job status
        job.updatedAt = block.timestamp;

        if (job.attempts < maxAttemptCount) {
            // Return to pending for retry
            job.status = DataTypes.JobStatus.Pending;
            job.takenBy = address(0);

            // Move from active to pending
            _removeFromActive(appInstance, jobId);
            pendingJobs[appInstance].push(jobId);
        } else {
            // Mark as failed after max attempts
            job.status = DataTypes.JobStatus.Failed;

            // Move from active to failed
            _removeFromActive(appInstance, jobId);
            failedJobs[appInstance].push(jobId);
        }

        // Emit event
        emit DataTypes.JobFailed(appInstance, jobId, msg.sender, error, job.attempts, block.timestamp);
    }

    /**
     * @inheritdoc IJobManager
     */
    function terminateJob(string calldata appInstance, uint256 jobId)
        external
        override
        whenNotPaused
        onlyRole(ADMIN_ROLE)
        validAppInstance(appInstance)
    {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }

        // Remove from appropriate list based on status
        if (job.status == DataTypes.JobStatus.Pending) {
            _removeFromPending(appInstance, jobId);
        } else if (job.status == DataTypes.JobStatus.Running) {
            _removeFromActive(appInstance, jobId);
        }

        // Update job
        job.status = DataTypes.JobStatus.Failed;
        job.updatedAt = block.timestamp;

        // Add to failed jobs
        failedJobs[appInstance].push(jobId);

        // Emit event
        emit DataTypes.JobTerminated(appInstance, jobId, msg.sender, block.timestamp);
    }

    /**
     * @inheritdoc IJobManager
     */
    function cancelJob(string calldata appInstance, uint256 jobId)
        external
        override
        whenNotPaused
        validAppInstance(appInstance)
    {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists and caller is authorized
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        require(job.status == DataTypes.JobStatus.Pending, "JobManager: job not pending");

        // TODO: Check if caller is app instance owner
        require(hasRole(ADMIN_ROLE, msg.sender), "JobManager: not authorized");

        // Remove from pending
        _removeFromPending(appInstance, jobId);

        // Delete job
        delete jobs[appInstance][jobId];
    }

    /**
     * @inheritdoc IJobManager
     */
    function restartFailedJobs(string calldata appInstance)
        external
        override
        whenNotPaused
        onlyRole(OPERATOR_ROLE)
        validAppInstance(appInstance)
        returns (uint256 count)
    {
        uint256[] storage failed = failedJobs[appInstance];
        count = failed.length;

        for (uint256 i = 0; i < count; i++) {
            uint256 jobId = failed[i];
            DataTypes.Job storage job = jobs[appInstance][jobId];

            // Reset job to pending
            job.status = DataTypes.JobStatus.Pending;
            job.attempts = 0;
            job.takenBy = address(0);
            job.updatedAt = block.timestamp;

            // Add to pending
            pendingJobs[appInstance].push(jobId);
        }

        // Clear failed jobs array
        delete failedJobs[appInstance];

        return count;
    }

    /**
     * @inheritdoc IJobManager
     */
    function rescheduleJob(string calldata appInstance, uint256 jobId)
        external
        override
        whenNotPaused
        onlyRole(OPERATOR_ROLE)
        validAppInstance(appInstance)
    {
        _rescheduleJob(appInstance, jobId);
    }

    /**
     * @inheritdoc IJobManager
     */
    function setMaxAttempts(string calldata appInstance, uint8 _maxAttempts)
        external
        override
        onlyRole(ADMIN_ROLE)
        validAppInstance(appInstance)
    {
        require(_maxAttempts > 0, "JobManager: invalid max attempts");
        maxAttempts[appInstance] = _maxAttempts;
    }

    /**
     * @inheritdoc IJobManager
     */
    function multicallJobs(
        uint256[] calldata completeJobs,
        bytes[] calldata completeOutputs,
        uint256[] calldata failJobs,
        string[] calldata failErrors,
        uint256[] calldata terminateJobs,
        uint256[] calldata takeJobs,
        string calldata appInstance
    ) external override onlyRole(COORDINATOR_ROLE) validAppInstance(appInstance) {
        // Validate array lengths
        require(
            completeJobs.length == completeOutputs.length,
            "JobManager: complete arrays length mismatch"
        );
        require(
            failJobs.length == failErrors.length,
            "JobManager: fail arrays length mismatch"
        );

        // Process each operation type
        _processCompleteJobs(appInstance, completeJobs, completeOutputs);
        _processFailJobs(appInstance, failJobs, failErrors);
        _processTerminateJobs(appInstance, terminateJobs);
        _processTakeJobs(appInstance, takeJobs);
    }

    // ============ External Functions - Read ============

    /**
     * @inheritdoc IJobManager
     */
    function getJob(string calldata appInstance, uint256 jobId)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job memory)
    {
        DataTypes.Job memory job = jobs[appInstance][jobId];
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        return job;
    }

    /**
     * @inheritdoc IJobManager
     */
    function getPendingJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        return _getJobsFromArray(appInstance, pendingJobs[appInstance], limit, offset);
    }

    /**
     * @inheritdoc IJobManager
     */
    function getActiveJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        return _getJobsFromArray(appInstance, activeJobs[appInstance], limit, offset);
    }

    /**
     * @inheritdoc IJobManager
     */
    function getFailedJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        return _getJobsFromArray(appInstance, failedJobs[appInstance], limit, offset);
    }

    /**
     * @inheritdoc IJobManager
     */
    function getCompletedJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        return _getJobsFromArray(appInstance, completedJobs[appInstance], limit, offset);
    }

    /**
     * @inheritdoc IJobManager
     */
    function getJobsByStatus(string calldata appInstance, DataTypes.JobStatus status, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        if (status == DataTypes.JobStatus.Pending) {
            return _getJobsFromArray(appInstance, pendingJobs[appInstance], limit, offset);
        } else if (status == DataTypes.JobStatus.Running) {
            return _getJobsFromArray(appInstance, activeJobs[appInstance], limit, offset);
        } else if (status == DataTypes.JobStatus.Failed) {
            return _getJobsFromArray(appInstance, failedJobs[appInstance], limit, offset);
        } else if (status == DataTypes.JobStatus.Completed) {
            return _getJobsFromArray(appInstance, completedJobs[appInstance], limit, offset);
        } else {
            // Return empty array for unknown status
            return new DataTypes.Job[](0);
        }
    }

    /**
     * @inheritdoc IJobManager
     */
    function getJobsForAgent(string calldata appInstance, address agent, uint256 limit, uint256 offset)
        external
        view
        override
        validAppInstance(appInstance)
        returns (DataTypes.Job[] memory)
    {
        return _getJobsFromArray(appInstance, agentJobs[appInstance][agent], limit, offset);
    }

    /**
     * @inheritdoc IJobManager
     */
    function getJobCount(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint256)
    {
        return nextJobId[appInstance];
    }

    /**
     * @inheritdoc IJobManager
     */
    function getPendingJobCount(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint256)
    {
        return pendingJobs[appInstance].length;
    }

    /**
     * @inheritdoc IJobManager
     */
    function getFailedJobCount(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint256)
    {
        return failedJobs[appInstance].length;
    }

    /**
     * @inheritdoc IJobManager
     */
    function getNextJobId(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint256)
    {
        return nextJobId[appInstance];
    }

    /**
     * @inheritdoc IJobManager
     */
    function getNextJobSequence(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint64)
    {
        return nextJobSequence[appInstance];
    }

    /**
     * @inheritdoc IJobManager
     */
    function jobExists(string calldata appInstance, uint256 jobId)
        external
        view
        override
        validAppInstance(appInstance)
        returns (bool)
    {
        return jobs[appInstance][jobId].id == jobId;
    }

    /**
     * @inheritdoc IJobManager
     */
    function canTakeJob(string calldata appInstance, uint256 jobId)
        external
        view
        override
        validAppInstance(appInstance)
        returns (bool)
    {
        DataTypes.Job memory job = jobs[appInstance][jobId];
        return job.id == jobId && job.status == DataTypes.JobStatus.Pending;
    }

    /**
     * @inheritdoc IJobManager
     */
    function getMaxAttempts(string calldata appInstance)
        external
        view
        override
        validAppInstance(appInstance)
        returns (uint8)
    {
        uint8 max = maxAttempts[appInstance];
        return max == 0 ? DEFAULT_MAX_ATTEMPTS : max;
    }

    // ============ Internal Functions ============

    /**
     * @dev Process complete job operations in batch
     * @param appInstance The app instance identifier
     * @param jobIds Array of job IDs to complete
     * @param outputs Array of output data for each job
     */
    function _processCompleteJobs(
        string calldata appInstance,
        uint256[] calldata jobIds,
        bytes[] calldata outputs
    ) internal {
        for (uint256 i = 0; i < jobIds.length; i++) {
            uint256 jobId = jobIds[i];
            DataTypes.Job storage job = jobs[appInstance][jobId];

            // Validate job exists and is in running state
            if (job.id != jobId) {
                continue;
            }

            if (job.status != DataTypes.JobStatus.Running) {
                continue;
            }

            if (job.takenBy != msg.sender) {
                continue;
            }

            // Complete the job
            job.status = DataTypes.JobStatus.Completed;
            job.output = outputs[i];
            job.updatedAt = block.timestamp;

            _removeFromActive(appInstance, jobId);

            emit DataTypes.JobCompleted(appInstance, jobId, msg.sender, outputs[i], block.timestamp);

            // Handle periodic jobs - create new job with same parameters
            if (job.intervalMs > 0) {
                _createPeriodicJob(appInstance, job);
            }
        }
    }

    /**
     * @dev Process fail job operations in batch
     * @param appInstance The app instance identifier
     * @param jobIds Array of job IDs to fail
     * @param errors Array of error messages for each job
     */
    function _processFailJobs(
        string calldata appInstance,
        uint256[] calldata jobIds,
        string[] calldata errors
    ) internal {
        for (uint256 i = 0; i < jobIds.length; i++) {
            uint256 jobId = jobIds[i];
            DataTypes.Job storage job = jobs[appInstance][jobId];

            // Validate job exists and is in running state
            if (job.id != jobId) {
                continue;
            }

            if (job.status != DataTypes.JobStatus.Running) {
                continue;
            }

            if (job.takenBy != msg.sender) {
                continue;
            }

            _removeFromActive(appInstance, jobId);

            // Check if job should retry
            uint8 max = maxAttempts[appInstance];
            if (max == 0) max = DEFAULT_MAX_ATTEMPTS;

            if (job.attempts < max) {
                // Return to pending for retry
                job.status = DataTypes.JobStatus.Pending;
                job.takenBy = address(0);
                job.updatedAt = block.timestamp;
                pendingJobs[appInstance].push(jobId);
                emit DataTypes.JobFailed(appInstance, jobId, msg.sender, errors[i], job.attempts, block.timestamp);
            } else {
                // Mark as permanently failed
                job.status = DataTypes.JobStatus.Failed;
                job.output = abi.encode(errors[i]);
                job.updatedAt = block.timestamp;
                failedJobs[appInstance].push(jobId);
                emit DataTypes.JobFailed(appInstance, jobId, msg.sender, errors[i], job.attempts, block.timestamp);
            }
        }
    }

    /**
     * @dev Process terminate job operations in batch
     * @param appInstance The app instance identifier
     * @param jobIds Array of job IDs to terminate
     */
    function _processTerminateJobs(
        string calldata appInstance,
        uint256[] calldata jobIds
    ) internal {
        for (uint256 i = 0; i < jobIds.length; i++) {
            uint256 jobId = jobIds[i];
            DataTypes.Job storage job = jobs[appInstance][jobId];

            // Validate job exists
            if (job.id != jobId) {
                continue;
            }

            // Remove from appropriate list
            if (job.status == DataTypes.JobStatus.Pending) {
                _removeFromPending(appInstance, jobId);
            } else if (job.status == DataTypes.JobStatus.Running) {
                _removeFromActive(appInstance, jobId);
            } else if (job.status == DataTypes.JobStatus.Failed) {
                _removeFromFailed(appInstance, jobId);
            }

            // Delete the job
            delete jobs[appInstance][jobId];

            emit DataTypes.JobTerminated(appInstance, jobId, msg.sender, block.timestamp);
        }
    }

    /**
     * @dev Process take job operations in batch
     * @param appInstance The app instance identifier
     * @param jobIds Array of job IDs to take
     */
    function _processTakeJobs(
        string calldata appInstance,
        uint256[] calldata jobIds
    ) internal {
        for (uint256 i = 0; i < jobIds.length; i++) {
            uint256 jobId = jobIds[i];
            DataTypes.Job storage job = jobs[appInstance][jobId];

            // Validate job exists and is pending
            if (job.id != jobId) {
                continue;
            }

            if (job.status != DataTypes.JobStatus.Pending) {
                continue;
            }

            // Check if periodic job is scheduled for future
            if (job.intervalMs > 0 && job.nextScheduledAt > uint64(block.timestamp * 1000)) {
                continue;
            }

            // Take the job
            job.status = DataTypes.JobStatus.Running;
            job.takenBy = msg.sender;
            job.attempts++;
            job.updatedAt = block.timestamp;

            _removeFromPending(appInstance, jobId);
            activeJobs[appInstance].push(jobId);

            emit DataTypes.JobTaken(appInstance, jobId, msg.sender, block.timestamp);
        }
    }

    /**
     * @dev Create a new periodic job with the same parameters as the completed one
     * @param appInstance The app instance identifier
     * @param completedJob The completed periodic job
     */
    function _createPeriodicJob(string memory appInstance, DataTypes.Job storage completedJob) internal {
        uint256 newJobId = nextJobId[appInstance]++;
        uint64 newSequence = nextJobSequence[appInstance]++;

        DataTypes.Job storage newJob = jobs[appInstance][newJobId];
        newJob.id = newJobId;
        newJob.jobSequence = newSequence;
        newJob.description = completedJob.description;
        newJob.developer = completedJob.developer;
        newJob.agent = completedJob.agent;
        newJob.agentMethod = completedJob.agentMethod;
        newJob.app = completedJob.app;
        newJob.appInstance = completedJob.appInstance;
        newJob.appInstanceMethod = completedJob.appInstanceMethod;
        newJob.blockNumber = completedJob.blockNumber;
        newJob.sequences = completedJob.sequences;
        newJob.data = completedJob.data;
        newJob.status = DataTypes.JobStatus.Pending;
        newJob.attempts = 0;
        newJob.takenBy = address(0);
        newJob.intervalMs = completedJob.intervalMs;
        newJob.nextScheduledAt = uint64(block.timestamp * 1000) + completedJob.intervalMs;
        newJob.createdAt = block.timestamp;
        newJob.updatedAt = block.timestamp;

        pendingJobs[appInstance].push(newJobId);

        emit DataTypes.JobCreated(
            appInstance,
            newJobId,
            newSequence,
            completedJob.developer,
            completedJob.agent,
            completedJob.agentMethod,
            block.timestamp
        );
    }

    /**
     * @dev Remove a job from the pending array
     */
    function _removeFromPending(string memory appInstance, uint256 jobId) internal {
        uint256[] storage pending = pendingJobs[appInstance];
        for (uint256 i = 0; i < pending.length; i++) {
            if (pending[i] == jobId) {
                pending[i] = pending[pending.length - 1];
                pending.pop();
                break;
            }
        }
    }

    /**
     * @dev Remove a job from the active array
     */
    function _removeFromActive(string memory appInstance, uint256 jobId) internal {
        uint256[] storage active = activeJobs[appInstance];
        for (uint256 i = 0; i < active.length; i++) {
            if (active[i] == jobId) {
                active[i] = active[active.length - 1];
                active.pop();
                break;
            }
        }
    }

    /**
     * @dev Remove a job from the failed array
     */
    function _removeFromFailed(string memory appInstance, uint256 jobId) internal {
        uint256[] storage failed = failedJobs[appInstance];
        for (uint256 i = 0; i < failed.length; i++) {
            if (failed[i] == jobId) {
                failed[i] = failed[failed.length - 1];
                failed.pop();
                break;
            }
        }
    }

    /**
     * @dev Reschedule a periodic job
     */
    function _rescheduleJob(string memory appInstance, uint256 jobId) internal {
        DataTypes.Job storage job = jobs[appInstance][jobId];

        // Validate job exists and is periodic
        if (job.id != jobId) {
            revert DataTypes.JobNotFound(appInstance, jobId);
        }
        require(job.intervalMs > 0, "JobManager: not a periodic job");

        // Calculate next scheduled time
        uint64 nextTime = uint64(block.timestamp * 1000) + job.intervalMs;

        // Create new job with same parameters
        uint256 newJobId = nextJobId[appInstance]++;
        uint64 sequence = nextJobSequence[appInstance]++;

        DataTypes.Job storage newJob = jobs[appInstance][newJobId];
        newJob.id = newJobId;
        newJob.jobSequence = sequence;
        newJob.description = job.description;
        newJob.developer = job.developer;
        newJob.agent = job.agent;
        newJob.agentMethod = job.agentMethod;
        newJob.app = job.app;
        newJob.appInstance = job.appInstance;
        newJob.appInstanceMethod = job.appInstanceMethod;
        newJob.data = job.data;
        newJob.status = DataTypes.JobStatus.Pending;
        newJob.attempts = 0;
        newJob.intervalMs = job.intervalMs;
        newJob.nextScheduledAt = nextTime;
        newJob.createdAt = block.timestamp;
        newJob.updatedAt = block.timestamp;

        // Add to pending jobs
        pendingJobs[appInstance].push(newJobId);

        // Emit event
        emit DataTypes.JobCreated(
            appInstance,
            newJobId,
            sequence,
            job.developer,
            job.agent,
            job.agentMethod,
            block.timestamp
        );
    }

    /**
     * @dev Get jobs from an array with pagination
     */
    function _getJobsFromArray(
        string memory appInstance,
        uint256[] storage jobIds,
        uint256 limit,
        uint256 offset
    ) internal view returns (DataTypes.Job[] memory) {
        uint256 total = jobIds.length;

        if (offset >= total) {
            return new DataTypes.Job[](0);
        }

        uint256 end = offset + limit;
        if (end > total) {
            end = total;
        }

        uint256 count = end - offset;
        DataTypes.Job[] memory result = new DataTypes.Job[](count);

        for (uint256 i = 0; i < count; i++) {
            result[i] = jobs[appInstance][jobIds[offset + i]];
        }

        return result;
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