//! jobs entity
//! Async job management following Move contract structure

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub job_sequence: u64,
    pub app_instance_id: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    // Metadata of the agent method to call
    pub developer: String,
    pub agent: String,
    pub agent_method: String,

    // Job data
    pub block_number: Option<u64>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences1: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sequences2: Option<Json>,  // vector<u64> as JSON array
    #[sea_orm(column_type = "Blob", nullable)]
    pub data: Option<Vec<u8>>,  // vector<u8> as BLOB
    pub data_da: Option<String>,  // S3 reference for large job data

    /// JWT for agent to access private state (only used in Private coordination)
    #[sea_orm(column_type = "Text", nullable)]
    pub agent_jwt: Option<String>,

    /// When the agent JWT expires
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub jwt_expires_at: Option<DateTimeUtc>,

    // Status (matching Move enum and SQL ENUM)
    pub status: String,  // PENDING, RUNNING, COMPLETED, FAILED (maps to SQL ENUM)
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,  // For FAILED status
    pub attempts: i16,

    // Periodic scheduling fields (NULL for one-time jobs)
    pub interval_ms: Option<u64>,  // NULL for one-time jobs
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub next_scheduled_at: Option<DateTimeUtc>,  // Absolute timestamp for next run

    // Metadata timestamps
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub created_at: DateTimeUtc,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub updated_at: DateTimeUtc,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,
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