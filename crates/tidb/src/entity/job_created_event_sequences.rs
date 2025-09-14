//! Child entity for `sequences`. `JobCreatedEvent` -> `job_created_event_sequences`

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_created_event_sequences")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub job_created_event_id: i64,
    pub sequence: i64,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
