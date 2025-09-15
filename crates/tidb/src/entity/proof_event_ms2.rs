//! Child entity for `merged_sequences_2`. `ProofEvent` -> `proof_event_ms2`

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "proof_event_ms2")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub proof_event_id: i64,
    pub merged_sequences_2: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
