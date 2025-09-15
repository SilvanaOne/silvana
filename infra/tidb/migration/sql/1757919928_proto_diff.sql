-- dry run --
CREATE TABLE IF NOT EXISTS coordinator_started_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `ethereum_address` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS coordinator_active_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS coordinator_shutdown_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS agent_session_started_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `developer` VARCHAR(255) NOT NULL,
    `agent` VARCHAR(255) NOT NULL,
    `agent_method` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_developer (`developer`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_agent (`agent`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_agent_method (`agent_method`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_created_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `app_method` VARCHAR(255) NOT NULL,
    `job_sequence` BIGINT NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_instance_id (`app_instance_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_method (`app_method`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_created_event_sequences (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `job_created_event_id` BIGINT NOT NULL,
    `sequence` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_job_created_event_sequences_parent (`job_created_event_id`),
    INDEX idx_job_created_event_sequences_value (`sequence`),
    CONSTRAINT fk_job_created_event_sequences_job_created_event_id FOREIGN KEY (`job_created_event_id`) REFERENCES job_created_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_created_event_ms1 (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `job_created_event_id` BIGINT NOT NULL,
    `merged_sequences_1` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_job_created_event_ms1_parent (`job_created_event_id`),
    INDEX idx_job_created_event_ms1_value (`merged_sequences_1`),
    CONSTRAINT fk_job_created_event_ms1_job_created_event_id FOREIGN KEY (`job_created_event_id`) REFERENCES job_created_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_created_event_ms2 (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `job_created_event_id` BIGINT NOT NULL,
    `merged_sequences_2` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_job_created_event_ms2_parent (`job_created_event_id`),
    INDEX idx_job_created_event_ms2_value (`merged_sequences_2`),
    CONSTRAINT fk_job_created_event_ms2_job_created_event_id FOREIGN KEY (`job_created_event_id`) REFERENCES job_created_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_started_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_instance_id (`app_instance_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS job_finished_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `duration` BIGINT NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `result` JSON NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS coordination_tx_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `tx_hash` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_tx_hash (`tx_hash`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_tx_hash (`tx_hash`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS coordinator_message_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `level` TINYINT NOT NULL,
    `message` TEXT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_message (`message`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS proof_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `data_availability` VARCHAR(255) NOT NULL,
    `block_number` BIGINT NOT NULL,
    `block_proof` BOOLEAN NULL,
    `proof_event_type` JSON NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_instance_id (`app_instance_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS proof_event_sequences (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `proof_event_id` BIGINT NOT NULL,
    `sequence` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_proof_event_sequences_parent (`proof_event_id`),
    INDEX idx_proof_event_sequences_value (`sequence`),
    CONSTRAINT fk_proof_event_sequences_proof_event_id FOREIGN KEY (`proof_event_id`) REFERENCES proof_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS proof_event_ms1 (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `proof_event_id` BIGINT NOT NULL,
    `merged_sequences_1` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_proof_event_ms1_parent (`proof_event_id`),
    INDEX idx_proof_event_ms1_value (`merged_sequences_1`),
    CONSTRAINT fk_proof_event_ms1_proof_event_id FOREIGN KEY (`proof_event_id`) REFERENCES proof_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS proof_event_ms2 (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `proof_event_id` BIGINT NOT NULL,
    `merged_sequences_2` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_proof_event_ms2_parent (`proof_event_id`),
    INDEX idx_proof_event_ms2_value (`merged_sequences_2`),
    CONSTRAINT fk_proof_event_ms2_proof_event_id FOREIGN KEY (`proof_event_id`) REFERENCES proof_event (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS settlement_transaction_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `block_number` BIGINT NOT NULL,
    `tx_hash` VARCHAR(255) NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_tx_hash (`tx_hash`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_instance_id (`app_instance_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_tx_hash (`tx_hash`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS settlement_transaction_included_in_block_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NOT NULL,
    `block_number` BIGINT NOT NULL,
    `event_timestamp` BIGINT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_app_instance_id (`app_instance_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE TABLE IF NOT EXISTS agent_message_event (
    `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
    `coordinator_id` VARCHAR(255) NOT NULL,
    `session_id` VARCHAR(255) NOT NULL,
    `job_id` VARCHAR(255) NULL,
    `event_timestamp` BIGINT NOT NULL,
    `level` TINYINT NOT NULL,
    `message` TEXT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (`created_at`),
    INDEX idx_coordinator_id (`coordinator_id`),
    INDEX idx_session_id (`session_id`),
    INDEX idx_job_id (`job_id`),
    INDEX idx_event_timestamp (`event_timestamp`),
    FULLTEXT INDEX ft_idx_coordinator_id (`coordinator_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_session_id (`session_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_job_id (`job_id`) WITH PARSER STANDARD,
    FULLTEXT INDEX ft_idx_message (`message`) WITH PARSER STANDARD
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
