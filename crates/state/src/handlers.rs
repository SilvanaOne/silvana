//! Service handler implementations for StateService

use anyhow::{anyhow, Result};
use chrono::Utc;
use sea_orm::{entity::*, query::*, DatabaseConnection, QueryOrder, QuerySelect};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{
    auth::{verify_jwt, AuthInfo},
    concurrency::ConcurrencyController,
    database::Database,
    entity::{
        app_instance_kv_binary, app_instance_kv_string, app_instances, jobs, object_lock_queue,
        object_versions, objects, optimistic_state, state, user_actions, lock_request_bundle,
    },
    proto::state_service_server::StateService,
    proto::*,
    storage::HybridStorage,
};

/// StateService implementation
pub struct StateServiceImpl {
    db: Arc<Database>,
    storage: HybridStorage,
    concurrency: Arc<ConcurrencyController>,
}

impl StateServiceImpl {
    pub fn new(db: Arc<Database>, storage: HybridStorage, concurrency: Arc<ConcurrencyController>) -> Self {
        Self {
            db,
            storage,
            concurrency,
        }
    }

    /// Extract auth from request body
    fn extract_auth_from_body(&self, auth: &Option<JwtAuth>) -> Result<AuthInfo, Status> {
        let jwt_auth = auth.as_ref()
            .ok_or_else(|| Status::unauthenticated("Missing auth field"))?;

        verify_jwt(&jwt_auth.token)
            .map_err(|e| Status::unauthenticated(format!("Invalid JWT: {}", e)))
    }

    /// Verify ownership of app instance
    async fn verify_ownership(&self, app_instance_id: &str, auth: &AuthInfo) -> Result<(), Status> {
        let owns = self
            .db
            .verify_app_instance_ownership(app_instance_id, &auth.public_key)
            .await
            .map_err(|e| Status::internal(format!("Database error: {}", e)))?;

        if !owns {
            return Err(Status::permission_denied(
                "Not authorized for this app instance",
            ));
        }

        Ok(())
    }
}

#[tonic::async_trait]
impl StateService for StateServiceImpl {
    async fn create_app_instance(
        &self,
        request: Request<CreateAppInstanceRequest>,
    ) -> Result<Response<CreateAppInstanceResponse>, Status> {
        Err(Status::unimplemented("create_app_instance not yet implemented"))
    }

    async fn get_app_instance(
        &self,
        request: Request<GetAppInstanceRequest>,
    ) -> Result<Response<GetAppInstanceResponse>, Status> {
        Err(Status::unimplemented("get_app_instance not yet implemented"))
    }

    async fn list_app_instances(
        &self,
        request: Request<ListAppInstancesRequest>,
    ) -> Result<Response<ListAppInstancesResponse>, Status> {
        Err(Status::unimplemented("list_app_instances not yet implemented"))
    }

    async fn submit_user_action(
        &self,
        request: Request<SubmitUserActionRequest>,
    ) -> Result<Response<SubmitUserActionResponse>, Status> {
        Err(Status::unimplemented("submit_user_action not yet implemented"))
    }

    async fn get_user_actions(
        &self,
        request: Request<GetUserActionsRequest>,
    ) -> Result<Response<GetUserActionsResponse>, Status> {
        Err(Status::unimplemented("get_user_actions not yet implemented"))
    }

    async fn update_optimistic_state(
        &self,
        request: Request<UpdateOptimisticStateRequest>,
    ) -> Result<Response<UpdateOptimisticStateResponse>, Status> {
        Err(Status::unimplemented("update_optimistic_state not yet implemented"))
    }

    async fn get_optimistic_state(
        &self,
        request: Request<GetOptimisticStateRequest>,
    ) -> Result<Response<GetOptimisticStateResponse>, Status> {
        Err(Status::unimplemented("get_optimistic_state not yet implemented"))
    }

    async fn submit_proved_state(
        &self,
        request: Request<SubmitProvedStateRequest>,
    ) -> Result<Response<SubmitProvedStateResponse>, Status> {
        Err(Status::unimplemented("submit_proved_state not yet implemented"))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        Err(Status::unimplemented("get_state not yet implemented"))
    }

    async fn get_latest_state(
        &self,
        request: Request<GetLatestStateRequest>,
    ) -> Result<Response<GetLatestStateResponse>, Status> {
        Err(Status::unimplemented("get_latest_state not yet implemented"))
    }

    async fn get_state_range(
        &self,
        request: Request<GetStateRangeRequest>,
    ) -> Result<Response<GetStateRangeResponse>, Status> {
        Err(Status::unimplemented("get_state_range not yet implemented"))
    }

    async fn get_kv_string(
        &self,
        request: Request<GetKvStringRequest>,
    ) -> Result<Response<GetKvStringResponse>, Status> {
        Err(Status::unimplemented("get_kv_string not yet implemented"))
    }

    async fn set_kv_string(
        &self,
        request: Request<SetKvStringRequest>,
    ) -> Result<Response<SetKvStringResponse>, Status> {
        Err(Status::unimplemented("set_kv_string not yet implemented"))
    }

    async fn delete_kv_string(
        &self,
        request: Request<DeleteKvStringRequest>,
    ) -> Result<Response<DeleteKvStringResponse>, Status> {
        Err(Status::unimplemented("delete_kv_string not yet implemented"))
    }

    async fn list_kv_string_keys(
        &self,
        request: Request<ListKvStringKeysRequest>,
    ) -> Result<Response<ListKvStringKeysResponse>, Status> {
        Err(Status::unimplemented("list_kv_string_keys not yet implemented"))
    }

    async fn batch_kv_string(
        &self,
        request: Request<BatchKvStringRequest>,
    ) -> Result<Response<BatchKvStringResponse>, Status> {
        Err(Status::unimplemented("batch_kv_string not yet implemented"))
    }

    async fn get_kv_binary(
        &self,
        request: Request<GetKvBinaryRequest>,
    ) -> Result<Response<GetKvBinaryResponse>, Status> {
        Err(Status::unimplemented("get_kv_binary not yet implemented"))
    }

    async fn set_kv_binary(
        &self,
        request: Request<SetKvBinaryRequest>,
    ) -> Result<Response<SetKvBinaryResponse>, Status> {
        Err(Status::unimplemented("set_kv_binary not yet implemented"))
    }

    async fn delete_kv_binary(
        &self,
        request: Request<DeleteKvBinaryRequest>,
    ) -> Result<Response<DeleteKvBinaryResponse>, Status> {
        Err(Status::unimplemented("delete_kv_binary not yet implemented"))
    }

    async fn list_kv_binary_keys(
        &self,
        request: Request<ListKvBinaryKeysRequest>,
    ) -> Result<Response<ListKvBinaryKeysResponse>, Status> {
        Err(Status::unimplemented("list_kv_binary_keys not yet implemented"))
    }

    async fn batch_kv_binary(
        &self,
        request: Request<BatchKvBinaryRequest>,
    ) -> Result<Response<BatchKvBinaryResponse>, Status> {
        Err(Status::unimplemented("batch_kv_binary not yet implemented"))
    }

    async fn create_object(
        &self,
        request: Request<CreateObjectRequest>,
    ) -> Result<Response<CreateObjectResponse>, Status> {
        Err(Status::unimplemented("create_object not yet implemented"))
    }

    async fn update_object(
        &self,
        request: Request<UpdateObjectRequest>,
    ) -> Result<Response<UpdateObjectResponse>, Status> {
        Err(Status::unimplemented("update_object not yet implemented"))
    }

    async fn get_object(
        &self,
        request: Request<GetObjectRequest>,
    ) -> Result<Response<GetObjectResponse>, Status> {
        Err(Status::unimplemented("get_object not yet implemented"))
    }

    async fn get_object_versions(
        &self,
        request: Request<GetObjectVersionsRequest>,
    ) -> Result<Response<GetObjectVersionsResponse>, Status> {
        Err(Status::unimplemented("get_object_versions not yet implemented"))
    }

    async fn transfer_object(
        &self,
        request: Request<TransferObjectRequest>,
    ) -> Result<Response<TransferObjectResponse>, Status> {
        Err(Status::unimplemented("transfer_object not yet implemented"))
    }

    async fn request_locks(
        &self,
        request: Request<RequestLocksRequest>,
    ) -> Result<Response<RequestLocksResponse>, Status> {
        Err(Status::unimplemented("request_locks not yet implemented"))
    }

    async fn release_locks(
        &self,
        request: Request<ReleaseLocksRequest>,
    ) -> Result<Response<ReleaseLocksResponse>, Status> {
        Err(Status::unimplemented("release_locks not yet implemented"))
    }

    async fn get_lock_status(
        &self,
        request: Request<GetLockStatusRequest>,
    ) -> Result<Response<GetLockStatusResponse>, Status> {
        Err(Status::unimplemented("get_lock_status not yet implemented"))
    }

    async fn get_lock_queue(
        &self,
        request: Request<GetLockQueueRequest>,
    ) -> Result<Response<GetLockQueueResponse>, Status> {
        Err(Status::unimplemented("get_lock_queue not yet implemented"))
    }

    async fn create_job(
        &self,
        request: Request<CreateJobRequest>,
    ) -> Result<Response<CreateJobResponse>, Status> {
        Err(Status::unimplemented("create_job not yet implemented"))
    }

    async fn start_job(
        &self,
        request: Request<StartJobRequest>,
    ) -> Result<Response<StartJobResponse>, Status> {
        Err(Status::unimplemented("start_job not yet implemented"))
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        Err(Status::unimplemented("complete_job not yet implemented"))
    }

    async fn fail_job(
        &self,
        request: Request<FailJobRequest>,
    ) -> Result<Response<FailJobResponse>, Status> {
        Err(Status::unimplemented("fail_job not yet implemented"))
    }

    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobResponse>, Status> {
        Err(Status::unimplemented("get_job not yet implemented"))
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        Err(Status::unimplemented("list_jobs not yet implemented"))
    }

    async fn get_pending_jobs(
        &self,
        request: Request<GetPendingJobsRequest>,
    ) -> Result<Response<GetPendingJobsResponse>, Status> {
        Err(Status::unimplemented("get_pending_jobs not yet implemented"))
    }
}