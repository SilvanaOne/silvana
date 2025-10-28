-- dry run --
ALTER TABLE `object_versions` DROP INDEX `idx_object_id_version`;
ALTER TABLE `object_versions` ADD INDEX `idx_object_id_version` (`object_id`, `version` desc);
ALTER TABLE `object_lock_queue` ADD INDEX `idx_cleanup` (`status`, `lease_until`);
