// SPDX-License-Identifier: MIT
pragma solidity ^0.8.23;

/**
 * @title DataTypes
 * @notice Core data structures for the Silvana Coordination Layer
 * @dev Defines the main data types used across all coordination contracts
 */
library DataTypes {
    // ============ Enums ============

    /**
     * @notice Job status enum matching the Move implementation
     * @dev Maps to coordination::jobs::JobStatus
     */
    enum JobStatus {
        Pending,    // Job is waiting to be taken
        Running,    // Job is currently being executed
        Completed,  // Job finished successfully
        Failed      // Job execution failed
    }

    // ============ Job Structures ============

    /**
     * @notice Core job structure
     * @dev Maps to coordination::jobs::Job and coordination-trait Job type
     */
    struct Job {
        uint256 id;                    // Unique job identifier
        uint64 jobSequence;             // Sequence number for ordering
        string description;             // Optional job description
        // Agent metadata
        string developer;               // Developer name
        string agent;                   // Agent name
        string agentMethod;             // Agent method to call
        // App metadata
        string app;                     // App name
        string appInstance;             // App instance identifier
        string appInstanceMethod;       // App instance method
        // Block and sequence data
        uint64 blockNumber;             // Associated block number
        uint64[] sequences;             // Sequence numbers
        uint64[] sequences1;            // Additional sequences set 1
        uint64[] sequences2;            // Additional sequences set 2
        bytes data;                     // Job input data
        // Job state
        JobStatus status;               // Current job status
        uint8 attempts;                 // Number of execution attempts
        address takenBy;                // Coordinator who took the job
        bytes output;                   // Job output data (when completed)
        // Scheduling
        uint64 intervalMs;              // Interval for periodic jobs (0 = one-time)
        uint64 nextScheduledAt;         // Next scheduled execution time
        // Timestamps
        uint256 createdAt;              // Creation timestamp
        uint256 updatedAt;              // Last update timestamp
    }

    /**
     * @notice Simplified job input for creation
     * @dev Used for gas-efficient job creation
     */
    struct JobInput {
        string description;
        string developer;
        string agent;
        string agentMethod;
        string app;
        string appInstance;
        string appInstanceMethod;
        bytes data;
        uint64 intervalMs;
    }

    // ============ App Instance Structures ============

    /**
     * @notice App instance structure
     * @dev Maps to coordination::app_instance::AppInstance
     */
    struct AppInstance {
        string id;                      // Unique instance identifier
        string name;                    // Instance name
        address owner;                  // Instance owner address
        string silvanaAppName;          // Associated Silvana app name
        string developerName;           // Developer name
        uint256 sequenceNumber;         // Current sequence number
        bytes32 stateCommitment;        // Current state commitment
        bytes32 actionsCommitment;      // Current actions commitment
        uint64 blockNumber;             // Current block number
        uint64 previousBlockTimestamp;  // Previous block timestamp
        uint64 previousBlockLastSequence; // Last sequence of previous block
        bool isPaused;                  // Whether instance is paused
        uint64 minTimeBetweenBlocks;    // Minimum time between blocks
        uint256 createdAt;              // Creation timestamp
        uint256 updatedAt;              // Last update timestamp
    }

    /**
     * @notice Sequence state for an app instance
     * @dev Maps to coordination::sequence_state::SequenceState
     */
    struct SequenceState {
        uint256 sequenceNumber;         // Current sequence number
        bytes32 actionsCommitment;      // Actions commitment hash
        bytes32 stateCommitment;        // State commitment hash
        uint64 blockNumber;             // Associated block number
        uint256 timestamp;              // Timestamp of this state
    }

    // ============ Block Structures ============

    /**
     * @notice Block structure
     * @dev Maps to coordination::block::Block
     */
    struct Block {
        uint64 blockNumber;             // Block number
        string name;                    // Block name
        uint64 startSequence;           // Starting sequence number
        uint64 endSequence;             // Ending sequence number
        bytes32 actionsCommitment;      // Actions commitment
        bytes32 stateCommitment;        // State commitment
        uint64 timeSinceLastBlock;      // Time since last block (ms)
        uint64 numberOfTransactions;    // Number of transactions
        bytes32 startActionsCommitment; // Starting actions commitment
        bytes32 endActionsCommitment;   // Ending actions commitment
        string stateDataAvailability;   // State DA reference
        string proofDataAvailability;   // Proof DA reference
        uint256 createdAt;              // Creation timestamp
        uint256 stateCalculatedAt;      // State calculation timestamp
        uint256 provedAt;               // Proof timestamp
    }

    // ============ Registry Structures ============

    /**
     * @notice Developer structure
     * @dev Maps to coordination::developer::Developer
     */
    struct Developer {
        string name;                    // Developer name
        string github;                  // GitHub username
        string image;                   // Profile image URL
        string description;             // Developer description
        string site;                    // Website URL
        address owner;                  // Owner address
        string[] agentNames;            // List of agent names
        uint256 createdAt;              // Creation timestamp
        uint256 updatedAt;              // Last update timestamp
        uint32 version;                 // Version number
    }

    /**
     * @notice Agent method structure
     * @dev Maps to coordination::agent::AgentMethod
     */
    struct AgentMethod {
        string dockerImage;             // Docker image URL
        string dockerSha256;            // Docker image SHA256
        uint16 minMemoryGb;             // Minimum memory requirement (GB)
        uint16 minCpuCores;             // Minimum CPU cores
        bool requiresTee;               // TEE requirement flag
    }

    /**
     * @notice Agent structure
     * @dev Maps to coordination::agent::Agent
     */
    struct Agent {
        string name;                    // Agent name
        string image;                   // Agent image URL
        string description;             // Agent description
        string site;                    // Agent website
        string[] chains;                // Supported chains
        AgentMethod defaultMethod;      // Default method
        uint256 createdAt;              // Creation timestamp
        uint256 updatedAt;              // Last update timestamp
        uint32 version;                 // Version number
    }

    // ============ Settlement Structures ============

    /**
     * @notice Block settlement structure
     * @dev Maps to coordination::settlement::BlockSettlement
     */
    struct BlockSettlement {
        uint64 blockNumber;             // Block number
        string settlementTxHash;        // Settlement transaction hash
        bool settlementTxIncludedInBlock; // Whether tx is included
        uint64 sentToSettlementAt;      // Sent to settlement timestamp
        uint64 settledAt;               // Settlement completion timestamp
        bool exists;                    // Whether the block settlement exists
    }

    /**
     * @notice Settlement structure for a chain
     * @dev Maps to coordination::settlement::Settlement
     */
    struct Settlement {
        string chain;                   // Chain identifier
        uint64 lastSettledBlockNumber;  // Last settled block
        string settlementAddress;       // Settlement contract address
        uint64 settlementJob;           // Active settlement job ID (0 if none)
        bool exists;                    // Whether the settlement exists
    }

    // ============ Proof Structures ============

    /**
     * @notice Proof structure
     * @dev Maps to coordination::prover::Proof
     */
    struct Proof {
        uint8 status;                   // Proof status
        string daHash;                  // Data availability hash
        uint64[] sequence1;             // First sequence set
        uint64[] sequence2;             // Second sequence set
        uint16 rejectedCount;           // Number of rejections
        uint256 timestamp;              // Proof timestamp
        address prover;                 // Prover address
        address user;                   // User address
        string jobId;                   // Associated job ID
    }

    /**
     * @notice Proof calculation structure
     * @dev Maps to coordination::prover::ProofCalculation
     */
    struct ProofCalculation {
        uint64 blockNumber;             // Block number
        uint64 startSequence;           // Starting sequence
        uint64 endSequence;             // Ending sequence
        mapping(bytes32 => Proof) proofs; // Sequence hash to proof
        string blockProof;              // Block proof
        bool isFinished;                // Completion flag
    }

    // ============ Events ============

    /**
     * @notice Emitted when a job is created
     */
    event JobCreated(
        string appInstance,
        uint64 indexed jobSequence,
        string developer,
        string agent,
        string agentMethod,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a job is taken by a coordinator
     */
    event JobTaken(
        string indexed appInstance,
        uint64 indexed jobSequence,
        address indexed coordinator,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a job is completed
     */
    event JobCompleted(
        string indexed appInstance,
        uint64 indexed jobSequence,
        address indexed coordinator,
        bytes output,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a job fails
     */
    event JobFailed(
        string indexed appInstance,
        uint64 indexed jobSequence,
        address indexed coordinator,
        string error,
        uint8 attempts,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a job is terminated
     */
    event JobTerminated(
        string indexed appInstance,
        uint64 indexed jobSequence,
        address indexed coordinator,
        uint256 timestamp
    );

    /**
     * @notice Emitted when an app instance is created
     */
    event AppInstanceCreated(
        string indexed instanceId,
        address indexed owner,
        string appName,
        string developerName,
        uint256 timestamp
    );

    /**
     * @notice Emitted when sequence is updated
     */
    event SequenceUpdated(
        string indexed instanceId,
        uint256 sequenceNumber,
        bytes32 stateCommitment,
        bytes32 actionsCommitment,
        uint256 timestamp
    );

    /**
     * @notice Emitted when an app instance is paused
     */
    event AppInstancePaused(
        string indexed instanceId,
        address indexed admin,
        uint256 timestamp
    );

    /**
     * @notice Emitted when an app instance is unpaused
     */
    event AppInstanceUnpaused(
        string indexed instanceId,
        address indexed admin,
        uint256 timestamp
    );

    // ============ Proof Events ============

    /**
     * @notice Emitted when a proof calculation is started
     */
    event ProofStartedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        uint256 timestamp,
        address indexed prover
    );

    /**
     * @notice Emitted when a proof is submitted
     */
    event ProofSubmittedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        string daHash,
        uint256 timestamp,
        address indexed prover,
        uint64 cpuTime,
        uint8 cpuCores,
        uint64 proverMemory,
        string proverArchitecture
    );

    /**
     * @notice Emitted when a proof is used in composition
     */
    event ProofUsedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        string daHash,
        uint256 timestamp,
        address indexed user
    );

    /**
     * @notice Emitted when a proof is reserved for composition
     */
    event ProofReservedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        string daHash,
        uint256 timestamp,
        address indexed user
    );

    /**
     * @notice Emitted when a proof is returned to available state
     */
    event ProofReturnedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a proof is rejected
     */
    event ProofRejectedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64[] sequences,
        uint256 timestamp,
        address indexed verifier
    );

    /**
     * @notice Emitted when a full block proof is completed
     */
    event BlockProofEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64 startSequence,
        uint64 endSequence,
        string daHash,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a proof calculation is created
     */
    event ProofCalculationCreatedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64 startSequence,
        uint64 endSequence,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a proof calculation is finished
     */
    event ProofCalculationFinishedEvent(
        string indexed appInstance,
        uint64 indexed blockNumber,
        uint64 startSequence,
        uint64 endSequence,
        string blockProof,
        bool isFinished,
        uint256 proofsCount,
        uint256 timestamp
    );

    // ============ Errors ============

    /**
     * @notice Error when job is not found
     */
    error JobNotFound(string appInstance, uint256 jobId);

    /**
     * @notice Error when job is not in pending state
     */
    error JobNotPending(string appInstance, uint256 jobId);

    /**
     * @notice Error when job is already taken
     */
    error JobAlreadyTaken(string appInstance, uint256 jobId);

    /**
     * @notice Error when caller is not authorized
     */
    error NotAuthorized(address caller, string requiredRole);

    /**
     * @notice Error when app instance not found
     */
    error AppInstanceNotFound(string instanceId);


    /**
     * @notice Error when invalid input provided
     */
    error InvalidInput(string parameter);

    /**
     * @notice Error when interval is too short for periodic jobs
     */
    error IntervalTooShort(uint64 provided, uint64 minimum);

    /**
     * @notice Error when proof is not calculated or already used
     */
    error ProofNotCalculated(string appInstance, uint64 blockNumber, bytes32 sequenceHash);

    /**
     * @notice Error when sequence is outside valid range for block
     */
    error SequenceOutOfRange(uint64 sequence, uint64 start, uint64 end);

    /**
     * @notice Error when sequences vector is empty
     */
    error SequencesCannotBeEmpty();

    /**
     * @notice Error when proof calculation not found
     */
    error ProofCalculationNotFound(string appInstance, uint64 blockNumber);

    /**
     * @notice Error when proof already started
     */
    error ProofAlreadyStarted(string appInstance, uint64 blockNumber, bytes32 sequenceHash);

    // ============ Settlement Events ============

    /**
     * @notice Emitted when a new settlement is created for a chain
     */
    event SettlementCreatedEvent(
        string indexed appInstance,
        string indexed chain,
        string settlementAddress,
        uint256 timestamp
    );

    /**
     * @notice Emitted when block settlement transaction is updated
     */
    event BlockSettlementTransactionEvent(
        string indexed appInstance,
        string indexed chain,
        uint64 indexed blockNumber,
        string settlementTxHash,
        bool settlementTxIncludedInBlock,
        uint64 sentToSettlementAt,
        uint64 settledAt,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a block is settled on chain
     */
    event BlockSettledEvent(
        string indexed appInstance,
        string indexed chain,
        uint64 indexed blockNumber,
        string settlementTxHash,
        uint64 sentToSettlementAt,
        uint64 settledAt,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a block settlement is purged/cleaned up
     */
    event BlockSettlementPurgedEvent(
        string indexed appInstance,
        string indexed chain,
        uint64 indexed blockNumber,
        string reason,
        uint256 timestamp
    );

    // ============ Settlement Errors ============

    /**
     * @notice Error when settlement not found for chain
     */
    error SettlementNotFound(string appInstance, string chain);

    /**
     * @notice Error when block settlement not found
     */
    error BlockSettlementNotFound(string appInstance, string chain, uint64 blockNumber);

    /**
     * @notice Error when chain is not supported
     */
    error ChainNotSupported(string chain);

    // ============ Storage Events ============

    /**
     * @notice Emitted when a string KV pair is set
     */
    event KvStringSetEvent(
        string indexed appInstance,
        string indexed key,
        string value,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a string KV pair is deleted
     */
    event KvStringDeletedEvent(
        string indexed appInstance,
        string indexed key,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a binary KV pair is set
     * @dev Key is bytes (arbitrary binary data), indexed as keccak256 hash
     */
    event KvBinarySetEvent(
        string indexed appInstance,
        bytes key,
        bytes value,
        uint256 timestamp
    );

    /**
     * @notice Emitted when a binary KV pair is deleted
     * @dev Key is bytes (arbitrary binary data), indexed as keccak256 hash
     */
    event KvBinaryDeletedEvent(
        string indexed appInstance,
        bytes key,
        uint256 timestamp
    );

    // ============ Storage Errors ============

    /**
     * @notice Error when key is not found
     */
    error KeyNotFound(string appInstance, string key);

    /**
     * @notice Error when key is too long
     */
    error KeyTooLong(uint256 length, uint256 maxLength);
}