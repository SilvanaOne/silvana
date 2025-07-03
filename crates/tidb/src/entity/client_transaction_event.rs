//! ClientTransactionEvent entity
//! Generated from proto definition: ClientTransactionEvent

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "client_transaction_event")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub coordinator_id: String,
    pub developer: String,
    pub agent: String,
    pub app: String,
    pub client_ip_address: String,
    pub method: String,
    pub data: Vec<u8>,
    pub tx_hash: String,
    pub sequence: i64,
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
