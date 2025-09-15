//! JobFinishedEvent entity
//! Generated from proto definition: JobFinishedEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_finished_event")]
pub struct Model {
    pub coordinator_id: String,
    #[sea_orm(primary_key)]
    pub job_id: String,
    pub duration: i64,
    pub event_timestamp: i64,
    pub result: String,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
