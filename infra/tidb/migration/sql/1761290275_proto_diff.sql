-- dry run --
ALTER TABLE `proof_event` ADD COLUMN `settlement_proof` boolean NULL AFTER `block_proof`;
ALTER TABLE `proof_event` ADD INDEX `idx_app_instance_id_block_number_settlement_proof` (`app_instance_id`, `block_number`, `settlement_proof`);
