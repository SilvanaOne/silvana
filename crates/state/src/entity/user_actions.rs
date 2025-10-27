//! user_actions entity
//! Input actions that trigger state changes

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_actions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub app_instance_id: String,
    pub sequence: u64,
    pub action_type: String,
    #[sea_orm(column_type = "Blob")]
    pub action_data: Vec<u8>,  // Action parameters (serialized)
    #[sea_orm(column_type = "Binary(32)")]
    pub action_hash: Vec<u8>,  // Hash of action data (32 bytes)
    pub action_da: Option<String>,  // S3 key for large action data
    pub submitter: String,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,  // Optional application metadata
    pub created_at: DateTimeUtc,
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