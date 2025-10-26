//! jobs entity
//! Async job management following Move contract structure

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub job_sequence: i64,
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    // Metadata of the agent method to call
    pub developer: String,
    pub agent: String,
    pub agent_method: String,

    // Job data
    pub block_number: Option<i64>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences1: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences2: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "Blob", nullable)]
    pub data: Option<Vec<u8>>,  // vector<u8> as BLOB
    pub data_da: Option<String>,  // S3 reference for large job data

    // Status (matching Move enum and SQL ENUM)
    pub status: String,  // PENDING, RUNNING, COMPLETED, FAILED (maps to SQL ENUM)
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,  // For FAILED status
    pub attempts: i16,

    // Periodic scheduling fields (NULL for one-time jobs)
    pub interval_ms: Option<i64>,  // NULL for one-time jobs
    #[sea_orm(column_type = "Timestamp", nullable)]
    pub next_scheduled_at: Option<DateTimeUtc>,  // Absolute timestamp for next run

    // Metadata timestamps
    #[sea_orm(column_type = "Timestamp")]
    pub created_at: DateTimeUtc,
    #[sea_orm(column_type = "Timestamp")]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::app_instances::Entity",
        from = "Column::AppInstanceId",
        to = "super::app_instances::Column::AppInstanceId",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    AppInstance,
}

impl Related<super::app_instances::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppInstance.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}