//! app_instances entity
//! Core authorization table - all other tables reference this

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "app_instances")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub app_instance_id: String,
    pub owner: String,  // Ed25519 public key (hex), NOT NULL for security
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::user_actions::Entity")]
    UserActions,
    #[sea_orm(has_many = "super::optimistic_state::Entity")]
    OptimisticState,
    #[sea_orm(has_many = "super::state::Entity")]
    State,
    #[sea_orm(has_many = "super::app_instance_kv_string::Entity")]
    KvString,
    #[sea_orm(has_many = "super::app_instance_kv_binary::Entity")]
    KvBinary,
    #[sea_orm(has_many = "super::object_lock_queue::Entity")]
    LockQueue,
    #[sea_orm(has_many = "super::lock_request_bundle::Entity")]
    LockBundle,
    #[sea_orm(has_many = "super::jobs::Entity")]
    Jobs,
}

impl Related<super::user_actions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserActions.def()
    }
}

impl Related<super::optimistic_state::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OptimisticState.def()
    }
}

impl Related<super::state::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::State.def()
    }
}

impl Related<super::app_instance_kv_string::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::KvString.def()
    }
}

impl Related<super::app_instance_kv_binary::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::KvBinary.def()
    }
}

impl Related<super::object_lock_queue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::LockQueue.def()
    }
}

impl Related<super::lock_request_bundle::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::LockBundle.def()
    }
}

impl Related<super::jobs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Jobs.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}