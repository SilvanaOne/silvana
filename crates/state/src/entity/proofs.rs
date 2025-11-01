//! proofs entity
//! ZK proofs with claim data

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "proofs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: u64,
    pub app_instance_id: String,
    pub proof_type: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub claim_json: Json,  // JSON representation of the claim
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub claim_hash: Option<Vec<u8>>,  // Optional hash of claim (32 bytes)
    #[sea_orm(column_type = "Blob", nullable)]
    pub claim_data: Option<Vec<u8>>,  // Claim data (if small)
    pub claim_da: Option<String>,  // S3 key for large claim
    #[sea_orm(column_type = "Blob", nullable)]
    pub proof_data: Option<Vec<u8>>,  // ZK proof (if small)
    pub proof_da: Option<String>,  // S3 key for large proof
    #[sea_orm(column_type = "Binary(32)", nullable)]
    pub proof_hash: Option<Vec<u8>>,  // Hash of proof (32 bytes)
    pub proof_time: Option<DateTimeUtc>,  // Timestamp at which proof is valid
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,  // Optional application metadata
    pub proved_at: DateTimeUtc,
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