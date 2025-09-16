//! ProofEvent entity
//! Generated from proto definition: ProofEvent

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "proof_event")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub coordinator_id: String,
    pub session_id: String,
    pub app_instance_id: String,
    pub job_id: String,
    pub data_availability: String,
    pub block_number: i64,
    pub block_proof: Option<bool>,
    pub proof_event_type: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub sequences: Option<Json>, // JSON array of u64
    #[sea_orm(column_type = "JsonBinary")]
    pub merged_sequences_1: Option<Json>, // JSON array of u64
    #[sea_orm(column_type = "JsonBinary")]
    pub merged_sequences_2: Option<Json>, // JSON array of u64
    pub event_timestamp: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
