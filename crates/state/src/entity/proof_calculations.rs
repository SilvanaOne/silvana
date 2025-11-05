//! proof_calculations entity
//! Proof calculation tracking for blocks

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "proof_calculations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    #[sea_orm(column_type = "Json", nullable)]
    pub proofs: Option<Json>,
    #[sea_orm(column_type = "Text", nullable)]
    pub block_proof: Option<String>,
    pub block_proof_submitted: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::app_instances::Entity",
        from = "Column::AppInstanceId",
        to = "super::app_instances::Column::AppInstanceId",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    AppInstance,
}

impl Related<super::app_instances::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppInstance.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
