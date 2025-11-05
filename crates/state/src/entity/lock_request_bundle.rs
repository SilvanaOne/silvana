//! lock_request_bundle entity
//! Bundle metadata for all-or-nothing lock acquisition

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "lock_request_bundle")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub req_id: String,  // Request ID (UUID)
    pub app_instance_id: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub object_ids: Json,  // Array of object IDs
    pub object_count: i32,  // Number of objects
    pub transaction_type: Option<String>,  // Type of operation
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub created_at: DateTimeUtc,
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub started_at: Option<DateTimeUtc>,  // When lock acquisition started
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub granted_at: Option<DateTimeUtc>,  // When all locks granted
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub released_at: Option<DateTimeUtc>,  // When locks released
    pub status: String,  // QUEUED, ACQUIRING, GRANTED, RELEASED, TIMEOUT, FAILED
    pub wait_time_ms: Option<u64>,  // Total wait time
    pub hold_time_ms: Option<u64>,  // Total hold time
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