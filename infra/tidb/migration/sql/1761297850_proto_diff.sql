-- dry run --
ALTER TABLE `proof_event` ADD INDEX `idx_aii_bn_sp_spc` (`app_instance_id`, `block_number`, `settlement_proof`, `settlement_proof_chain`);
ALTER TABLE `proof_event` DROP INDEX `idx_app_instance_id_block_number_settlement_proof`;
