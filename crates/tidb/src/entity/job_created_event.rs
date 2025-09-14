//! JobCreatedEvent entity
//! Generated from proto definition: JobCreatedEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_created_event")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub coordinator_id: String,
    pub session_id: String,
    pub app_instance_id: String,
    pub app_method: String,
    pub job_sequence: i64,
    pub job_id: String,
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
