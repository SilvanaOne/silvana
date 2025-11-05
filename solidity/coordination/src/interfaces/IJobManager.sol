// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

import "../libraries/DataTypes.sol";

/**
 * @title IJobManager
 * @notice Interface for job management operations
 * @dev Defines the interface for the JobManager contract
 */
interface IJobManager {
    // ============ Write Functions ============

    /**
     * @notice Create a new job
     * @param input Job input data
     * @return jobId The created job ID
     */
    function createJob(DataTypes.JobInput calldata input) external returns (uint256 jobId);

    /**
     * @notice Take a pending job for execution
     * @param appInstance The app instance identifier
     * @param jobId The job ID to take
     */
    function takeJob(string calldata appInstance, uint256 jobId) external;

    /**
     * @notice Complete a job with output
     * @param appInstance The app instance identifier
     * @param jobId The job ID to complete
     * @param output The job output data
     */
    function completeJob(string calldata appInstance, uint256 jobId, bytes calldata output) external;

    /**
     * @notice Mark a job as failed
     * @param appInstance The app instance identifier
     * @param jobId The job ID to fail
     * @param error The error message
     */
    function failJob(string calldata appInstance, uint256 jobId, string calldata error) external;

    /**
     * @notice Terminate a stuck job
     * @param appInstance The app instance identifier
     * @param jobId The job ID to terminate
     */
    function terminateJob(string calldata appInstance, uint256 jobId) external;

    /**
     * @notice Cancel a pending job
     * @param appInstance The app instance identifier
     * @param jobId The job ID to cancel
     */
    function cancelJob(string calldata appInstance, uint256 jobId) external;

    /**
     * @notice Restart failed jobs for an app instance
     * @param appInstance The app instance identifier
     * @return count Number of jobs restarted
     */
    function restartFailedJobs(string calldata appInstance) external returns (uint256 count);

    /**
     * @notice Reschedule a periodic job
     * @param appInstance The app instance identifier
     * @param jobId The job ID to reschedule
     */
    function rescheduleJob(string calldata appInstance, uint256 jobId) external;

    // ============ Multicall Functions ============

    /**
     * @notice Execute multiple job operations in a single transaction
     * @param completeJobs Array of job IDs to complete
     * @param completeOutputs Array of outputs for completed jobs
     * @param failJobs Array of job IDs to fail
     * @param failErrors Array of error messages for failed jobs
     * @param terminateJobs Array of job IDs to terminate
     * @param takeJobs Array of job IDs to take
     * @param appInstance The app instance identifier
     */
    function multicallJobs(
        uint256[] calldata completeJobs,
        bytes[] calldata completeOutputs,
        uint256[] calldata failJobs,
        string[] calldata failErrors,
        uint256[] calldata terminateJobs,
        uint256[] calldata takeJobs,
        string calldata appInstance
    ) external;

    // ============ Read Functions ============

    /**
     * @notice Get a job by ID
     * @param appInstance The app instance identifier
     * @param jobId The job ID
     * @return job The job data
     */
    function getJob(string calldata appInstance, uint256 jobId) external view returns (DataTypes.Job memory job);

    /**
     * @notice Get pending jobs for an app instance
     * @param appInstance The app instance identifier
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of pending jobs
     */
    function getPendingJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get active jobs for an app instance
     * @param appInstance The app instance identifier
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of active jobs
     */
    function getActiveJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get failed jobs for an app instance
     * @param appInstance The app instance identifier
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of failed jobs
     */
    function getFailedJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get completed jobs for an app instance
     * @param appInstance The app instance identifier
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of completed jobs
     */
    function getCompletedJobs(string calldata appInstance, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get jobs by status
     * @param appInstance The app instance identifier
     * @param status The job status to filter by
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of jobs with the specified status
     */
    function getJobsByStatus(string calldata appInstance, DataTypes.JobStatus status, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get jobs for a specific agent
     * @param appInstance The app instance identifier
     * @param agent The agent address
     * @param limit Maximum number of jobs to return
     * @param offset Starting index for pagination
     * @return jobs Array of jobs for the agent
     */
    function getJobsForAgent(string calldata appInstance, address agent, uint256 limit, uint256 offset)
        external
        view
        returns (DataTypes.Job[] memory jobs);

    /**
     * @notice Get the total number of jobs for an app instance
     * @param appInstance The app instance identifier
     * @return count Total number of jobs
     */
    function getJobCount(string calldata appInstance) external view returns (uint256 count);

    /**
     * @notice Get the number of pending jobs for an app instance
     * @param appInstance The app instance identifier
     * @return count Number of pending jobs
     */
    function getPendingJobCount(string calldata appInstance) external view returns (uint256 count);

    /**
     * @notice Get the number of failed jobs for an app instance
     * @param appInstance The app instance identifier
     * @return count Number of failed jobs
     */
    function getFailedJobCount(string calldata appInstance) external view returns (uint256 count);

    /**
     * @notice Get the next job ID for an app instance
     * @param appInstance The app instance identifier
     * @return nextId The next job ID that will be assigned
     */
    function getNextJobId(string calldata appInstance) external view returns (uint256 nextId);

    /**
     * @notice Get the next job sequence for an app instance
     * @param appInstance The app instance identifier
     * @return nextSequence The next job sequence that will be assigned
     */
    function getNextJobSequence(string calldata appInstance) external view returns (uint64 nextSequence);

    /**
     * @notice Check if a job exists
     * @param appInstance The app instance identifier
     * @param jobId The job ID
     * @return exists True if the job exists
     */
    function jobExists(string calldata appInstance, uint256 jobId) external view returns (bool exists);

    /**
     * @notice Check if a job can be taken
     * @param appInstance The app instance identifier
     * @param jobId The job ID
     * @return canTake True if the job can be taken
     */
    function canTakeJob(string calldata appInstance, uint256 jobId) external view returns (bool canTake);

    /**
     * @notice Get maximum job attempts allowed
     * @param appInstance The app instance identifier
     * @return maxAttempts Maximum number of attempts
     */
    function getMaxAttempts(string calldata appInstance) external view returns (uint8 maxAttempts);

    /**
     * @notice Set maximum job attempts for an app instance
     * @param appInstance The app instance identifier
     * @param maxAttempts Maximum number of attempts
     */
    function setMaxAttempts(string calldata appInstance, uint8 maxAttempts) external;
}