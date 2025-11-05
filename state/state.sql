-- TiDB State Management Schema
-- This schema implements private state management for Silvana zkRollup system
-- All tables enforce Ed25519 JWT authentication through owner fields and foreign keys
--
-- NOTE: Database name is specified in the connection string (STATE_DATABASE_URL in .env)
-- The mysqldef migration tool connects directly to the target database.
-- Run "CREATE DATABASE IF NOT EXISTS state;" before running this script.
-- ============================================================================
-- Core Tables
-- ============================================================================

-- 1. App Instances Table - Core authorization table
-- All other tables reference this for ownership verification
CREATE TABLE IF NOT EXISTS app_instances (
    `app_instance_id` VARCHAR(255) PRIMARY KEY,
    `owner` VARCHAR(64) NOT NULL,          -- Ed25519 public key (hex), NOT NULL for security
    `name` VARCHAR(255) NOT NULL,
    `description` TEXT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    `metadata` JSON NULL,                  -- Optional metadata
    -- Coordination layer support fields
    `admin` VARCHAR(64) NULL,                          -- Administrator public key
    `is_paused` BOOLEAN DEFAULT FALSE,                 -- Pause functionality
    `min_time_between_blocks` BIGINT UNSIGNED DEFAULT 60,  -- Block timing in seconds
    `block_number` BIGINT UNSIGNED DEFAULT 0,          -- Current block number
    `sequence` BIGINT UNSIGNED DEFAULT 0,              -- Current sequence number
    `last_proved_block_number` BIGINT UNSIGNED DEFAULT 0,  -- Last proved block
    `last_settled_block_number` BIGINT UNSIGNED DEFAULT 0, -- Last settled block
    `last_settled_sequence` BIGINT UNSIGNED DEFAULT 0,     -- Last settled sequence
    `last_purged_sequence` BIGINT UNSIGNED DEFAULT 0,      -- Last purged sequence
    INDEX idx_owner (`owner`),
    INDEX idx_created_at (`created_at`),
    INDEX idx_is_paused (`is_paused`),
    INDEX idx_block_number (`block_number`),
    INDEX idx_sequence (`sequence`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='App instances with Ed25519 ownership for JWT authentication';

-- 2. User Actions Table - Input actions that trigger state changes
CREATE TABLE IF NOT EXISTS user_actions (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `sequence` BIGINT UNSIGNED NOT NULL,
    `action_type` VARCHAR(255) NOT NULL,
    `action_data` BLOB NOT NULL,           -- Action parameters (serialized)
    `action_hash` BINARY(32) NOT NULL,     -- Hash of action data
    `action_da` VARCHAR(255) NULL,         -- Data Availability key for large action data
    `submitter` VARCHAR(255) NOT NULL,     -- Who submitted the action
    `metadata` JSON NULL,                  -- Optional application metadata
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uk_app_sequence (`app_instance_id`, `sequence`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_sequence (`sequence`),
    INDEX idx_created_at (`created_at`),
    INDEX idx_action_type (`action_type`),
    CONSTRAINT fk_actions_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='User actions that trigger state transitions';

-- 3. Optimistic State Table - Fast state calculation without proof
CREATE TABLE IF NOT EXISTS optimistic_state (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `sequence` BIGINT UNSIGNED NOT NULL,
    `state_hash` BINARY(32) NOT NULL,      -- Hash of state data
    `state_data` BLOB NOT NULL,            -- State data (if small)
    `state_da` VARCHAR(255) NULL,          -- Data Availability key for large state data
    `transition_data` BLOB NULL,           -- Transition delta
    `transition_da` VARCHAR(255) NULL,     -- Data Availability key for large transition
    `commitment` BINARY(32) NULL,          -- State commitment
    `metadata` JSON NULL,                  -- Optional application metadata
    `computed_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uk_app_sequence (`app_instance_id`, `sequence`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_sequence (`sequence`),
    INDEX idx_state_hash (`state_hash`),
    INDEX idx_computed_at (`computed_at`),
    CONSTRAINT fk_optimistic_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Optimistic state calculations for fast feedback';

-- 4. State Table - Final proved state with ZK proof
CREATE TABLE IF NOT EXISTS state (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `sequence` BIGINT UNSIGNED NOT NULL,
    `state_hash` BINARY(32) NOT NULL,
    `state_data` BLOB NULL,                -- State data (if small)
    `state_da` VARCHAR(255) NULL,          -- Data Availability key for large state
    `proof_data` BLOB NULL,                -- ZK proof (if small)
    `proof_da` VARCHAR(255) NULL,          -- Data Availability key for large proof
    `proof_hash` BINARY(32) NULL,          -- Hash of proof
    `commitment` BINARY(32) NULL,          -- State commitment
    `metadata` JSON NULL,                  -- Optional application metadata
    `proved_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uk_app_sequence (`app_instance_id`, `sequence`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_sequence (`sequence`),
    INDEX idx_state_hash (`state_hash`),
    INDEX idx_proved_at (`proved_at`),
    CONSTRAINT fk_state_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Final proved states with ZK proofs';

-- 5. Proofs Table - ZK proofs without sequence or commitment
CREATE TABLE IF NOT EXISTS proofs (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `proof_type` VARCHAR(255) NOT NULL,    -- Type/category of proof
    `claim_json` JSON NOT NULL,            -- JSON representation of the claim
    `claim_hash` BINARY(32) NULL,
    `claim_data` BLOB NULL,                -- Claim data (if small)
    `claim_da` VARCHAR(255) NULL,          -- S3 key for large claim
    `proof_data` BLOB NULL,                -- ZK proof (if small)
    `proof_da` VARCHAR(255) NULL,          -- S3 key for large proof
    `proof_hash` BINARY(32) NULL,          -- Hash of proof
    `proof_time` TIMESTAMP NULL,           -- Timestamp at which proof is valid
    `metadata` JSON NULL,                  -- Optional application metadata
    `proved_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_proof_type (`proof_type`),
    INDEX idx_claim_hash (`claim_hash`),
    INDEX idx_proof_time (`proof_time`),
    INDEX idx_proved_at (`proved_at`),
    INDEX idx_proofs_paging (`app_instance_id`, `proof_type`, `proved_at`, `id`),  -- Keyset pagination
    CONSTRAINT fk_proofs_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='ZK proofs with type and validity time';

-- ============================================================================
-- Object Storage Tables
-- ============================================================================

-- 5. Objects Table - Latest version of each object (cache)
CREATE TABLE IF NOT EXISTS objects (
    `object_id` VARCHAR(64) PRIMARY KEY,   -- Hex string ED25519 address
    `version` BIGINT UNSIGNED NOT NULL,              -- Current version (Lamport timestamp)
    `owner` VARCHAR(255) NOT NULL,         -- Ed25519 public key or app_instance_id, NOT NULL for security
    `object_type` VARCHAR(255) NOT NULL,   -- Type of the object
    `shared` BOOLEAN DEFAULT FALSE,        -- Whether object can be accessed by multiple owners
    `object_data` BLOB NULL,               -- Object data (if small)
    `object_da` VARCHAR(255) NULL,         -- Data Availability key for large objects
    `object_hash` BINARY(32) NOT NULL,     -- Hash of object data
    `previous_tx` VARCHAR(64) NULL,        -- Previous transaction that modified this
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    `metadata` JSON NULL,                  -- Optional metadata
    INDEX idx_owner (`owner`),
    INDEX idx_version (`version`),
    INDEX idx_object_type (`object_type`),
    INDEX idx_shared (`shared`),
    UNIQUE KEY uk_object_version (`object_id`, `version`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Current version of objects with Ed25519 ownership';

-- 6. Object Versions Table - Complete version history
CREATE TABLE IF NOT EXISTS object_versions (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `object_id` VARCHAR(64) NOT NULL,      -- Object identifier
    `version` BIGINT UNSIGNED NOT NULL,             -- Version number (Lamport timestamp)
    `object_data` BLOB NULL,               -- Object data (if small)
    `object_da` VARCHAR(255) NULL,         -- Data Availability key for large objects
    `object_hash` BINARY(32) NOT NULL,     -- Hash of object data
    `owner` VARCHAR(255) NOT NULL,         -- Ed25519 public key or app_instance_id, NOT NULL for security
    `object_type` VARCHAR(255) NOT NULL,   -- Type of the object
    `shared` BOOLEAN DEFAULT FALSE,        -- Whether object was shared at this version
    `previous_tx` VARCHAR(64) NULL,        -- Transaction that created this version
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uk_object_version (`object_id`, `version`),
    INDEX idx_object_id_version (`object_id`, `version` DESC),
    INDEX idx_owner (`owner`),
    INDEX idx_created_at (`created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Complete version history of all objects';

-- ============================================================================
-- Key-Value Storage Tables
-- ============================================================================

-- 7. App Instance KV String Table - String key-value pairs
CREATE TABLE IF NOT EXISTS app_instance_kv_string (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `key` VARCHAR(255) NOT NULL,
    `value` TEXT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `key`),
    INDEX idx_key (`key`),
    CONSTRAINT fk_kv_string_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='String key-value storage per app instance';

-- 8. App Instance KV Binary Table - Binary key-value pairs
CREATE TABLE IF NOT EXISTS app_instance_kv_binary (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `key` VARBINARY(1024) NOT NULL,
    `value` BLOB NOT NULL,
    `value_da` VARCHAR(255) NULL,          -- Data Availability key for large values
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `key`),
    CONSTRAINT fk_kv_binary_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Binary key-value storage per app instance';

-- ============================================================================
-- Queue Management Tables
-- ============================================================================

-- 9. Object Lock Queue Table - FIFO queue for object locking
CREATE TABLE IF NOT EXISTS object_lock_queue (
    `object_id` VARCHAR(64) NOT NULL,          -- Object being locked
    `req_id` VARCHAR(64) NOT NULL,             -- Request identifier (UUID)
    `app_instance_id` VARCHAR(255) NOT NULL,   -- Which app instance requested
    `retry_count` INT DEFAULT 0,               -- Number of retries before queuing
    `queued_at` TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    `lease_until` TIMESTAMP(6) NULL,           -- When lease expires
    `lease_granted_at` TIMESTAMP(6) NULL,      -- When lease was granted
    `status` ENUM('WAITING', 'GRANTED', 'EXPIRED', 'RELEASED') DEFAULT 'WAITING',
    PRIMARY KEY (`object_id`, `req_id`),
    INDEX idx_req_id (`req_id`),
    INDEX idx_status (`status`),
    INDEX idx_lease_until (`lease_until`),
    INDEX idx_queue_order (`object_id`, `queued_at`),  -- Pure FIFO ordering
    INDEX idx_cleanup (`status`, `lease_until`),       -- Optimized for cleanup query
    CONSTRAINT fk_lock_queue_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='FIFO queue for deadlock-free object locking';

-- 10. Lock Request Bundle Table - Bundle metadata for atomic locks
CREATE TABLE IF NOT EXISTS lock_request_bundle (
    `req_id` VARCHAR(64) PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `object_ids` JSON NOT NULL,                -- Array of object IDs
    `object_count` INT NOT NULL,               -- Number of objects
    `transaction_type` VARCHAR(255) NULL,      -- Type of operation
    `created_at` TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP(6),
    `started_at` TIMESTAMP(6) NULL,            -- When lock acquisition started
    `granted_at` TIMESTAMP(6) NULL,            -- When all locks granted
    `released_at` TIMESTAMP(6) NULL,           -- When locks released
    `status` ENUM('QUEUED', 'ACQUIRING', 'GRANTED', 'RELEASED', 'TIMEOUT', 'FAILED') DEFAULT 'QUEUED',
    `wait_time_ms` BIGINT UNSIGNED NULL,                -- Total wait time
    `hold_time_ms` BIGINT UNSIGNED NULL,                -- Total hold time
    INDEX idx_app_instance (`app_instance_id`),
    INDEX idx_status (`status`),
    INDEX idx_created_at (`created_at`),
    CONSTRAINT fk_lock_bundle_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Bundle metadata for all-or-nothing lock acquisition';

-- ============================================================================
-- Job Management Table
-- ============================================================================

-- 11. Jobs Table - Async job management with auto-increment primary key
CREATE TABLE IF NOT EXISTS jobs (
    `job_sequence` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `description` TEXT NULL,

    -- Metadata of the agent method to call
    `developer` VARCHAR(255) NOT NULL,
    `agent` VARCHAR(255) NOT NULL,
    `agent_method` VARCHAR(255) NOT NULL,

    -- Job data
    `block_number` BIGINT UNSIGNED NULL,
    `sequences` JSON NULL,                     -- vector<u64> as JSON array
    `sequences1` JSON NULL,                    -- vector<u64> as JSON array
    `sequences2` JSON NULL,                    -- vector<u64> as JSON array
    `data` BLOB NULL,                          -- vector<u8> as BLOB
    `data_da` VARCHAR(255) NULL,               -- Data Availability key for large job data
    `agent_jwt` TEXT NULL,                     -- JWT for agent to access private state
    `jwt_expires_at` TIMESTAMP NULL,           -- When the agent JWT expires

    -- Status (matching Move enum)
    `status` ENUM('PENDING', 'RUNNING', 'COMPLETED', 'FAILED') NOT NULL DEFAULT 'PENDING',
    `error_message` TEXT NULL,                 -- For Failed status
    `attempts` TINYINT UNSIGNED DEFAULT 0,

    -- Periodic scheduling fields (NULL for one-time jobs)
    `interval_ms` BIGINT UNSIGNED NULL,                 -- NULL for one-time jobs
    `next_scheduled_at` TIMESTAMP(6) NULL,     -- Absolute timestamp for next run

    -- Metadata timestamps
    `created_at` TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP(6),
    `updated_at` TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    `metadata` JSON NULL,                  -- Optional metadata

    -- Indexes for efficient querying
    INDEX idx_app_instance (`app_instance_id`),
    INDEX idx_app_instance_status (`app_instance_id`, `status`),
    INDEX idx_status (`status`),
    INDEX idx_developer_agent_method (`developer`, `agent`, `agent_method`),
    INDEX idx_next_scheduled (`next_scheduled_at`),
    INDEX idx_block_number (`block_number`),
    INDEX idx_job_cleanup (`status`, `updated_at`),         -- Optimized for cleanup query
    CONSTRAINT fk_jobs_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Job queue for async operations with auto-increment sequence for event streaming';

-- ============================================================================
-- Sequence Counter Table for gapless action sequences
-- ============================================================================

CREATE TABLE IF NOT EXISTS action_seq (
    `app_instance_id` VARCHAR(255) PRIMARY KEY,
    `next_seq` BIGINT UNSIGNED NOT NULL DEFAULT 1,

    -- Foreign key to app_instances for cascade delete
    CONSTRAINT fk_action_seq_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Sequence counter for generating gapless action sequences per app instance';

-- ============================================================================
-- Job Sequence Counter Table for gapless job sequences
-- ============================================================================

CREATE TABLE IF NOT EXISTS job_seq (
    `app_instance_id` VARCHAR(255) PRIMARY KEY,
    `next_seq` BIGINT UNSIGNED NOT NULL DEFAULT 1,

    -- Foreign key to app_instances for cascade delete
    CONSTRAINT fk_job_seq_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Sequence counter for generating gapless job sequences per app instance';

-- ============================================================================
-- Helper Views (Optional - for common query patterns)
-- ============================================================================
-- NOTE: Views are commented out because mysqldef doesn't support CREATE OR REPLACE VIEW
-- You can create these views manually after applying the schema with:
-- mysql -u <user> -p -h <host> -P <port> <database> < create_views.sql
--
-- -- View for latest state per app instance
-- CREATE OR REPLACE VIEW latest_state AS
-- SELECT s.*
-- FROM state s
-- INNER JOIN (
--     SELECT app_instance_id, MAX(sequence) as max_sequence
--     FROM state
--     GROUP BY app_instance_id
-- ) latest ON s.app_instance_id = latest.app_instance_id
--     AND s.sequence = latest.max_sequence;
--
-- -- View for pending jobs ready to run
-- CREATE OR REPLACE VIEW pending_jobs AS
-- SELECT *
-- FROM jobs
-- WHERE status = 'PENDING'
--   AND (next_scheduled_at IS NULL OR next_scheduled_at <= NOW(6))
-- ORDER BY job_sequence;
--
-- -- View for active locks
-- CREATE OR REPLACE VIEW active_locks AS
-- SELECT *
-- FROM object_lock_queue
-- WHERE status = 'GRANTED'
--   AND lease_until > NOW(6);

-- ============================================================================
-- Coordination Layer Support Tables
-- ============================================================================

-- 12. Blocks Table - Required for block management
CREATE TABLE IF NOT EXISTS blocks (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `block_number` BIGINT UNSIGNED NOT NULL,
    `start_sequence` BIGINT UNSIGNED NOT NULL,
    `end_sequence` BIGINT UNSIGNED NOT NULL,
    `actions_commitment` BINARY(32) NULL,
    `state_commitment` BINARY(32) NULL,
    `time_since_last_block` BIGINT UNSIGNED NULL,
    `number_of_transactions` BIGINT UNSIGNED DEFAULT 0,
    `start_actions_commitment` BINARY(32) NULL,
    `end_actions_commitment` BINARY(32) NULL,
    `state_data_availability` VARCHAR(255) NULL,
    `proof_data_availability` VARCHAR(255) NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `state_calculated_at` TIMESTAMP NULL,
    `proved_at` TIMESTAMP NULL,
    PRIMARY KEY (`app_instance_id`, `block_number`),
    INDEX idx_block_number (`block_number`),
    INDEX idx_sequences (`start_sequence`, `end_sequence`),
    INDEX idx_created_at (`created_at`),
    CONSTRAINT fk_blocks_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Block management for coordination layers';

-- 13. Proof Calculations Table - Required for proof management
CREATE TABLE IF NOT EXISTS proof_calculations (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `id` VARCHAR(64) NOT NULL,
    `block_number` BIGINT UNSIGNED NOT NULL,
    `start_sequence` BIGINT UNSIGNED NOT NULL,
    `end_sequence` BIGINT UNSIGNED NULL,
    `proofs` JSON NULL,  -- Array of proof objects
    `block_proof` TEXT NULL,
    `block_proof_submitted` BOOLEAN DEFAULT FALSE,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `id`),
    INDEX idx_block_number (`block_number`),
    INDEX idx_sequences (`start_sequence`, `end_sequence`),
    CONSTRAINT fk_proof_calc_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Proof calculation tracking for blocks';

-- 14. Settlements Table - Required for settlement operations
CREATE TABLE IF NOT EXISTS settlements (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `chain` VARCHAR(50) NOT NULL,
    `last_settled_block_number` BIGINT UNSIGNED DEFAULT 0,
    `settlement_address` VARCHAR(255) NULL,
    `settlement_job` BIGINT UNSIGNED NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `chain`),
    INDEX idx_chain (`chain`),
    CONSTRAINT fk_settlements_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Settlement configuration per chain';

-- 15. Block Settlements Table - Required for settlement tracking
CREATE TABLE IF NOT EXISTS block_settlements (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `block_number` BIGINT UNSIGNED NOT NULL,
    `chain` VARCHAR(50) NOT NULL,
    `settlement_tx_hash` VARCHAR(255) NULL,
    `settlement_tx_included_in_block` BIGINT UNSIGNED NULL,
    `sent_to_settlement_at` TIMESTAMP NULL,
    `settled_at` TIMESTAMP NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `block_number`, `chain`),
    INDEX idx_chain (`chain`),
    INDEX idx_settled_at (`settled_at`),
    CONSTRAINT fk_block_settlements_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Block settlement status tracking';

-- 16. App Instance Metadata Table - Required for metadata operations
CREATE TABLE IF NOT EXISTS app_instance_metadata (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `key` VARCHAR(255) NOT NULL,
    `value` TEXT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    PRIMARY KEY (`app_instance_id`, `key`),
    INDEX idx_key (`key`),
    CONSTRAINT fk_metadata_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Key-value metadata storage for app instances';

-- ============================================================================
-- Coordinator Authentication and Access Control Tables
-- ============================================================================

-- 17. Coordinator Groups Table - Groups of coordinators for access control
CREATE TABLE IF NOT EXISTS coordinator_groups (
    `group_id` VARCHAR(255) PRIMARY KEY,
    `group_name` VARCHAR(255) NOT NULL,
    `description` TEXT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uk_group_name (`group_name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Groups of coordinators for access control';

-- 18. Coordinator Group Members Table - Coordinators in each group
CREATE TABLE IF NOT EXISTS coordinator_group_members (
    `group_id` VARCHAR(255) NOT NULL,
    `coordinator_public_key` VARCHAR(64) NOT NULL,  -- Ed25519 public key (hex)
    `coordinator_name` VARCHAR(255) NULL,
    `added_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `added_by` VARCHAR(64) NOT NULL,  -- Who added this coordinator
    PRIMARY KEY (`group_id`, `coordinator_public_key`),
    INDEX idx_coordinator_public_key (`coordinator_public_key`),
    CONSTRAINT fk_coordinator_group FOREIGN KEY (`group_id`)
        REFERENCES coordinator_groups (`group_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Coordinator public keys in each group';

-- 19. App Instance Coordinator Access Table - Whitelist of coordinator groups per app instance
CREATE TABLE IF NOT EXISTS app_instance_coordinator_access (
    `app_instance_id` VARCHAR(255) NOT NULL,
    `group_id` VARCHAR(255) NOT NULL,
    `granted_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `granted_by` VARCHAR(64) NOT NULL,  -- Should be app instance owner
    `expires_at` TIMESTAMP NULL,  -- Optional expiry
    `access_level` ENUM('READ_ONLY', 'JOB_MANAGE', 'FULL') DEFAULT 'JOB_MANAGE',
    PRIMARY KEY (`app_instance_id`, `group_id`),
    INDEX idx_group_id (`group_id`),
    INDEX idx_expires_at (`expires_at`),
    INDEX idx_access_level (`access_level`),
    CONSTRAINT fk_access_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE,
    CONSTRAINT fk_access_group FOREIGN KEY (`group_id`)
        REFERENCES coordinator_groups (`group_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Which coordinator groups can access which app instances';

-- 20. Coordinator Audit Log Table - Audit trail of all coordinator actions
CREATE TABLE IF NOT EXISTS coordinator_audit_log (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_public_key` VARCHAR(64) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `action` VARCHAR(255) NOT NULL,  -- e.g., 'start_job', 'complete_job', 'fail_job'
    `job_sequence` BIGINT UNSIGNED NULL,
    `timestamp` TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP(6),
    `success` BOOLEAN NOT NULL,
    `error_message` TEXT NULL,
    INDEX idx_coordinator (`coordinator_public_key`),
    INDEX idx_app_instance (`app_instance_id`),
    INDEX idx_timestamp (`timestamp`),
    INDEX idx_action (`action`),
    INDEX idx_success (`success`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Audit log of all coordinator actions';

-- ============================================================================
-- End of Schema
-- ============================================================================