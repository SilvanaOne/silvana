//! blocks entity
//! Block management for coordination layers

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "blocks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub block_number: u64,
    pub start_sequence: u64,
    pub end_sequence: u64,
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub actions_commitment: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub state_commitment: Option<Vec<u8>>,
    pub time_since_last_block: Option<u64>,
    pub number_of_transactions: u64,
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub start_actions_commitment: Option<Vec<u8>>,
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub end_actions_commitment: Option<Vec<u8>>,
    pub state_data_availability: Option<String>,
    pub proof_data_availability: Option<String>,
    #[sea_orm(column_type = "Timestamp")]
    pub created_at: DateTimeUtc,
    #[sea_orm(column_type = "Timestamp", nullable)]
    pub state_calculated_at: Option<DateTimeUtc>,
    #[sea_orm(column_type = "Timestamp", nullable)]
    pub proved_at: Option<DateTimeUtc>,
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