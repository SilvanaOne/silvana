//! AgentSessionStartedEvent entity
//! Generated from proto definition: AgentSessionStartedEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "agent_session")]
pub struct Model {
    pub coordinator_id: String,
    pub developer: String,
    pub agent: String,
    pub agent_method: String,
    #[sea_orm(primary_key)]
    pub session_id: String,
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
