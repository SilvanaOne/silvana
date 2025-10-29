-- dry run --
CREATE TABLE IF NOT EXISTS proofs (
    `id` BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    `app_instance_id` VARCHAR(255) NOT NULL,
    `proof_type` VARCHAR(255) NOT NULL,    -- Type/category of proof
    `claim_json` JSON NOT NULL,            -- JSON representation of the claim
    `claim_hash` BINARY(32) NULL,
    `claim_data` BLOB NULL,                -- Claim data (if small)
    `claim_da` VARCHAR(255) NULL,          -- Data Availability key for large claim
    `proof_data` BLOB NULL,                -- ZK proof (if small)
    `proof_da` VARCHAR(255) NULL,          -- Data Availability key for large proof
    `proof_hash` BINARY(32) NULL,          -- Hash of proof
    `proof_time` TIMESTAMP NULL,           -- Timestamp at which proof is valid
    `metadata` JSON NULL,                  -- Optional application metadata
    `proved_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    INDEX idx_app_instance_id (`app_instance_id`),
    INDEX idx_proof_type (`proof_type`),
    INDEX idx_claim_hash (`claim_hash`),
    INDEX idx_proof_time (`proof_time`),
    INDEX idx_proved_at (`proved_at`),
    CONSTRAINT fk_proofs_app_instance FOREIGN KEY (`app_instance_id`)
        REFERENCES app_instances (`app_instance_id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
COMMENT='ZK proofs with type and validity time';
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
