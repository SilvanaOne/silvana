-- dry run --
ALTER TABLE `app_instances` ADD COLUMN `metadata` json NULL AFTER `updated_at`;
ALTER TABLE `objects` ADD COLUMN `metadata` json NULL AFTER `updated_at`;
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
ALTER TABLE `jobs` ADD COLUMN `metadata` json NULL AFTER `updated_at`;
