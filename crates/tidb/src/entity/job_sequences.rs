//! Job sequences entity
//! Maps (app_instance_id, sequence) => job_id (many-to-many)

use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "job_sequences")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub app_instance_id: String,
    pub sequence: i64,
    pub job_id: String,
    pub sequence_type: String,
    pub created_at: Option<DateTimeUtc>,
    pub updated_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
