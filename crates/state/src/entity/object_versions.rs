//! object_versions entity
//! Complete version history of all objects

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "object_versions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub object_id: String,  // Object identifier
    pub version: i64,  // Version number (Lamport timestamp)
    #[sea_orm(column_type = "Blob", nullable)]
    pub object_data: Option<Vec<u8>>,  // Object data (if small)
    pub object_da: Option<String>,  // S3 reference for large objects
    #[sea_orm(column_type = "Binary(32)")]
    pub object_hash: Vec<u8>,  // Hash of object data (32 bytes)
    pub owner: String,  // Ed25519 public key or app_instance_id, NOT NULL for security
    pub object_type: String,
    pub shared: bool,  // Whether object was shared at this version
    pub previous_tx: Option<String>,  // Transaction that created this version
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}