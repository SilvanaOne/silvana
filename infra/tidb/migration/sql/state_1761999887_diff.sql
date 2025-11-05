-- dry run --
ALTER TABLE `app_instances` ADD COLUMN `admin` varchar(64) NULL AFTER `metadata`;
ALTER TABLE `app_instances` ADD COLUMN `is_paused` boolean DEFAULT false AFTER `admin`;
ALTER TABLE `app_instances` ADD COLUMN `min_time_between_blocks` bigint UNSIGNED DEFAULT 60 AFTER `is_paused`;
ALTER TABLE `app_instances` ADD COLUMN `block_number` bigint UNSIGNED DEFAULT 0 AFTER `min_time_between_blocks`;
ALTER TABLE `app_instances` ADD COLUMN `sequence` bigint UNSIGNED DEFAULT 0 AFTER `block_number`;
ALTER TABLE `app_instances` ADD COLUMN `last_proved_block_number` bigint UNSIGNED DEFAULT 0 AFTER `sequence`;
ALTER TABLE `app_instances` ADD COLUMN `last_settled_block_number` bigint UNSIGNED DEFAULT 0 AFTER `last_proved_block_number`;
ALTER TABLE `app_instances` ADD COLUMN `last_settled_sequence` bigint UNSIGNED DEFAULT 0 AFTER `last_settled_block_number`;
ALTER TABLE `app_instances` ADD COLUMN `last_purged_sequence` bigint UNSIGNED DEFAULT 0 AFTER `last_settled_sequence`;
ALTER TABLE `app_instances` ADD INDEX `idx_is_paused` (`is_paused`);
ALTER TABLE `app_instances` ADD INDEX `idx_block_number` (`block_number`);
ALTER TABLE `app_instances` ADD INDEX `idx_sequence` (`sequence`);
ALTER TABLE `proofs` ADD INDEX `idx_proofs_paging` (`app_instance_id`, `proof_type`, `proved_at`, `id`);
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
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
