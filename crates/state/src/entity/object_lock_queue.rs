//! object_lock_queue entity
//! FIFO queue for deadlock-free object locking

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "object_lock_queue")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub object_id: String,  // Object being locked (VARCHAR(64))
    #[sea_orm(primary_key, auto_increment = false)]
    pub req_id: String,  // Request identifier UUID (VARCHAR(64))
    pub app_instance_id: String,  // Which app instance requested
    pub retry_count: i32,  // Number of retries before queuing
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub queued_at: DateTimeUtc,
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub lease_until: Option<DateTimeUtc>,  // When lease expires
    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub lease_granted_at: Option<DateTimeUtc>,  // When lease was granted
    pub status: String,  // WAITING, GRANTED, EXPIRED, RELEASED
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