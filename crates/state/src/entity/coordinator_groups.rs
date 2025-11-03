//! coordinator_groups entity
//! Groups of coordinators for access control

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "coordinator_groups")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub group_id: String,

    pub group_name: String,

    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub created_at: DateTimeUtc,

    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::coordinator_group_members::Entity")]
    CoordinatorGroupMembers,

    #[sea_orm(has_many = "super::app_instance_coordinator_access::Entity")]
    AppInstanceCoordinatorAccess,
}

impl Related<super::coordinator_group_members::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CoordinatorGroupMembers.def()
    }
}

impl Related<super::app_instance_coordinator_access::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AppInstanceCoordinatorAccess.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
