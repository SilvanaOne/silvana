-- dry run --
ALTER TABLE `user_actions` CHANGE COLUMN `id` `id` bigint UNSIGNED NOT NULL AUTO_INCREMENT;
ALTER TABLE `user_actions` CHANGE COLUMN `sequence` `sequence` bigint UNSIGNED NOT NULL;
ALTER TABLE `optimistic_state` CHANGE COLUMN `id` `id` bigint UNSIGNED NOT NULL AUTO_INCREMENT;
ALTER TABLE `optimistic_state` CHANGE COLUMN `sequence` `sequence` bigint UNSIGNED NOT NULL;
ALTER TABLE `state` CHANGE COLUMN `id` `id` bigint UNSIGNED NOT NULL AUTO_INCREMENT;
ALTER TABLE `state` CHANGE COLUMN `sequence` `sequence` bigint UNSIGNED NOT NULL;
ALTER TABLE `objects` CHANGE COLUMN `version` `version` bigint UNSIGNED NOT NULL;
ALTER TABLE `object_versions` CHANGE COLUMN `id` `id` bigint UNSIGNED NOT NULL AUTO_INCREMENT;
ALTER TABLE `object_versions` CHANGE COLUMN `version` `version` bigint UNSIGNED NOT NULL;
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
ALTER TABLE `lock_request_bundle` CHANGE COLUMN `wait_time_ms` `wait_time_ms` bigint UNSIGNED NULL;
ALTER TABLE `lock_request_bundle` CHANGE COLUMN `hold_time_ms` `hold_time_ms` bigint UNSIGNED NULL;
ALTER TABLE `jobs` CHANGE COLUMN `job_sequence` `job_sequence` bigint UNSIGNED NOT NULL;
ALTER TABLE `jobs` CHANGE COLUMN `block_number` `block_number` bigint UNSIGNED NULL;
ALTER TABLE `jobs` CHANGE COLUMN `interval_ms` `interval_ms` bigint UNSIGNED NULL;
