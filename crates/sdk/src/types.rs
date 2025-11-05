//! Type re-exports and convenience types for the Silvana SDK

// Re-export all proto types for easy access
pub use crate::proto::coordinator::{
    // Core job types
    Job,

    // Logging
    LogLevel,

    // Proof types
    ProofEventType,

    // State types
    SequenceState,

    // Block types
    Block,
    BlockSettlement,
    SettlementInfo,

    // Metadata and app instance types
    Metadata,
    AppInstanceData,
    AppMethod,

    // Response types
    GetJobResponse,
    CompleteJobResponse,
    FailJobResponse,
    TerminateJobResponse,
    GetSequenceStatesResponse,
    SubmitProofResponse,
    RejectProofResponse,
    SubmitStateResponse,
    GetProofResponse,
    GetBlockProofResponse,
    GetBlockResponse,
    GetBlockSettlementResponse,
    UpdateBlockSettlementResponse,
    GetSettlementProofResponse,
    SubmitSettlementProofResponse,
    GetAppInstanceResponse,
    ReadDataAvailabilityResponse,
    RetrieveSecretResponse,
    SetKvResponse as SetKVResponse,
    GetKvResponse as GetKVResponse,
    DeleteKvResponse as DeleteKVResponse,
    AddMetadataResponse,
    GetMetadataResponse,
    TryCreateBlockResponse,
    UpdateBlockStateDataAvailabilityResponse,
    UpdateBlockProofDataAvailabilityResponse,
    UpdateBlockSettlementTxHashResponse,
    UpdateBlockSettlementTxIncludedInBlockResponse,
    CreateAppJobResponse,
    ProofEventResponse,
    AgentMessageResponse,
};

// Re-export parameter types from other modules
pub use crate::state::{
    SubmitStateParams,
    SubmitProofParams,
    GetProofParams,
    RejectProofParams,
    ProofEventParams,
};

pub use crate::block::{
    SubmitSettlementProofParams,
    UpdateBlockSettlementParams,
};

pub use crate::kv::CreateAppJobParams;