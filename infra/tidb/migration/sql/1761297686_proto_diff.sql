-- dry run --
ALTER TABLE `proof_event` ADD COLUMN `settlement_proof_chain` varchar(255) NULL AFTER `settlement_proof`;
ALTER TABLE `proof_event` ADD INDEX `idx_app_instance_id_block_number_settlement_proof_settlement_proof_chain` (`app_instance_id`, `block_number`, `settlement_proof`, `settlement_proof_chain`);
ALTER TABLE `proof_event` DROP INDEX `idx_app_instance_id_block_number_settlement_proof`;
