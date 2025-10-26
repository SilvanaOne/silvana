//! optimistic_state entity
//! Fast state calculation without proof

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "optimistic_state")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub app_instance_id: String,
    pub sequence: i64,
    #[sea_orm(column_type = "Binary(32)")]
    pub state_hash: Vec<u8>,  // Hash of state data (32 bytes)
    #[sea_orm(column_type = "Blob")]
    pub state_data: Vec<u8>,  // State data (if small)
    pub state_da: Option<String>,  // S3 key for large state data
    #[sea_orm(column_type = "Blob", nullable)]
    pub transition_data: Option<Vec<u8>>,  // Transition delta
    pub transition_da: Option<String>,  // S3 key for large transition
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub commitment: Option<Vec<u8>>,  // State commitment (32 bytes)
    pub computed_at: DateTimeUtc,
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