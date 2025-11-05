//! app_instance_coordinator_access entity
//! Whitelist of coordinator groups per app instance

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "app_instance_coordinator_access")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,

    #[sea_orm(primary_key, auto_increment = false)]
    pub group_id: String,

    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub granted_at: DateTimeUtc,

    pub granted_by: String,  // Should be app instance owner's public key

    #[sea_orm(column_type = "TimestampWithTimeZone", nullable)]
    pub expires_at: Option<DateTimeUtc>,

    pub access_level: String,  // READ_ONLY, JOB_MANAGE, FULL (maps to SQL ENUM)
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

    #[sea_orm(
        belongs_to = "super::coordinator_groups::Entity",
        from = "Column::GroupId",
        to = "super::coordinator_groups::Column::GroupId",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    CoordinatorGroup,
}

impl Related<super::app_instances::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppInstance.def()
    }
}

impl Related<super::coordinator_groups::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CoordinatorGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
