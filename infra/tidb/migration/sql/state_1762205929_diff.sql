-- dry run --
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
ALTER TABLE `jobs` ADD COLUMN `agent_jwt` text NULL AFTER `data_da`;
ALTER TABLE `jobs` ADD COLUMN `jwt_expires_at` timestamp NULL AFTER `agent_jwt`;
CREATE TABLE IF NOT EXISTS coordinator_groups (
    `group_id` VARCHAR(255) PRIMARY KEY,
    `group_name` VARCHAR(255) NOT NULL,
    `description` TEXT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uk_group_name (`group_name`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='Groups of coordinators for access control';
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
