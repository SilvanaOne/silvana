//! coordinator_group_members entity
//! Coordinator public keys in each group

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "coordinator_group_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub group_id: String,

    #[sea_orm(primary_key, auto_increment = false)]
    pub coordinator_public_key: String,  // Ed25519 public key (hex, 64 chars)

    pub coordinator_name: Option<String>,

    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub added_at: DateTimeUtc,

    pub added_by: String,  // Public key of who added this coordinator
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::coordinator_groups::Entity",
        from = "Column::GroupId",
        to = "super::coordinator_groups::Column::GroupId",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    CoordinatorGroup,
}

impl Related<super::coordinator_groups::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CoordinatorGroup.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
