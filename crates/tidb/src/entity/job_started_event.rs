//! JobStartedEvent entity
//! Generated from proto definition: JobStartedEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_started_event")]
pub struct Model {
    pub coordinator_id: String,
    pub session_id: String,
    pub app_instance_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub job_id: String,
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
