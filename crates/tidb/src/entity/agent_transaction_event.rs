//! AgentTransactionEvent entity
//! Generated from proto definition: AgentTransactionEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "agent_transaction_event")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub coordinator_id: String,
    pub tx_type: String,
    pub developer: String,
    pub agent: String,
    pub app: String,
    pub job_sequence: String,
    pub event_timestamp: i64,
    pub tx_hash: String,
    pub chain: String,
    pub network: String,
    pub memo: String,
    pub metadata: Option<String>,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
