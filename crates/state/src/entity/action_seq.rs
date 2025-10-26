//! action_seq entity
//! Counter table for generating gapless action sequences per app instance

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "action_seq")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    pub next_seq: i64,  // BIGINT UNSIGNED in MySQL, i64 in Rust
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::app_instances::Entity",
        from = "Column::AppInstanceId",
        to = "super::app_instances::Column::AppInstanceId"
    )]
    AppInstance,
}

impl Related<super::app_instances::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppInstance.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}