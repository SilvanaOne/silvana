//! AgentStartedJobEvent entity
//! Generated from proto definition: AgentStartedJobEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "agent_started_job_event")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub coordinator_id: String,
    pub developer: String,
    pub agent: String,
    pub app: String,
    pub job_id: String,
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
