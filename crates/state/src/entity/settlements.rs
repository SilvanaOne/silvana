//! settlements entity
//! Settlement configuration per chain

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "settlements")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub chain: String,
    pub last_settled_block_number: u64,
    pub settlement_address: Option<String>,
    pub settlement_job: Option<u64>,
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
