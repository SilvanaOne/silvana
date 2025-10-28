//! objects entity
//! Current version of objects with Ed25519 ownership

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "objects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub object_id: String,  // Hex string ED25519 address
    pub version: u64,  // Current version (Lamport timestamp)
    pub owner: String,  // Ed25519 public key or app_instance_id, NOT NULL for security
    pub object_type: String,
    pub shared: bool,  // Whether object can be accessed by multiple owners
    #[sea_orm(column_type = "Blob", nullable)]
    pub object_data: Option<Vec<u8>>,  // Object data (if small)
    pub object_da: Option<String>,  // S3 reference for large objects
    #[sea_orm(column_type = "Binary(32)")]
    pub object_hash: Vec<u8>,  // Hash of object data (32 bytes)
    pub previous_tx: Option<String>,  // Previous transaction that modified this
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}