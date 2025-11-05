//! coordinator_audit_log entity
//! Audit trail of all coordinator actions

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "coordinator_audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub coordinator_public_key: String,
    pub app_instance_id: String,
    pub action: String,  // e.g., 'create_job', 'start_job', 'complete_job', 'fail_job'

    pub job_sequence: Option<u64>,

    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub timestamp: DateTimeUtc,

    pub success: bool,

    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
