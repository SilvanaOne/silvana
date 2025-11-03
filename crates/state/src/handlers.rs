//! Service handler implementations for StateService

use anyhow::Result;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, NotSet, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::{
    auth::{verify_jwt, AuthInfo},
    concurrency::ConcurrencyController,
    database::Database,
    entity::{
        app_instance_kv_binary, app_instance_kv_string, app_instances, jobs,
        object_lock_queue, object_versions, objects, optimistic_state, proofs, state, user_actions,
        lock_request_bundle,
    },
    proto::state_service_server::StateService,
    proto::*,
    storage::PrivateStateStorage,
};

/// Helper function to log and return internal errors
fn log_internal_error(context: &str, error: impl std::fmt::Display) -> Status {
    error!("Internal error - {}: {}", context, error);
    Status::internal(format!("{}: {}", context, error))
}

/// Helper function to log and return already_exists errors
#[allow(dead_code)]
fn log_already_exists(context: &str) -> Status {
    warn!("Already exists: {}", context);
    Status::already_exists(context)
}

/// Helper function to log and return not_found errors
#[allow(dead_code)]
fn log_not_found(context: &str) -> Status {
    warn!("Not found: {}", context);
    Status::not_found(context)
}

/// Convert chrono DateTime to protobuf Timestamp
fn to_timestamp(dt: chrono::DateTime<Utc>) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// Convert prost_types::Struct to serde_json::Value
fn prost_struct_to_json(s: prost_types::Struct) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in s.fields {
        map.insert(k, prost_value_to_json(v));
    }
    serde_json::Value::Object(map)
}

/// Convert prost_types::Value to serde_json::Value
fn prost_value_to_json(v: prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match v.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::json!(n),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(b),
        Some(Kind::StructValue(s)) => prost_struct_to_json(s),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.into_iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

/// Convert serde_json::Value to prost_types::Struct
fn json_to_prost_struct(val: serde_json::Value) -> Option<prost_types::Struct> {
    if let serde_json::Value::Object(map) = val {
        let mut fields = BTreeMap::new();
        for (k, v) in map {
            fields.insert(k, json_to_prost_value(v));
        }
        Some(prost_types::Struct { fields })
    } else {
        None
    }
}

/// Convert serde_json::Value to prost_types::Value
fn json_to_prost_value(val: serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match val {
        serde_json::Value::Null => Some(Kind::NullValue(0)),
        serde_json::Value::Bool(b) => Some(Kind::BoolValue(b)),
        serde_json::Value::Number(n) => {
            Some(Kind::NumberValue(n.as_f64().unwrap_or(0.0)))
        }
        serde_json::Value::String(s) => Some(Kind::StringValue(s)),
        serde_json::Value::Array(arr) => {
            let values = arr.into_iter().map(json_to_prost_value).collect();
            Some(Kind::ListValue(prost_types::ListValue { values }))
        }
        serde_json::Value::Object(map) => {
            let mut fields = BTreeMap::new();
            for (k, v) in map {
                fields.insert(k, json_to_prost_value(v));
            }
            Some(Kind::StructValue(prost_types::Struct { fields }))
        }
    };
    prost_types::Value { kind }
}

/// Calculate the next binary prefix for range queries
/// Used to create an upper bound for prefix filtering: [prefix, next_prefix)
///
/// Returns None if the prefix is all 0xFF bytes (cannot increment further)
fn next_binary_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    if prefix.is_empty() {
        return None;
    }

    let mut next = prefix.to_vec();

    // Increment from the rightmost byte, carrying overflow left
    for i in (0..next.len()).rev() {
        if next[i] < 0xFF {
            next[i] += 1;
            return Some(next);
        }
        // This byte is 0xFF, set to 0 and continue carry
        next[i] = 0;
    }

    // All bytes were 0xFF, cannot increment
    None
}

/// StateService implementation
pub struct StateServiceImpl {
    db: Arc<Database>,
    storage: Option<PrivateStateStorage>,
    concurrency: Arc<ConcurrencyController>,
}

impl StateServiceImpl {
    pub fn new(db: Arc<Database>, storage: Option<PrivateStateStorage>, concurrency: Arc<ConcurrencyController>) -> Self {
        Self {
            db,
            storage,
            concurrency,
        }
    }

    /// Extract auth from request body
    fn extract_auth_from_body(&self, auth: &Option<JwtAuth>) -> Result<AuthInfo, Status> {
        let jwt_auth = auth.as_ref()
            .ok_or_else(|| {
                warn!("Authentication failed: Missing auth field");
                Status::unauthenticated("Missing auth field")
            })?;

        verify_jwt(&jwt_auth.token)
            .map_err(|e| {
                warn!("JWT verification failed: {}", e);
                Status::unauthenticated(format!("Invalid JWT: {}", e))
            })
    }

    /// Check app instance permission and log detailed error if missing
    fn require_app_instance_permission(&self, auth: &AuthInfo, app_instance_id: &str) -> Result<(), Status> {
        if !auth.has_app_instance_permission(app_instance_id) {
            let app_instances: Vec<String> = auth.scope_permissions.iter()
                .filter_map(|p| match p {
                    crate::auth::ScopePermission::AppInstance(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            warn!(
                "Permission denied: JWT scope missing permission for app_instance '{}'. Available app_instances in scope: {:?}",
                app_instance_id, app_instances
            );
            return Err(Status::permission_denied(
                format!("JWT scope missing permission for app_instance {}", app_instance_id),
            ));
        }
        Ok(())
    }

    /// Check object permission and log detailed error if missing
    #[allow(dead_code)]
    fn require_object_permission(&self, auth: &AuthInfo, object_id: &str) -> Result<(), Status> {
        if !auth.has_object_permission(object_id) {
            let objects: Vec<String> = auth.scope_permissions.iter()
                .filter_map(|p| match p {
                    crate::auth::ScopePermission::Object(id) => Some(id.clone()),
                    _ => None,
                })
                .collect();
            warn!(
                "Permission denied: JWT scope missing permission for object '{}'. Available objects in scope: {:?}",
                object_id, objects
            );
            return Err(Status::permission_denied(
                format!("JWT scope missing permission for object {}", object_id),
            ));
        }
        Ok(())
    }

    /// Verify ownership of app instance
    async fn verify_ownership(&self, app_instance_id: &str, auth: &AuthInfo) -> Result<(), Status> {
        let owns = self
            .db
            .verify_app_instance_ownership(app_instance_id, &auth.public_key)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if !owns {
            warn!(
                "Authorization failed: user {} is not owner of app_instance {}",
                auth.public_key, app_instance_id
            );
            return Err(Status::permission_denied(
                "Not authorized for this app instance",
            ));
        }

        Ok(())
    }

    /// Start background cleanup task for old completed jobs
    pub fn start_jobs_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400)); // Daily
            loop {
                interval.tick().await;
                if let Err(e) = self.cleanup_completed_jobs().await {
                    warn!("Failed to cleanup completed jobs: {}", e);
                }
            }
        });
    }

    /// Clean up completed jobs older than 30 days
    async fn cleanup_completed_jobs(&self) -> Result<u64, Status> {
        use sea_orm::EntityTrait;

        let cutoff = Utc::now() - chrono::Duration::days(30);

        let result = jobs::Entity::delete_many()
            .filter(jobs::Column::Status.eq("COMPLETED"))
            .filter(jobs::Column::UpdatedAt.lt(cutoff))
            .exec(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Failed to delete old completed jobs: {}", e);
                Status::internal("Cleanup failed")
            })?;

        info!("Cleaned up {} old completed jobs", result.rows_affected);
        Ok(result.rows_affected)
    }
}

#[tonic::async_trait]
impl StateService for StateServiceImpl {
    async fn create_app_instance(
        &self,
        request: Request<CreateAppInstanceRequest>,
    ) -> Result<Response<CreateAppInstanceResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Create entity
        let app_instance = app_instances::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            owner: Set(auth.public_key.clone()),
            name: Set(req.name),
            description: Set(req.description),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            // Coordination layer support fields - default values
            admin: Set(None),
            is_paused: Set(false),
            min_time_between_blocks: Set(60),
            block_number: Set(0),
            sequence: Set(0),
            last_proved_block_number: Set(0),
            last_settled_block_number: Set(0),
            last_settled_sequence: Set(0),
            last_purged_sequence: Set(0),
        };

        // Insert to database
        let result = app_instance
            .insert(self.db.connection())
            .await
            .map_err(|e| {
                if e.to_string().contains("Duplicate") {
                    Status::already_exists("App instance already exists")
                } else {
                    warn!("Database error creating app instance: {}", e);
                    Status::internal("Failed to create app instance")
                }
            })?;

        info!("Created app instance {} for owner {}", result.app_instance_id, result.owner);

        // Return response
        Ok(Response::new(CreateAppInstanceResponse {
            success: true,
            message: "App instance created successfully".to_string(),
            app_instance: Some(AppInstance {
                app_instance_id: result.app_instance_id,
                owner: result.owner,
                name: result.name,
                description: result.description,
                created_at: Some(to_timestamp(result.created_at)),
                updated_at: Some(to_timestamp(result.updated_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_app_instance(
        &self,
        request: Request<GetAppInstanceRequest>,
    ) -> Result<Response<GetAppInstanceResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Fetch from database
        let app_instance = app_instances::Entity::find_by_id(req.app_instance_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Database error fetching app instance: {}", e);
                Status::internal("Failed to fetch app instance")
            })?
            .ok_or_else(|| {
                debug!("App instance not found: app_instance_id={}", req.app_instance_id);
                Status::not_found("App instance not found")
            })?;

        debug!("Retrieved app instance {}", app_instance.app_instance_id);

        // Return response
        Ok(Response::new(GetAppInstanceResponse {
            app_instance: Some(AppInstance {
                app_instance_id: app_instance.app_instance_id,
                owner: app_instance.owner,
                name: app_instance.name,
                description: app_instance.description,
                created_at: Some(to_timestamp(app_instance.created_at)),
                updated_at: Some(to_timestamp(app_instance.updated_at)),
                metadata: app_instance.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn list_app_instances(
        &self,
        request: Request<ListAppInstancesRequest>,
    ) -> Result<Response<ListAppInstancesResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Prepare query
        let limit = req.limit.unwrap_or(100).min(1000) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        // Query instances owned by this user
        let paginator = app_instances::Entity::find()
            .filter(app_instances::Column::Owner.eq(&auth.public_key))
            .order_by_desc(app_instances::Column::CreatedAt)
            .paginate(self.db.connection(), limit);

        // Get total count
        let total = paginator.num_items()
            .await
            .map_err(|e| {
                warn!("Database error counting app instances: {}", e);
                Status::internal("Failed to count app instances")
            })? as i32;

        // Fetch the page
        let instances = paginator.fetch_page(offset / limit)
            .await
            .map_err(|e| {
                warn!("Database error listing app instances: {}", e);
                Status::internal("Failed to list app instances")
            })?;

        debug!("Listed {} app instances for owner {}", instances.len(), auth.public_key);

        // Convert to proto messages
        let app_instances = instances
            .into_iter()
            .map(|instance| AppInstance {
                app_instance_id: instance.app_instance_id,
                owner: instance.owner,
                name: instance.name,
                description: instance.description,
                created_at: Some(to_timestamp(instance.created_at)),
                updated_at: Some(to_timestamp(instance.updated_at)),
                metadata: instance.metadata.and_then(json_to_prost_struct),
            })
            .collect();

        Ok(Response::new(ListAppInstancesResponse {
            app_instances,
            total,
        }))
    }

    async fn submit_user_action(
        &self,
        request: Request<SubmitUserActionRequest>,
    ) -> Result<Response<SubmitUserActionResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Calculate action hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&req.action_data);
        let action_hash = hasher.finalize().to_vec();

        // Store action data inline (S3 offloading removed - use explicit blob storage API if needed)
        let action_data = req.action_data.clone();
        let action_da = req.action_da;

        // Start transaction with pessimistic locking for gapless sequence generation
        use sea_orm::{TransactionTrait, DatabaseBackend, Statement};
        let txn = self.db.connection().begin().await
            .map_err(|e| log_internal_error("Failed to start transaction", e))?;

        // Get or create sequence counter with FOR UPDATE lock
        // First ensure the counter exists, then select it with lock
        let _insert_result = txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            INSERT INTO action_seq (app_instance_id, next_seq)
            VALUES (?, 1)
            ON DUPLICATE KEY UPDATE next_seq = next_seq
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to initialize sequence", e))?;

        // Now get the current sequence with FOR UPDATE lock
        let seq_result = txn.query_one(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            SELECT next_seq FROM action_seq
            WHERE app_instance_id = ?
            FOR UPDATE
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to get sequence", e))?
            .ok_or_else(|| {
                error!("Failed to get sequence counter - query returned no rows");
                Status::internal("Failed to get sequence counter")
            })?;

        let current_seq: u64 = seq_result.try_get("", "next_seq")
            .map_err(|e| log_internal_error("Failed to parse sequence", e))?;

        // Create user action entity with the assigned sequence
        let user_action = user_actions::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(current_seq),
            action_type: Set(req.action_type.clone()),
            action_data: Set(action_data),
            action_hash: Set(action_hash.clone()),
            action_da: Set(action_da.clone()),
            submitter: Set(auth.public_key.clone()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            created_at: Set(Utc::now()),
        };

        // Insert user action with the assigned sequence
        let _result = user_action
            .insert(&txn)
            .await
            .map_err(|e| log_internal_error("Failed to insert action", e))?;

        // Increment the counter for next use
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            UPDATE action_seq
            SET next_seq = next_seq + 1
            WHERE app_instance_id = ?
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to update sequence", e))?;

        // Commit transaction
        txn.commit().await
            .map_err(|e| log_internal_error("Failed to commit transaction", e))?;

        info!("Submitted user action {} for app {} with sequence {}",
            req.action_type, req.app_instance_id, current_seq);

        // Return simplified response with just the sequence number
        Ok(Response::new(SubmitUserActionResponse {
            success: true,
            message: "User action submitted successfully".to_string(),
            action_sequence: current_seq as u64,
        }))
    }

    async fn get_user_actions(
        &self,
        request: Request<GetUserActionsRequest>,
    ) -> Result<Response<GetUserActionsResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query user actions in range
        let actions = user_actions::Entity::find()
            .filter(user_actions::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(user_actions::Column::Sequence.gte(req.from_sequence as i64))
            .filter(user_actions::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(user_actions::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Database error fetching user actions: {}", e);
                Status::internal("Failed to fetch user actions")
            })?;

        debug!("Retrieved {} user actions for app {} in range {}-{}",
            actions.len(), req.app_instance_id, req.from_sequence, req.to_sequence);

        // Convert to proto messages, retrieving from S3 if needed
        let mut user_actions = Vec::new();
        for action in actions {
            // Retrieve data from inline or S3
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let action_data = action.action_data.clone();

            user_actions.push(UserAction {
                app_instance_id: action.app_instance_id,
                sequence: action.sequence as u64,
                action_type: action.action_type,
                action_data,
                action_hash: action.action_hash,
                action_da: action.action_da,
                submitter: action.submitter,
                created_at: Some(to_timestamp(action.created_at)),
                metadata: action.metadata.and_then(json_to_prost_struct),
            });
        }

        Ok(Response::new(GetUserActionsResponse {
            user_actions,
        }))
    }

    async fn update_optimistic_state(
        &self,
        request: Request<UpdateOptimisticStateRequest>,
    ) -> Result<Response<UpdateOptimisticStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&req.state_data);
        let computed_hash = hasher.finalize().to_vec();

        if req.state_hash != computed_hash {
            warn!(
                "State hash mismatch for app_instance_id={}, sequence={}: client_hash={}, computed_hash={}",
                req.app_instance_id,
                req.sequence,
                hex::encode(&req.state_hash),
                hex::encode(&computed_hash)
            );
            return Err(Status::invalid_argument("State hash mismatch"));
        }

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = req.state_data.clone();
        let state_da = req.state_da;

        let (transition_data, transition_da) = if let Some(td) = req.transition_data {
            (Some(td), req.transition_da)
        } else {
            (None, req.transition_da)
        };

        let opt_state = optimistic_state::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(req.sequence),
            state_hash: Set(req.state_hash.clone()),
            state_data: Set(state_data.clone()), // Vec<u8>
            state_da: Set(state_da),
            transition_data: Set(transition_data.clone()), // Option<Vec<u8>>
            transition_da: Set(transition_da),
            commitment: Set(req.commitment),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            computed_at: Set(Utc::now()),
        };

        let result = opt_state.insert(self.db.connection()).await.map_err(|e| {
            if e.to_string().contains("Duplicate") {
                Status::already_exists(format!("Optimistic state for sequence {} exists", req.sequence))
            } else {
                warn!("Database error: {}", e);
                Status::internal("Failed to update optimistic state")
            }
        })?;

        info!("Updated optimistic state for {} sequence {}", req.app_instance_id, req.sequence);

        Ok(Response::new(UpdateOptimisticStateResponse {
            success: true,
            message: "Optimistic state updated".to_string(),
            optimistic_state: Some(OptimisticState {
                app_instance_id: result.app_instance_id,
                sequence: result.sequence as u64,
                state_hash: result.state_hash,
                state_data: state_data.clone(),
                state_da: result.state_da,
                transition_data,
                transition_da: result.transition_da,
                commitment: result.commitment,
                computed_at: Some(to_timestamp(result.computed_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_optimistic_state(
        &self,
        request: Request<GetOptimisticStateRequest>,
    ) -> Result<Response<GetOptimisticStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Build query based on whether sequence is provided
        let opt_state = match req.sequence {
            Some(sequence) => {
                // Get specific sequence
                optimistic_state::Entity::find()
                    .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
                    .filter(optimistic_state::Column::Sequence.eq(sequence as i64))
                    .one(self.db.connection())
                    .await
                    .map_err(|e| {
                        warn!("Database error: {}", e);
                        log_internal_error("Failed to fetch optimistic state", e)
                    })?
            }
            None => {
                // Get latest optimistic state (highest sequence)
                optimistic_state::Entity::find()
                    .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
                    .order_by_desc(optimistic_state::Column::Sequence)
                    .one(self.db.connection())
                    .await
                    .map_err(|e| {
                        warn!("Database error: {}", e);
                        log_internal_error("Failed to fetch latest optimistic state", e)
                    })?
            }
        };

        // Handle not found case gracefully
        let opt_state = match opt_state {
            Some(state) => state,
            None => {
                let msg = if req.sequence.is_some() {
                    format!("Optimistic state not found for sequence {}", req.sequence.unwrap())
                } else {
                    "No optimistic state found for this app instance".to_string()
                };
                return Ok(Response::new(GetOptimisticStateResponse {
                    success: false,
                    message: msg,
                    optimistic_state: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = opt_state.state_data.clone();
        let transition_data = opt_state.transition_data.clone();

        Ok(Response::new(GetOptimisticStateResponse {
            success: true,
            message: "Optimistic state found".to_string(),
            optimistic_state: Some(OptimisticState {
                app_instance_id: opt_state.app_instance_id,
                sequence: opt_state.sequence as u64,
                state_hash: opt_state.state_hash,
                state_data,
                state_da: opt_state.state_da,
                transition_data,
                transition_da: opt_state.transition_da,
                commitment: opt_state.commitment,
                computed_at: Some(to_timestamp(opt_state.computed_at)),
                metadata: opt_state.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn submit_proved_state(
        &self,
        request: Request<SubmitProvedStateRequest>,
    ) -> Result<Response<SubmitProvedStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let (state_data, state_da) = if let Some(sd) = req.state_data {
            (Some(sd), req.state_da)
        } else {
            (None, req.state_da)
        };

        let (proof_data, proof_da) = if let Some(pd) = req.proof_data {
            (Some(pd), req.proof_da)
        } else {
            (None, req.proof_da)
        };

        let proved_state = state::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            sequence: Set(req.sequence),
            state_hash: Set(req.state_hash.clone()),
            state_data: Set(state_data.clone()), // Option<Vec<u8>>
            state_da: Set(state_da),
            proof_data: Set(proof_data.clone()), // Option<Vec<u8>>
            proof_da: Set(proof_da),
            proof_hash: Set(req.proof_hash),
            commitment: Set(req.commitment),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            proved_at: Set(Utc::now()),
        };

        let result = proved_state.insert(self.db.connection()).await.map_err(|e| {
            if e.to_string().contains("Duplicate") {
                Status::already_exists(format!("Proved state for sequence {} exists", req.sequence))
            } else {
                warn!("Database error: {}", e);
                Status::internal("Failed to submit proved state")
            }
        })?;

        info!("Submitted proved state for {} sequence {}", req.app_instance_id, req.sequence);

        Ok(Response::new(SubmitProvedStateResponse {
            success: true,
            message: "Proved state submitted".to_string(),
            state: Some(State {
                app_instance_id: result.app_instance_id,
                sequence: result.sequence as u64,
                state_hash: result.state_hash,
                state_data: state_data.clone(),
                state_da: result.state_da,
                proof_data: proof_data.clone(),
                proof_da: result.proof_da,
                proof_hash: result.proof_hash,
                commitment: result.commitment,
                proved_at: Some(to_timestamp(result.proved_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let proved_state = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.eq(req.sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Proved state not found"))?;

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = proved_state.state_data.clone();

        use crate::proto::get_state_response;
        Ok(Response::new(GetStateResponse {
            state: Some(get_state_response::State::ProvedState(State {
                app_instance_id: proved_state.app_instance_id,
                sequence: proved_state.sequence as u64,
                state_hash: proved_state.state_hash,
                state_data,
                state_da: proved_state.state_da,
                proof_data: None, // Don't return proof by default
                proof_da: proved_state.proof_da,
                proof_hash: proved_state.proof_hash,
                commitment: proved_state.commitment,
                proved_at: Some(to_timestamp(proved_state.proved_at)),
                metadata: proved_state.metadata.and_then(json_to_prost_struct),
            })),
        }))
    }

    async fn get_latest_state(
        &self,
        request: Request<GetLatestStateRequest>,
    ) -> Result<Response<GetLatestStateResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let latest = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .order_by_desc(state::Column::Sequence)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Handle not found case gracefully
        let latest = match latest {
            Some(state) => state,
            None => {
                return Ok(Response::new(GetLatestStateResponse {
                    success: false,
                    message: "No proved state found for this app instance".to_string(),
                    state: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let state_data = latest.state_data.clone();

        Ok(Response::new(GetLatestStateResponse {
            success: true,
            message: "Latest proved state found".to_string(),
            state: Some(State {
                app_instance_id: latest.app_instance_id,
                sequence: latest.sequence as u64,
                state_hash: latest.state_hash,
                state_data,
                state_da: latest.state_da,
                proof_data: None,
                proof_da: latest.proof_da,
                proof_hash: latest.proof_hash,
                commitment: latest.commitment,
                proved_at: Some(to_timestamp(latest.proved_at)),
                metadata: latest.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_state_range(
        &self,
        request: Request<GetStateRangeRequest>,
    ) -> Result<Response<GetStateRangeResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Query user actions in range
        let actions = user_actions::Entity::find()
            .filter(user_actions::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(user_actions::Column::Sequence.gte(req.from_sequence as i64))
            .filter(user_actions::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(user_actions::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut user_actions_result = Vec::new();
        for action in actions {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let action_data = action.action_data.clone();

            user_actions_result.push(UserAction {
                app_instance_id: action.app_instance_id,
                sequence: action.sequence as u64,
                action_type: action.action_type,
                action_data,
                action_hash: action.action_hash,
                action_da: action.action_da,
                submitter: action.submitter,
                created_at: Some(to_timestamp(action.created_at)),
                metadata: action.metadata.and_then(json_to_prost_struct),
            });
        }

        // Query optimistic states in range
        let opt_states = optimistic_state::Entity::find()
            .filter(optimistic_state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(optimistic_state::Column::Sequence.gte(req.from_sequence as i64))
            .filter(optimistic_state::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(optimistic_state::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut optimistic_states_result = Vec::new();
        for opt in opt_states {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let state_data = opt.state_data.clone();
            let transition_data = opt.transition_data.clone();

            optimistic_states_result.push(OptimisticState {
                app_instance_id: opt.app_instance_id,
                sequence: opt.sequence as u64,
                state_hash: opt.state_hash,
                state_data,
                state_da: opt.state_da,
                transition_data,
                transition_da: opt.transition_da,
                commitment: opt.commitment,
                computed_at: Some(to_timestamp(opt.computed_at)),
                metadata: opt.metadata.and_then(json_to_prost_struct),
            });
        }

        // Query proved states in range
        let states = state::Entity::find()
            .filter(state::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(state::Column::Sequence.gte(req.from_sequence as i64))
            .filter(state::Column::Sequence.lte(req.to_sequence as i64))
            .order_by_asc(state::Column::Sequence)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut proved_states_result = Vec::new();
        for s in states {
            // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
            let state_data = s.state_data.clone();
            let proof_data = if req.include_proofs {
                s.proof_data.clone()
            } else {
                None
            };

            proved_states_result.push(State {
                app_instance_id: s.app_instance_id,
                sequence: s.sequence as u64,
                state_hash: s.state_hash,
                state_data,
                state_da: s.state_da,
                proof_data,
                proof_da: s.proof_da,
                proof_hash: s.proof_hash,
                commitment: s.commitment,
                proved_at: Some(to_timestamp(s.proved_at)),
                metadata: s.metadata.and_then(json_to_prost_struct),
            });
        }

        Ok(Response::new(GetStateRangeResponse {
            user_actions: user_actions_result,
            optimistic_states: optimistic_states_result,
            proved_states: proved_states_result,
        }))
    }

    async fn get_kv_string(
        &self,
        request: Request<GetKvStringRequest>,
    ) -> Result<Response<GetKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let entry = app_instance_kv_string::Entity::find()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_string::Column::Key.eq(&req.key))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        match entry {
            Some(entry) => Ok(Response::new(GetKvStringResponse {
                success: true,
                message: "Key found".to_string(),
                entry: Some(KvStringEntry {
                    app_instance_id: entry.app_instance_id,
                    key: entry.key,
                    value: entry.value,
                    created_at: Some(to_timestamp(entry.created_at)),
                    updated_at: Some(to_timestamp(entry.updated_at)),
                }),
            })),
            None => Ok(Response::new(GetKvStringResponse {
                success: false,
                message: format!("Key '{}' not found", req.key),
                entry: None,
            })),
        }
    }

    async fn set_kv_string(
        &self,
        request: Request<SetKvStringRequest>,
    ) -> Result<Response<SetKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sea_orm::sea_query::OnConflict;
        let entry = app_instance_kv_string::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            key: Set(req.key.clone()),
            value: Set(req.value.clone()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        app_instance_kv_string::Entity::insert(entry)
            .on_conflict(
                OnConflict::columns(vec![
                    app_instance_kv_string::Column::AppInstanceId,
                    app_instance_kv_string::Column::Key,
                ])
                .update_columns(vec![
                    app_instance_kv_string::Column::Value,
                    app_instance_kv_string::Column::UpdatedAt,
                ])
                .to_owned()
            )
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(Response::new(SetKvStringResponse {
            success: true,
            message: "KV string set successfully".to_string(),
        }))
    }

    async fn delete_kv_string(
        &self,
        request: Request<DeleteKvStringRequest>,
    ) -> Result<Response<DeleteKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let result = app_instance_kv_string::Entity::delete_many()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_string::Column::Key.eq(&req.key))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if result.rows_affected == 0 {
            debug!(
                "KV string key not found for deletion: app_instance_id={}, key={}",
                req.app_instance_id, req.key
            );
            return Err(Status::not_found("Key not found"));
        }

        Ok(Response::new(DeleteKvStringResponse {
            success: true,
            message: "KV string deleted successfully".to_string(),
        }))
    }

    async fn list_kv_string_keys(
        &self,
        request: Request<ListKvStringKeysRequest>,
    ) -> Result<Response<ListKvStringKeysResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000);

        let mut query = app_instance_kv_string::Entity::find()
            .filter(app_instance_kv_string::Column::AppInstanceId.eq(&req.app_instance_id));

        // Apply prefix filter if provided
        if let Some(prefix) = req.prefix {
            query = query.filter(app_instance_kv_string::Column::Key.starts_with(&prefix));
        }

        let paginator = query
            .order_by_asc(app_instance_kv_string::Column::Key)
            .paginate(self.db.connection(), limit as u64);

        let entries = paginator.fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let keys: Vec<String> = entries.into_iter().map(|e| e.key).collect();

        Ok(Response::new(ListKvStringKeysResponse {
            keys,
        }))
    }

    async fn batch_kv_string(
        &self,
        request: Request<BatchKvStringRequest>,
    ) -> Result<Response<BatchKvStringResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        use sea_orm::sea_query::OnConflict;
        let now = Utc::now();
        let mut sets_processed = 0;
        let mut deletes_processed = 0;

        // Process SET operations
        for set_req in req.sets {
            // Verify ownership for each request
            self.verify_ownership(&set_req.app_instance_id, &auth).await?;

            let entry = app_instance_kv_string::ActiveModel {
                app_instance_id: Set(set_req.app_instance_id),
                key: Set(set_req.key),
                value: Set(set_req.value),
                created_at: Set(now),
                updated_at: Set(now),
            };

            app_instance_kv_string::Entity::insert(entry)
                .on_conflict(
                    OnConflict::columns(vec![
                        app_instance_kv_string::Column::AppInstanceId,
                        app_instance_kv_string::Column::Key,
                    ])
                    .update_columns(vec![
                        app_instance_kv_string::Column::Value,
                        app_instance_kv_string::Column::UpdatedAt,
                    ])
                    .to_owned()
                )
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            sets_processed += 1;
        }

        // Process DELETE operations
        for del_req in req.deletes {
            // Verify ownership for each request
            self.verify_ownership(&del_req.app_instance_id, &auth).await?;

            app_instance_kv_string::Entity::delete_many()
                .filter(app_instance_kv_string::Column::AppInstanceId.eq(&del_req.app_instance_id))
                .filter(app_instance_kv_string::Column::Key.eq(&del_req.key))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            deletes_processed += 1;
        }

        Ok(Response::new(BatchKvStringResponse {
            success: true,
            message: format!("Processed {} sets and {} deletes", sets_processed, deletes_processed),
            sets_processed,
            deletes_processed,
        }))
    }

    async fn get_kv_binary(
        &self,
        request: Request<GetKvBinaryRequest>,
    ) -> Result<Response<GetKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let entry = app_instance_kv_binary::Entity::find()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_binary::Column::Key.eq(req.key.clone()))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        match entry {
            Some(entry) => Ok(Response::new(GetKvBinaryResponse {
                success: true,
                message: "Key found".to_string(),
                entry: Some(KvBinaryEntry {
                    app_instance_id: entry.app_instance_id,
                    key: entry.key.clone(),
                    value: entry.value,
                    value_da: entry.value_da,
                    created_at: Some(to_timestamp(entry.created_at)),
                    updated_at: Some(to_timestamp(entry.updated_at)),
                }),
            })),
            None => Ok(Response::new(GetKvBinaryResponse {
                success: false,
                message: "Key not found".to_string(),
                entry: None,
            })),
        }
    }

    async fn set_kv_binary(
        &self,
        request: Request<SetKvBinaryRequest>,
    ) -> Result<Response<SetKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        use sea_orm::sea_query::OnConflict;
        let entry = app_instance_kv_binary::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            key: Set(req.key.clone()),
            value: Set(req.value.clone()),
            value_da: Set(req.value_da.clone()),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        };

        app_instance_kv_binary::Entity::insert(entry)
            .on_conflict(
                OnConflict::columns(vec![
                    app_instance_kv_binary::Column::AppInstanceId,
                    app_instance_kv_binary::Column::Key,
                ])
                .update_columns(vec![
                    app_instance_kv_binary::Column::Value,
                    app_instance_kv_binary::Column::UpdatedAt,
                ])
                .to_owned()
            )
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        Ok(Response::new(SetKvBinaryResponse {
            success: true,
            message: "KV binary set successfully".to_string(),
        }))
    }

    async fn delete_kv_binary(
        &self,
        request: Request<DeleteKvBinaryRequest>,
    ) -> Result<Response<DeleteKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let result = app_instance_kv_binary::Entity::delete_many()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(app_instance_kv_binary::Column::Key.eq(req.key.clone()))
            .exec(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        if result.rows_affected == 0 {
            debug!(
                "KV binary key not found for deletion: app_instance_id={}, key={}",
                req.app_instance_id, hex::encode(&req.key)
            );
            return Err(Status::not_found("Key not found"));
        }

        Ok(Response::new(DeleteKvBinaryResponse {
            success: true,
            message: "KV binary deleted successfully".to_string(),
        }))
    }

    async fn list_kv_binary_keys(
        &self,
        request: Request<ListKvBinaryKeysRequest>,
    ) -> Result<Response<ListKvBinaryKeysResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000);

        let mut query = app_instance_kv_binary::Entity::find()
            .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&req.app_instance_id));

        // Apply prefix filter using database-level range query
        if let Some(prefix) = &req.prefix {
            // Lower bound: key >= prefix
            query = query.filter(app_instance_kv_binary::Column::Key.gte(prefix.clone()));

            // Upper bound: key < next_prefix (if computable)
            if let Some(next_prefix) = next_binary_prefix(prefix) {
                query = query.filter(app_instance_kv_binary::Column::Key.lt(next_prefix));
            }
            // If next_prefix is None (all 0xFF), no upper bound needed
            // The query will return all keys >= prefix
        }

        let paginator = query
            .order_by_asc(app_instance_kv_binary::Column::Key)
            .paginate(self.db.connection(), limit as u64);

        let entries = paginator.fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        let keys: Vec<Vec<u8>> = entries.into_iter().map(|e| e.key).collect();

        Ok(Response::new(ListKvBinaryKeysResponse {
            keys,
        }))
    }

    async fn batch_kv_binary(
        &self,
        request: Request<BatchKvBinaryRequest>,
    ) -> Result<Response<BatchKvBinaryResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        use sea_orm::sea_query::OnConflict;
        let now = Utc::now();
        let mut sets_processed = 0;
        let mut deletes_processed = 0;

        for set_req in req.sets {
            self.verify_ownership(&set_req.app_instance_id, &auth).await?;

            let entry = app_instance_kv_binary::ActiveModel {
                app_instance_id: Set(set_req.app_instance_id),
                key: Set(set_req.key),
                value: Set(set_req.value),
                value_da: Set(set_req.value_da),
                created_at: Set(now),
                updated_at: Set(now),
            };

            app_instance_kv_binary::Entity::insert(entry)
                .on_conflict(
                    OnConflict::columns(vec![
                        app_instance_kv_binary::Column::AppInstanceId,
                        app_instance_kv_binary::Column::Key,
                    ])
                    .update_columns(vec![
                        app_instance_kv_binary::Column::Value,
                        app_instance_kv_binary::Column::UpdatedAt,
                    ])
                    .to_owned()
                )
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            sets_processed += 1;
        }

        for del_req in req.deletes {
            self.verify_ownership(&del_req.app_instance_id, &auth).await?;

            app_instance_kv_binary::Entity::delete_many()
                .filter(app_instance_kv_binary::Column::AppInstanceId.eq(&del_req.app_instance_id))
                .filter(app_instance_kv_binary::Column::Key.eq(del_req.key.clone()))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            deletes_processed += 1;
        }

        Ok(Response::new(BatchKvBinaryResponse {
            success: true,
            message: format!("Processed {} sets and {} deletes", sets_processed, deletes_processed),
            sets_processed,
            deletes_processed,
        }))
    }

    async fn create_object(
        &self,
        request: Request<CreateObjectRequest>,
    ) -> Result<Response<CreateObjectResponse>, Status> {
        let req = request.into_inner();

        // Validate authentication
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object data (use empty if not provided)
        let object_data_bytes = req.object_data.unwrap_or_default();

        // Calculate object hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&object_data_bytes);
        let object_hash = hasher.finalize().to_vec();

        // Store object data inline (S3 offloading removed - use explicit blob storage API if needed)
        let object_data = Some(object_data_bytes.clone());
        let object_da = req.object_da;

        // Create object entity with initial version 1
        let object = objects::ActiveModel {
            object_id: Set(req.object_id.clone()),
            version: Set(1),
            owner: Set(auth.public_key.clone()),  // Owner is from JWT auth
            object_type: Set(req.object_type),
            shared: Set(req.shared),
            object_data: Set(object_data.clone()),
            object_da: Set(object_da.clone()),
            object_hash: Set(object_hash.clone()),
            previous_tx: Set(Option::<String>::None),  // No previous transaction for new object
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
        };

        // Insert to database
        let result = object
            .insert(self.db.connection())
            .await
            .map_err(|e| {
                if e.to_string().contains("Duplicate") {
                    Status::already_exists(format!("Object {} already exists", req.object_id))
                } else {
                    warn!("Database error creating object: {}", e);
                    Status::internal("Failed to create object")
                }
            })?;

        // Also create initial version record
        let version = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(result.object_id.clone()),
            version: Set(1),
            object_data: Set(object_data),
            object_da: Set(object_da),
            object_hash: Set(object_hash),
            owner: Set(result.owner.clone()),
            object_type: Set(result.object_type.clone()),
            shared: Set(result.shared),
            previous_tx: Set(Option::<String>::None),
            created_at: Set(Utc::now()),
        };

        version.insert(self.db.connection())
            .await
            .map_err(|e| {
                warn!("Failed to create object version record: {}", e);
                Status::internal("Failed to create object version")
            })?;

        info!("Created object {} with version 1 for owner {}",
            result.object_id, result.owner);

        Ok(Response::new(CreateObjectResponse {
            success: true,
            message: "Object created successfully".to_string(),
            object: Some(Object {
                object_id: result.object_id,
                version: result.version as u64,
                owner: result.owner,
                object_type: result.object_type,
                shared: result.shared,
                object_data: Some(object_data_bytes), // Return original data
                object_da: result.object_da,
                object_hash: result.object_hash,
                previous_tx: result.previous_tx,
                created_at: Some(to_timestamp(result.created_at)),
                updated_at: Some(to_timestamp(result.updated_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn update_object(
        &self,
        request: Request<UpdateObjectRequest>,
    ) -> Result<Response<UpdateObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get primary app instance from scope for concurrency control
        let app_instance_id = auth.get_primary_app_instance()
            .ok_or_else(|| {
                warn!(
                    "No app_instance permission in JWT scope for update_object (requester: {})",
                    auth.public_key
                );
                Status::permission_denied("No app_instance permission in scope")
            })?;

        let object_data_bytes = req.object_data.unwrap_or_default();
        let object_id_clone = req.object_id.clone();

        // Use optimistic concurrency control
        let result = self.concurrency.optimistic_update(
            &app_instance_id,
            &req.object_id,
            req.expected_version,
            |_txn| {
                Box::pin(async move {
                    Ok(())
                })
            }
        ).await;

        if result.is_err() {
            return Err(Status::failed_precondition("Version mismatch - object was modified"));
        }

        // Get the updated object
        let object = objects::Entity::find_by_id(object_id_clone)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        // Verify ownership
        if object.owner != auth.public_key {
            warn!(
                "Object ownership check failed: object_id={}, owner={}, requester={}",
                object.object_id, object.owner, auth.public_key
            );
            return Err(Status::permission_denied("Not the object owner"));
        }

        // Calculate new hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&object_data_bytes);
        let object_hash = hasher.finalize().to_vec();

        // Store object data inline (S3 offloading removed - use explicit blob storage API if needed)
        let inline_data = Some(object_data_bytes.clone());
        let final_object_da = req.object_da;

        // Update object
        let mut active_model: objects::ActiveModel = object.clone().into();
        active_model.version = Set(object.version + 1);
        active_model.object_data = Set(inline_data.clone());
        active_model.object_da = Set(final_object_da.clone());
        active_model.object_hash = Set(object_hash.clone());
        active_model.updated_at = Set(Utc::now());

        let updated = active_model.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to update object", e))?;

        // Create version record
        let version_record = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(updated.object_id.clone()),
            version: Set(updated.version),
            object_data: Set(inline_data),
            object_da: Set(final_object_da),
            object_hash: Set(object_hash),
            owner: Set(updated.owner.clone()),
            object_type: Set(updated.object_type.clone()),
            shared: Set(updated.shared),
            previous_tx: Set(updated.previous_tx.clone()),
            created_at: Set(Utc::now()),
        };

        version_record.insert(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to create version", e))?;

        info!("Updated object {} to version {}", updated.object_id, updated.version);

        Ok(Response::new(UpdateObjectResponse {
            success: true,
            message: "Object updated successfully".to_string(),
            object: Some(Object {
                object_id: updated.object_id,
                version: updated.version as u64,
                owner: updated.owner,
                object_type: updated.object_type,
                shared: updated.shared,
                object_data: Some(object_data_bytes),
                object_da: updated.object_da,
                object_hash: updated.object_hash,
                previous_tx: updated.previous_tx,
                created_at: Some(to_timestamp(updated.created_at)),
                updated_at: Some(to_timestamp(updated.updated_at)),
                metadata: updated.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_object(
        &self,
        request: Request<GetObjectRequest>,
    ) -> Result<Response<GetObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object (version-specific or latest)
        let object_result = if let Some(version) = req.version {
            object_versions::Entity::find()
                .filter(object_versions::Column::ObjectId.eq(&req.object_id))
                .filter(object_versions::Column::Version.eq(version as i64))
                .one(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?
        } else {
            // Get latest from objects table then convert to version format
            let obj = objects::Entity::find_by_id(&req.object_id)
                .one(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Database error", e))?;

            obj.map(|obj| {
                // Check access: owner or shared
                if obj.owner != auth.public_key && !obj.shared {
                    return None;
                }

                // Convert to version model structure for uniform handling
                Some(object_versions::Model {
                    id: 0,
                    object_id: obj.object_id,
                    version: obj.version,
                    object_data: obj.object_data,
                    object_da: obj.object_da,
                    object_hash: obj.object_hash,
                    owner: obj.owner,
                    object_type: obj.object_type,
                    shared: obj.shared,
                    previous_tx: obj.previous_tx,
                    created_at: obj.created_at,
                })
            }).flatten()
        };

        // Handle not found case
        let object = match object_result {
            Some(obj) => obj,
            None => {
                let msg = if let Some(v) = req.version {
                    format!("Object version {} not found", v)
                } else {
                    "Object not found".to_string()
                };
                return Ok(Response::new(GetObjectResponse {
                    success: false,
                    message: msg,
                    object: None,
                }));
            }
        };

        // Use inline data directly (S3 offloading removed - use explicit blob storage API if needed)
        let object_data = object.object_data.clone();

        Ok(Response::new(GetObjectResponse {
            success: true,
            message: "Object found".to_string(),
            object: Some(Object {
                object_id: object.object_id,
                version: object.version as u64,
                owner: object.owner,
                object_type: object.object_type,
                shared: object.shared,
                object_data,
                object_da: object.object_da,
                object_hash: object.object_hash,
                previous_tx: object.previous_tx,
                created_at: Some(to_timestamp(object.created_at)),
                updated_at: None, // Versions don't track updated_at
                metadata: None, // Object versions don't have metadata, only current objects do
            }),
        }))
    }

    async fn get_object_versions(
        &self,
        request: Request<GetObjectVersionsRequest>,
    ) -> Result<Response<GetObjectVersionsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if user has access to this object
        let obj = objects::Entity::find_by_id(&req.object_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        if obj.owner != auth.public_key && !obj.shared {
            warn!(
                "Access denied to object versions: object_id={}, owner={}, requester={}, shared={}",
                obj.object_id, obj.owner, auth.public_key, obj.shared
            );
            return Err(Status::permission_denied("Access denied"));
        }

        let limit = req.limit.unwrap_or(100).min(1000) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        let paginator = object_versions::Entity::find()
            .filter(object_versions::Column::ObjectId.eq(&req.object_id))
            .order_by_desc(object_versions::Column::Version)
            .paginate(self.db.connection(), limit);

        let total = paginator.num_items().await
            .map_err(|e| log_internal_error("Database error", e))? as i32;

        let versions_data = paginator.fetch_page(offset / limit).await
            .map_err(|e| log_internal_error("Database error", e))?;

        let mut versions = Vec::new();
        for ver in versions_data {
            versions.push(ObjectVersion {
                object_id: ver.object_id,
                version: ver.version as u64,
                object_data: ver.object_data,
                object_da: ver.object_da,
                object_hash: ver.object_hash,
                owner: ver.owner,
                object_type: ver.object_type,
                shared: ver.shared,
                previous_tx: ver.previous_tx,
                created_at: Some(to_timestamp(ver.created_at)),
            });
        }

        Ok(Response::new(GetObjectVersionsResponse {
            versions,
            total,
        }))
    }

    async fn transfer_object(
        &self,
        request: Request<TransferObjectRequest>,
    ) -> Result<Response<TransferObjectResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get object
        let object = objects::Entity::find_by_id(&req.object_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Object not found"))?;

        // Verify current owner
        if object.owner != auth.public_key {
            warn!(
                "Transfer object failed: object_id={}, owner={}, requester={}",
                object.object_id, object.owner, auth.public_key
            );
            return Err(Status::permission_denied("Only the owner can transfer this object"));
        }

        // Update owner
        let mut active_model: objects::ActiveModel = object.clone().into();
        active_model.owner = Set(req.new_owner.clone());
        active_model.version = Set(object.version + 1);
        active_model.updated_at = Set(Utc::now());

        let updated = active_model.update(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to transfer object", e))?;

        // Create version record for transfer
        let version_record = object_versions::ActiveModel {
            id: NotSet,
            object_id: Set(updated.object_id.clone()),
            version: Set(updated.version),
            object_data: Set(object.object_data.clone()),
            object_da: Set(object.object_da.clone()),
            object_hash: Set(object.object_hash.clone()),
            owner: Set(updated.owner.clone()),
            object_type: Set(updated.object_type.clone()),
            shared: Set(updated.shared),
            previous_tx: Set(object.previous_tx.clone()),
            created_at: Set(Utc::now()),
        };

        version_record.insert(self.db.connection()).await
            .map_err(|e| log_internal_error("Failed to create version", e))?;

        info!("Transferred object {} to {}", updated.object_id, updated.owner);

        Ok(Response::new(TransferObjectResponse {
            success: true,
            message: format!("Object transferred to {}", req.new_owner),
            object: Some(Object {
                object_id: updated.object_id,
                version: updated.version as u64,
                owner: updated.owner,
                object_type: updated.object_type,
                shared: updated.shared,
                object_data: object.object_data,
                object_da: updated.object_da,
                object_hash: updated.object_hash,
                previous_tx: updated.previous_tx,
                created_at: Some(to_timestamp(updated.created_at)),
                updated_at: Some(to_timestamp(updated.updated_at)),
                metadata: updated.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn request_locks(
        &self,
        request: Request<RequestLocksRequest>,
    ) -> Result<Response<RequestLocksResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Get primary app instance from scope
        let app_instance_id = auth.get_primary_app_instance()
            .ok_or_else(|| {
                warn!(
                    "No app_instance permission in JWT scope for request_locks (requester: {})",
                    auth.public_key
                );
                Status::permission_denied("No app_instance permission in scope")
            })?;

        // Sort object IDs for canonical ordering (deadlock prevention)
        let mut object_ids = req.object_ids.clone();
        object_ids.sort();

        let now = Utc::now();

        // Create bundle record
        let bundle = lock_request_bundle::ActiveModel {
            req_id: Set(req.req_id.clone()),
            app_instance_id: Set(app_instance_id.clone()),
            object_ids: Set(serde_json::to_value(&object_ids).unwrap()),
            object_count: Set(object_ids.len() as i32),
            transaction_type: Set(req.transaction_type.clone()),
            created_at: Set(now),
            started_at: Set(Some(now)),
            granted_at: NotSet,
            released_at: NotSet,
            status: Set("ACQUIRING".to_string()),
            wait_time_ms: NotSet,
            hold_time_ms: NotSet,
        };
        bundle.insert(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to create lock bundle", e)
        })?;

        // Queue lock requests for each object in sorted order
        for object_id in &object_ids {
            let lock_entry = object_lock_queue::ActiveModel {
                object_id: Set(object_id.clone()),
                req_id: Set(req.req_id.clone()),
                app_instance_id: Set(app_instance_id.clone()),
                retry_count: Set(0),
                queued_at: Set(now),
                lease_until: NotSet,  // Will be set when granted
                lease_granted_at: NotSet,
                status: Set("WAITING".to_string()),
            };
            lock_entry.insert(self.db.connection()).await.map_err(|e| {
                Status::internal(format!("Failed to queue lock for {}: {}", object_id, e))
            })?;
        }

        // Attempt to acquire the bundle (with timeout)
        let acquired = self.concurrency.attempt_acquire_bundle(
            &req.req_id,
            req.timeout_seconds,
        ).await.map_err(|e| {
            log_internal_error("Failed to acquire locks", e)
        })?;

        if !acquired {
            // Timeout - clean up queue entries
            object_lock_queue::Entity::delete_many()
                .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
                .exec(self.db.connection())
                .await
                .map_err(|e| log_internal_error("Failed to clean up queue", e))?;

            return Ok(Response::new(RequestLocksResponse {
                success: false,
                message: format!("Lock acquisition timed out after {} seconds", req.timeout_seconds),
                bundle: None,
            }));
        }

        // Reload bundle to get updated status
        let updated_bundle = lock_request_bundle::Entity::find_by_id(&req.req_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| {
                error!("Bundle disappeared - req_id: {}", req.req_id);
                Status::internal("Bundle disappeared")
            })?;

        // Convert bundle to proto response
        let proto_bundle = LockRequestBundle {
            req_id: updated_bundle.req_id.clone(),
            app_instance_id: updated_bundle.app_instance_id.clone(),
            object_ids: object_ids.clone(),
            object_count: updated_bundle.object_count,
            transaction_type: updated_bundle.transaction_type.clone(),
            created_at: Some(to_timestamp(updated_bundle.created_at)),
            started_at: updated_bundle.started_at.map(to_timestamp),
            granted_at: updated_bundle.granted_at.map(to_timestamp),
            released_at: None,
            status: BundleStatus::Granted.into(),
            wait_time_ms: updated_bundle.wait_time_ms,
            hold_time_ms: None,
        };

        Ok(Response::new(RequestLocksResponse {
            success: true,
            message: format!("Successfully acquired locks for {} objects", object_ids.len()),
            bundle: Some(proto_bundle),
        }))
    }

    async fn release_locks(
        &self,
        request: Request<ReleaseLocksRequest>,
    ) -> Result<Response<ReleaseLocksResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Find the bundle
        let bundle = lock_request_bundle::Entity::find_by_id(req.req_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Lock bundle not found"))?;

        // Update all locks in the bundle to RELEASED
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        for lock in lock_entries {
            let mut active: object_lock_queue::ActiveModel = lock.into();
            active.status = Set("RELEASED".to_string());
            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to release lock", e)
            })?;
        }

        // Update bundle status
        let now = Utc::now();
        let mut active_bundle: lock_request_bundle::ActiveModel = bundle.into();
        active_bundle.status = Set("RELEASED".to_string());
        active_bundle.released_at = Set(Some(now));
        active_bundle.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update bundle", e)
        })?;

        Ok(Response::new(ReleaseLocksResponse {
            success: true,
            message: format!("Released locks for bundle {}", req.req_id),
        }))
    }

    async fn get_lock_status(
        &self,
        request: Request<GetLockStatusRequest>,
    ) -> Result<Response<GetLockStatusResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Find the bundle
        let bundle = lock_request_bundle::Entity::find_by_id(req.req_id.clone())
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Lock bundle not found"))?;

        // Get all queue entries for this bundle
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ReqId.eq(&req.req_id))
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Convert to proto
        let queue_entries: Vec<LockQueueEntry> = lock_entries
            .into_iter()
            .map(|entry| {
                let status = match entry.status.as_str() {
                    "WAITING" => LockStatus::Waiting,
                    "GRANTED" => LockStatus::Granted,
                    "EXPIRED" => LockStatus::Expired,
                    "RELEASED" => LockStatus::Released,
                    _ => LockStatus::Unspecified,
                };

                LockQueueEntry {
                    object_id: entry.object_id,
                    req_id: entry.req_id,
                    app_instance_id: entry.app_instance_id,
                    retry_count: entry.retry_count,
                    queued_at: Some(to_timestamp(entry.queued_at)),
                    lease_until: entry.lease_until.map(to_timestamp),
                    lease_granted_at: entry.lease_granted_at.map(to_timestamp),
                    status: status.into(),
                }
            })
            .collect();

        // Parse object_ids from JSON
        let object_ids: Vec<String> = serde_json::from_value(bundle.object_ids.clone())
            .unwrap_or_default();

        let bundle_status = match bundle.status.as_str() {
            "QUEUED" => BundleStatus::Queued,
            "ACQUIRING" => BundleStatus::Acquiring,
            "GRANTED" => BundleStatus::Granted,
            "RELEASED" => BundleStatus::Released,
            "TIMEOUT" => BundleStatus::Timeout,
            "FAILED" => BundleStatus::Failed,
            _ => BundleStatus::Unspecified,
        };

        let proto_bundle = LockRequestBundle {
            req_id: bundle.req_id,
            app_instance_id: bundle.app_instance_id,
            object_ids,
            object_count: bundle.object_count,
            transaction_type: bundle.transaction_type,
            created_at: Some(to_timestamp(bundle.created_at)),
            started_at: bundle.started_at.map(to_timestamp),
            granted_at: bundle.granted_at.map(to_timestamp),
            released_at: bundle.released_at.map(to_timestamp),
            status: bundle_status.into(),
            wait_time_ms: bundle.wait_time_ms,
            hold_time_ms: bundle.hold_time_ms,
        };

        Ok(Response::new(GetLockStatusResponse {
            bundle: Some(proto_bundle),
            queue_entries,
        }))
    }

    async fn get_lock_queue(
        &self,
        request: Request<GetLockQueueRequest>,
    ) -> Result<Response<GetLockQueueResponse>, Status> {
        let req = request.into_inner();
        let _auth = self.extract_auth_from_body(&req.auth)?;

        // Get all queue entries for this object, ordered by queued_at (FIFO)
        let lock_entries = object_lock_queue::Entity::find()
            .filter(object_lock_queue::Column::ObjectId.eq(&req.object_id))
            .order_by_asc(object_lock_queue::Column::QueuedAt)
            .all(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Convert to proto
        let queue_entries: Vec<LockQueueEntry> = lock_entries
            .into_iter()
            .map(|entry| {
                let status = match entry.status.as_str() {
                    "WAITING" => LockStatus::Waiting,
                    "GRANTED" => LockStatus::Granted,
                    "EXPIRED" => LockStatus::Expired,
                    "RELEASED" => LockStatus::Released,
                    _ => LockStatus::Unspecified,
                };

                LockQueueEntry {
                    object_id: entry.object_id,
                    req_id: entry.req_id,
                    app_instance_id: entry.app_instance_id,
                    retry_count: entry.retry_count,
                    queued_at: Some(to_timestamp(entry.queued_at)),
                    lease_until: entry.lease_until.map(to_timestamp),
                    lease_granted_at: entry.lease_granted_at.map(to_timestamp),
                    status: status.into(),
                }
            })
            .collect();

        Ok(Response::new(GetLockQueueResponse {
            queue_entries,
        }))
    }

    async fn create_job(
        &self,
        request: Request<CreateJobRequest>,
    ) -> Result<Response<CreateJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let now = Utc::now();

        // Calculate next_scheduled_at for periodic jobs
        let next_scheduled_at = if let Some(interval_ms) = req.interval_ms {
            Some(now + chrono::Duration::milliseconds(interval_ms as i64))
        } else {
            None
        };

        // Convert sequences to JSON
        let sequences_json = if !req.sequences.is_empty() {
            Some(serde_json::to_value(&req.sequences).unwrap())
        } else {
            None
        };
        let sequences1_json = if !req.sequences1.is_empty() {
            Some(serde_json::to_value(&req.sequences1).unwrap())
        } else {
            None
        };
        let sequences2_json = if !req.sequences2.is_empty() {
            Some(serde_json::to_value(&req.sequences2).unwrap())
        } else {
            None
        };

        // Start transaction with pessimistic locking for gapless sequence generation
        use sea_orm::{TransactionTrait, DatabaseBackend, Statement};
        let txn = self.db.connection().begin().await
            .map_err(|e| log_internal_error("Failed to start transaction", e))?;

        // Get or create sequence counter with FOR UPDATE lock
        let _insert_result = txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            INSERT INTO job_seq (app_instance_id, next_seq)
            VALUES (?, 1)
            ON DUPLICATE KEY UPDATE next_seq = next_seq
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to initialize job sequence", e))?;

        // Get current sequence with FOR UPDATE lock
        let seq_result = txn.query_one(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            SELECT next_seq FROM job_seq
            WHERE app_instance_id = ?
            FOR UPDATE
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to get job sequence", e))?
            .ok_or_else(|| {
                error!("Failed to get job sequence counter - query returned no rows");
                Status::internal("Failed to get job sequence counter")
            })?;

        let current_seq: u64 = seq_result.try_get("", "next_seq")
            .map_err(|e| log_internal_error("Failed to parse job sequence", e))?;

        // Create job with assigned sequence
        let job = jobs::ActiveModel {
            app_instance_id: Set(req.app_instance_id.clone()),
            job_sequence: Set(current_seq),
            description: Set(req.description.clone()),
            developer: Set(req.developer.clone()),
            agent: Set(req.agent.clone()),
            agent_method: Set(req.agent_method.clone()),
            block_number: Set(req.block_number),
            sequences: Set(sequences_json.clone()),
            sequences1: Set(sequences1_json.clone()),
            sequences2: Set(sequences2_json.clone()),
            data: Set(req.data.clone()),
            data_da: Set(req.data_da.clone()),
            agent_jwt: Set(req.agent_jwt.clone()),
            jwt_expires_at: Set(req.jwt_expires_at.as_ref().map(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                    .expect("Invalid timestamp")
                    .with_timezone(&chrono::Utc)
            })),
            status: Set("PENDING".to_string()),
            error_message: NotSet,
            attempts: Set(0),
            interval_ms: Set(req.interval_ms),
            next_scheduled_at: Set(next_scheduled_at),
            created_at: Set(now),
            updated_at: Set(now),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
        };

        let result = job.insert(&txn).await
            .map_err(|e| log_internal_error("Failed to create job", e))?;

        // Increment counter for next use
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::MySql,
            r#"
            UPDATE job_seq
            SET next_seq = next_seq + 1
            WHERE app_instance_id = ?
            "#,
            vec![req.app_instance_id.clone().into()]
        )).await.map_err(|e| log_internal_error("Failed to update job sequence", e))?;

        // Commit transaction
        txn.commit().await
            .map_err(|e| log_internal_error("Failed to commit transaction", e))?;

        info!("Created job {}/{} for {}/{}",
            req.app_instance_id, current_seq, req.developer, req.agent);

        // Convert to proto
        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences: req.sequences,
            sequences1: req.sequences1,
            sequences2: req.sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Pending.into(),
            error_message: None,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(CreateJobResponse {
            success: true,
            message: "Job created successfully".to_string(),
            job_sequence: current_seq as u64,
            job: Some(proto_job),
        }))
    }

    async fn start_job(
        &self,
        request: Request<StartJobRequest>,
    ) -> Result<Response<StartJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        // Update status to RUNNING
        let mut active: jobs::ActiveModel = job.into();
        active.status = Set("RUNNING".to_string());
        active.attempts = Set(active.attempts.as_ref() + 1);
        active.updated_at = Set(Utc::now());

        let result = active.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update job", e)
        })?;

        // Convert to proto
        let sequences: Vec<u64> = result.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = result.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = result.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences,
            sequences1,
            sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Running.into(),
            error_message: result.error_message,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(StartJobResponse {
            success: true,
            message: format!("Job {}/{} started", req.app_instance_id, req.job_sequence),
            job: Some(proto_job),
        }))
    }

    async fn complete_job(
        &self,
        request: Request<CompleteJobRequest>,
    ) -> Result<Response<CompleteJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        let now = Utc::now();

        // For periodic jobs, reschedule. For one-time jobs, mark as COMPLETED
        if let Some(interval_ms) = job.interval_ms {
            // Periodic job - reset to PENDING and update next_scheduled_at
            let next_scheduled_at = now + chrono::Duration::milliseconds(interval_ms as i64);

            let mut active: jobs::ActiveModel = job.into();
            active.status = Set("PENDING".to_string());
            active.next_scheduled_at = Set(Some(next_scheduled_at));
            active.updated_at = Set(now);

            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to reschedule job", e)
            })?;

            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!(
                    "Job {}/{} completed and rescheduled for {:?}",
                    req.app_instance_id, req.job_sequence, next_scheduled_at
                ),
            }))
        } else {
            // One-time job - mark as COMPLETED for audit trail
            let mut active: jobs::ActiveModel = job.into();
            active.status = Set("COMPLETED".to_string());
            active.updated_at = Set(now);

            active.update(self.db.connection()).await.map_err(|e| {
                log_internal_error("Failed to mark job as completed", e)
            })?;

            Ok(Response::new(CompleteJobResponse {
                success: true,
                message: format!("Job {}/{} marked as completed", req.app_instance_id, req.job_sequence),
            }))
        }
    }

    async fn fail_job(
        &self,
        request: Request<FailJobRequest>,
    ) -> Result<Response<FailJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?
            .ok_or_else(|| Status::not_found("Job not found"))?;

        // Update status to FAILED
        let mut active: jobs::ActiveModel = job.into();
        active.status = Set("FAILED".to_string());
        active.error_message = Set(Some(req.error_message.clone()));
        active.updated_at = Set(Utc::now());

        let result = active.update(self.db.connection()).await.map_err(|e| {
            log_internal_error("Failed to update job", e)
        })?;

        // Convert to proto
        let sequences: Vec<u64> = result.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = result.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = result.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let proto_job = Job {
            app_instance_id: result.app_instance_id,
            job_sequence: result.job_sequence as u64,
            description: result.description,
            developer: result.developer,
            agent: result.agent,
            agent_method: result.agent_method,
            block_number: result.block_number,
            sequences,
            sequences1,
            sequences2,
            data: result.data,
            data_da: result.data_da,
            agent_jwt: result.agent_jwt,
            jwt_expires_at: result.jwt_expires_at.map(to_timestamp),
            status: JobStatus::Failed.into(),
            error_message: result.error_message,
            attempts: result.attempts as u32,
            interval_ms: result.interval_ms,
            next_scheduled_at: result.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(result.created_at)),
            updated_at: Some(to_timestamp(result.updated_at)),
            metadata: result.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(FailJobResponse {
            success: true,
            message: format!("Job {}/{} marked as failed", req.app_instance_id, req.job_sequence),
            job: Some(proto_job),
        }))
    }

    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        // Find the job
        let job = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::JobSequence.eq(req.job_sequence as i64))
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Database error", e))?;

        // Handle not found case
        let job = match job {
            Some(j) => j,
            None => {
                return Ok(Response::new(GetJobResponse {
                    success: false,
                    message: format!("Job {}/{} not found", req.app_instance_id, req.job_sequence),
                    job: None,
                }));
            }
        };

        // Convert to proto
        let sequences: Vec<u64> = job.sequences.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences1: Vec<u64> = job.sequences1.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sequences2: Vec<u64> = job.sequences2.as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let job_status = match job.status.as_str() {
            "PENDING" => JobStatus::Pending,
            "RUNNING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            "FAILED" => JobStatus::Failed,
            _ => JobStatus::Unspecified,
        };

        let proto_job = Job {
            app_instance_id: job.app_instance_id,
            job_sequence: job.job_sequence as u64,
            description: job.description,
            developer: job.developer,
            agent: job.agent,
            agent_method: job.agent_method,
            block_number: job.block_number,
            sequences,
            sequences1,
            sequences2,
            data: job.data,
            data_da: job.data_da,
            agent_jwt: job.agent_jwt,
            jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
            status: job_status.into(),
            error_message: job.error_message,
            attempts: job.attempts as u32,
            interval_ms: job.interval_ms,
            next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
            created_at: Some(to_timestamp(job.created_at)),
            updated_at: Some(to_timestamp(job.updated_at)),
            metadata: job.metadata.and_then(json_to_prost_struct),
        };

        Ok(Response::new(GetJobResponse {
            success: true,
            message: "Job found".to_string(),
            job: Some(proto_job),
        }))
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> Result<Response<ListJobsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let limit = req.limit.unwrap_or(100) as u64;
        let offset = req.offset.unwrap_or(0) as u64;

        // Build query with optional status filter
        let mut query = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id));

        if let Some(status) = req.status {
            let status_str = match JobStatus::try_from(status) {
                Ok(JobStatus::Pending) => "PENDING",
                Ok(JobStatus::Running) => "RUNNING",
                Ok(JobStatus::Completed) => "COMPLETED",
                Ok(JobStatus::Failed) => "FAILED",
                _ => {
                    warn!("Invalid job status value provided: {}", status);
                    return Err(Status::invalid_argument("Invalid job status"));
                }
            };
            query = query.filter(jobs::Column::Status.eq(status_str));
        }

        // Get total count
        let total = query.clone().count(self.db.connection()).await.map_err(|e| {
            log_internal_error("Database error", e)
        })? as i32;

        // Get paginated results
        let paginator = query
            .order_by_asc(jobs::Column::JobSequence)
            .paginate(self.db.connection(), limit);

        let page = offset / limit;
        let job_list = paginator.fetch_page(page).await.map_err(|e| {
            log_internal_error("Database error", e)
        })?;

        // Convert to proto
        let jobs: Vec<Job> = job_list
            .into_iter()
            .map(|job| {
                let sequences: Vec<u64> = job.sequences.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences1: Vec<u64> = job.sequences1.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences2: Vec<u64> = job.sequences2.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let job_status = match job.status.as_str() {
                    "PENDING" => JobStatus::Pending,
                    "RUNNING" => JobStatus::Running,
                    "COMPLETED" => JobStatus::Completed,
                    "FAILED" => JobStatus::Failed,
                    _ => JobStatus::Unspecified,
                };

                Job {
                    app_instance_id: job.app_instance_id,
                    job_sequence: job.job_sequence as u64,
                    description: job.description,
                    developer: job.developer,
                    agent: job.agent,
                    agent_method: job.agent_method,
                    block_number: job.block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data: job.data,
                    data_da: job.data_da,
                    agent_jwt: job.agent_jwt,
                    jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
                    status: job_status.into(),
                    error_message: job.error_message,
                    attempts: job.attempts as u32,
                    interval_ms: job.interval_ms,
                    next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
                    created_at: Some(to_timestamp(job.created_at)),
                    updated_at: Some(to_timestamp(job.updated_at)),
                    metadata: job.metadata.and_then(json_to_prost_struct),
                }
            })
            .collect();

        Ok(Response::new(ListJobsResponse {
            jobs,
            total,
        }))
    }

    async fn get_pending_jobs(
        &self,
        request: Request<GetPendingJobsRequest>,
    ) -> Result<Response<GetPendingJobsResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify scope contains permission for this app instance
        self.require_app_instance_permission(&auth, &req.app_instance_id)?;

        let now = Utc::now();
        let limit = req.limit as u64;

        // Build query for pending jobs
        let mut query = jobs::Entity::find()
            .filter(jobs::Column::AppInstanceId.eq(&req.app_instance_id))
            .filter(jobs::Column::Status.eq("PENDING"));

        // Filter by next_scheduled_at (ready to run)
        query = query.filter(
            jobs::Column::NextScheduledAt.is_null()
                .or(jobs::Column::NextScheduledAt.lte(now))
        );

        // Optional filters
        if let Some(developer) = &req.developer {
            query = query.filter(jobs::Column::Developer.eq(developer));
        }
        if let Some(agent) = &req.agent {
            query = query.filter(jobs::Column::Agent.eq(agent));
        }
        if let Some(agent_method) = &req.agent_method {
            query = query.filter(jobs::Column::AgentMethod.eq(agent_method));
        }

        // Get jobs ordered by job_sequence
        let paginator = query
            .order_by_asc(jobs::Column::JobSequence)
            .paginate(self.db.connection(), limit);

        let job_list = paginator.fetch_page(0).await.map_err(|e| {
            log_internal_error("Database error", e)
        })?;

        // Convert to proto
        let jobs: Vec<Job> = job_list
            .into_iter()
            .map(|job| {
                let sequences: Vec<u64> = job.sequences.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences1: Vec<u64> = job.sequences1.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let sequences2: Vec<u64> = job.sequences2.as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                Job {
                    app_instance_id: job.app_instance_id,
                    job_sequence: job.job_sequence as u64,
                    description: job.description,
                    developer: job.developer,
                    agent: job.agent,
                    agent_method: job.agent_method,
                    block_number: job.block_number,
                    sequences,
                    sequences1,
                    sequences2,
                    data: job.data,
                    data_da: job.data_da,
                    agent_jwt: job.agent_jwt,
                    jwt_expires_at: job.jwt_expires_at.map(to_timestamp),
                    status: JobStatus::Pending.into(),
                    error_message: job.error_message,
                    attempts: job.attempts as u32,
                    interval_ms: job.interval_ms,
                    next_scheduled_at: job.next_scheduled_at.map(to_timestamp),
                    created_at: Some(to_timestamp(job.created_at)),
                    updated_at: Some(to_timestamp(job.updated_at)),
                    metadata: job.metadata.and_then(json_to_prost_struct),
                }
            })
            .collect();

        Ok(Response::new(GetPendingJobsResponse {
            jobs,
        }))
    }

    async fn store_private_blob(
        &self,
        request: Request<StorePrivateBlobRequest>,
    ) -> Result<Response<StorePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Verify scope permission for the resource
        match req.resource_type.as_str() {
            "app_instance" => {
                self.require_app_instance_permission(&auth, &req.resource_id)?;
            }
            "object" => {
                if !auth.has_object_permission(&req.resource_id) {
                    return Err(Status::permission_denied(
                        format!("Missing object permission for {}", req.resource_id)
                    ));
                }
            }
            _ => {
                warn!(
                    "Invalid resource_type for blob storage: provided='{}', expected='app_instance' or 'object'",
                    req.resource_type
                );
                return Err(Status::invalid_argument(
                    "resource_type must be 'app_instance' or 'object'"
                ));
            }
        }

        // Store blob with owner metadata - key format: resource_type/resource_id/hash
        let key = storage
            .store(
                &req.data,
                &auth.public_key,
                &req.resource_type,
                &req.resource_id,
            )
            .await
            .map_err(|e| log_internal_error("Failed to store blob", e))?;

        // Calculate hash for response
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&req.data);
        let content_hash = hex::encode(hash);

        info!(
            "Stored private blob: {} ({} bytes, owner: {})",
            key,
            req.data.len(),
            auth.public_key
        );

        Ok(Response::new(StorePrivateBlobResponse {
            success: true,
            message: "Blob stored successfully".to_string(),
            key,
            size_bytes: req.data.len() as u64,
            content_hash,
        }))
    }

    async fn retrieve_private_blob(
        &self,
        request: Request<RetrievePrivateBlobRequest>,
    ) -> Result<Response<RetrievePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Retrieve blob with authentication
        let data = storage
            .retrieve(
                &req.key,
                &auth.public_key,
                &auth.scope_permissions,
            )
            .await
            .map_err(|e| {
                if e.to_string().contains("Access denied") {
                    Status::permission_denied(e.to_string())
                } else {
                    log_internal_error("Failed to retrieve blob", e)
                }
            })?;

        info!(
            "Retrieved private blob: {} ({} bytes, requester: {})",
            req.key,
            data.len(),
            auth.public_key
        );

        Ok(Response::new(RetrievePrivateBlobResponse {
            success: true,
            message: "Blob retrieved successfully".to_string(),
            data,
        }))
    }

    async fn delete_private_blob(
        &self,
        request: Request<DeletePrivateBlobRequest>,
    ) -> Result<Response<DeletePrivateBlobResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Check if S3 storage is configured
        let storage = self.storage.as_ref()
            .ok_or_else(|| Status::unavailable("Blob storage is not configured"))?;

        // Delete blob (owner only)
        storage
            .delete(&req.key, &auth.public_key)
            .await
            .map_err(|e| {
                if e.to_string().contains("Access denied") {
                    Status::permission_denied(e.to_string())
                } else {
                    log_internal_error("Failed to delete blob", e)
                }
            })?;

        info!(
            "Deleted private blob: {} (requester: {})",
            req.key,
            auth.public_key
        );

        Ok(Response::new(DeletePrivateBlobResponse {
            success: true,
            message: "Blob deleted successfully".to_string(),
        }))
    }

    // =========================================================================
    // Proof Management
    // =========================================================================

    async fn submit_proof(
        &self,
        request: Request<SubmitProofRequest>,
    ) -> Result<Response<SubmitProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        // Store data inline (S3 offloading removed - use explicit blob storage API if needed)
        let (claim_data, claim_da) = if let Some(cd) = req.claim_data {
            (Some(cd), req.claim_da)
        } else {
            (None, req.claim_da)
        };

        let (proof_data, proof_da) = if let Some(pd) = req.proof_data {
            (Some(pd), req.proof_da)
        } else {
            (None, req.proof_da)
        };

        let proof = proofs::ActiveModel {
            id: NotSet,
            app_instance_id: Set(req.app_instance_id.clone()),
            proof_type: Set(req.proof_type.clone()),
            claim_json: Set(prost_struct_to_json(req.claim_json.unwrap_or_default())),
            claim_hash: Set(req.claim_hash),
            claim_data: Set(claim_data.clone()),
            claim_da: Set(claim_da.clone()),
            proof_data: Set(proof_data.clone()),
            proof_da: Set(proof_da.clone()),
            proof_hash: Set(req.proof_hash),
            proof_time: Set(req.proof_time.map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap())),
            metadata: Set(req.metadata.map(prost_struct_to_json)),
            proved_at: Set(Utc::now()),
        };

        let result = proof.insert(self.db.connection()).await.map_err(|e| {
            warn!("Database error inserting proof: {}", e);
            log_internal_error("Failed to insert proof", e)
        })?;

        info!(
            "Submitted proof: id={}, app_instance={}, type={}",
            result.id, result.app_instance_id, result.proof_type
        );

        Ok(Response::new(SubmitProofResponse {
            success: true,
            message: "Proof submitted successfully".to_string(),
            proof: Some(Proof {
                id: result.id,
                app_instance_id: result.app_instance_id,
                proof_type: result.proof_type,
                claim_json: Some(json_to_prost_struct(result.claim_json).unwrap_or_default()),
                claim_hash: result.claim_hash,
                claim_data: result.claim_data,
                claim_da: result.claim_da,
                proof_data: result.proof_data,
                proof_da: result.proof_da,
                proof_hash: result.proof_hash,
                proof_time: result.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(result.proved_at)),
                metadata: result.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Find the proof
        let proof = proofs::Entity::find_by_id(req.proof_id)
            .one(self.db.connection())
            .await
            .map_err(|e| log_internal_error("Failed to query proof", e))?
            .ok_or_else(|| Status::not_found(format!("Proof {} not found", req.proof_id)))?;

        // Verify ownership via app_instance
        self.verify_ownership(&proof.app_instance_id, &auth).await?;

        Ok(Response::new(GetProofResponse {
            success: true,
            message: "Proof retrieved successfully".to_string(),
            proof: Some(Proof {
                id: proof.id,
                app_instance_id: proof.app_instance_id,
                proof_type: proof.proof_type,
                claim_json: Some(json_to_prost_struct(proof.claim_json).unwrap_or_default()),
                claim_hash: proof.claim_hash,
                claim_data: proof.claim_data,
                claim_da: proof.claim_da,
                proof_data: proof.proof_data,
                proof_da: proof.proof_da,
                proof_hash: proof.proof_hash,
                proof_time: proof.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(proof.proved_at)),
                metadata: proof.metadata.and_then(json_to_prost_struct),
            }),
        }))
    }

    async fn list_proofs(
        &self,
        request: Request<ListProofsRequest>,
    ) -> Result<Response<ListProofsResponse>, Status> {
        use sea_orm::Condition;

        let req = request.into_inner();
        let auth = self.extract_auth_from_body(&req.auth)?;

        // Verify ownership of app_instance
        self.verify_ownership(&req.app_instance_id, &auth).await?;

        let limit = req.limit.unwrap_or(100).min(1000) as u64;

        // Build keyset pagination condition
        let mut condition = Condition::all()
            .add(proofs::Column::AppInstanceId.eq(&req.app_instance_id));

        // Add proof_type filter if specified
        if let Some(proof_type) = &req.proof_type {
            condition = condition.add(proofs::Column::ProofType.eq(proof_type));
        }

        // Add keyset pagination (proved_at, app_instance_id, id) > (last_proved_at, last_app_instance_id, last_id)
        if let (Some(last_proved_at), Some(last_app_instance_id), Some(last_id)) =
            (req.last_proved_at, req.last_app_instance_id, req.last_id) {

            let last_ts = chrono::DateTime::from_timestamp(last_proved_at.seconds, last_proved_at.nanos as u32)
                .ok_or_else(|| Status::invalid_argument("Invalid last_proved_at timestamp"))?;

            // Keyset pagination: (proved_at, app_instance_id, id) > (last_proved_at, last_app_instance_id, last_id)
            condition = condition.add(
                Condition::any()
                    .add(proofs::Column::ProvedAt.gt(last_ts))
                    .add(
                        Condition::all()
                            .add(proofs::Column::ProvedAt.eq(last_ts))
                            .add(proofs::Column::AppInstanceId.gt(last_app_instance_id.clone()))
                    )
                    .add(
                        Condition::all()
                            .add(proofs::Column::ProvedAt.eq(last_ts))
                            .add(proofs::Column::AppInstanceId.eq(last_app_instance_id))
                            .add(proofs::Column::Id.gt(last_id))
                    )
            );
        }

        // Query with limit + 1 to check if there are more pages
        let paginator = proofs::Entity::find()
            .filter(condition)
            .order_by_asc(proofs::Column::ProvedAt)
            .order_by_asc(proofs::Column::AppInstanceId)
            .order_by_asc(proofs::Column::Id)
            .paginate(self.db.connection(), limit + 1);

        let proofs = paginator
            .fetch_page(0)
            .await
            .map_err(|e| log_internal_error("Failed to list proofs", e))?;

        // Check if there are more pages
        let has_more = proofs.len() > limit as usize;
        let proofs = if has_more {
            &proofs[..limit as usize]
        } else {
            &proofs[..]
        };

        // Convert to proto messages
        let proto_proofs = proofs
            .iter()
            .map(|proof| Proof {
                id: proof.id,
                app_instance_id: proof.app_instance_id.clone(),
                proof_type: proof.proof_type.clone(),
                claim_json: Some(json_to_prost_struct(proof.claim_json.clone()).unwrap_or_default()),
                claim_hash: proof.claim_hash.clone(),
                claim_data: proof.claim_data.clone(),
                claim_da: proof.claim_da.clone(),
                proof_data: proof.proof_data.clone(),
                proof_da: proof.proof_da.clone(),
                proof_hash: proof.proof_hash.clone(),
                proof_time: proof.proof_time.map(to_timestamp),
                proved_at: Some(to_timestamp(proof.proved_at)),
                metadata: proof.metadata.as_ref().and_then(|m| json_to_prost_struct(m.clone())),
            })
            .collect();

        Ok(Response::new(ListProofsResponse {
            proofs: proto_proofs,
            has_more,
        }))
    }

    // ============================================================================
    // Coordinator Authentication Methods (Phase 3 - Not Yet Implemented)
    // ============================================================================

    async fn coordinator_job_operation(
        &self,
        _request: Request<CoordinatorJobRequest>,
    ) -> Result<Response<CoordinatorJobResponse>, Status> {
        Err(Status::unimplemented(
            "Coordinator job operations not yet implemented - Phase 3"
        ))
    }

    async fn grant_coordinator_access(
        &self,
        _request: Request<GrantCoordinatorAccessRequest>,
    ) -> Result<Response<()>, Status> {
        Err(Status::unimplemented(
            "Grant coordinator access not yet implemented - Phase 3"
        ))
    }

    async fn revoke_coordinator_access(
        &self,
        _request: Request<RevokeCoordinatorAccessRequest>,
    ) -> Result<Response<()>, Status> {
        Err(Status::unimplemented(
            "Revoke coordinator access not yet implemented - Phase 3"
        ))
    }

    async fn list_coordinator_access(
        &self,
        _request: Request<ListCoordinatorAccessRequest>,
    ) -> Result<Response<ListCoordinatorAccessResponse>, Status> {
        Err(Status::unimplemented(
            "List coordinator access not yet implemented - Phase 3"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_binary_prefix_basic() {
        // Basic increment
        assert_eq!(next_binary_prefix(&[0x01, 0x02]), Some(vec![0x01, 0x03]));
        assert_eq!(next_binary_prefix(&[0x00, 0x00]), Some(vec![0x00, 0x01]));
        assert_eq!(next_binary_prefix(&[0xAB, 0xCD]), Some(vec![0xAB, 0xCE]));
    }

    #[test]
    fn test_next_binary_prefix_carry() {
        // Carry from rightmost byte
        assert_eq!(next_binary_prefix(&[0x01, 0xFF]), Some(vec![0x02, 0x00]));
        assert_eq!(next_binary_prefix(&[0x00, 0xFF]), Some(vec![0x01, 0x00]));

        // Multiple carries
        assert_eq!(next_binary_prefix(&[0x01, 0xFF, 0xFF]), Some(vec![0x02, 0x00, 0x00]));
    }

    #[test]
    fn test_next_binary_prefix_overflow() {
        // All 0xFF bytes - cannot increment
        assert_eq!(next_binary_prefix(&[0xFF]), None);
        assert_eq!(next_binary_prefix(&[0xFF, 0xFF]), None);
        assert_eq!(next_binary_prefix(&[0xFF, 0xFF, 0xFF]), None);
    }

    #[test]
    fn test_next_binary_prefix_edge_cases() {
        // Empty prefix
        assert_eq!(next_binary_prefix(&[]), None);

        // Single byte
        assert_eq!(next_binary_prefix(&[0x00]), Some(vec![0x01]));
        assert_eq!(next_binary_prefix(&[0x7F]), Some(vec![0x80]));
        assert_eq!(next_binary_prefix(&[0xFE]), Some(vec![0xFF]));
    }

    #[test]
    fn test_next_binary_prefix_realistic() {
        // Realistic key prefixes
        let prefix = b"user:";
        let next = next_binary_prefix(prefix).unwrap();
        assert_eq!(next, b"user;");  // ':' + 1 = ';'

        // Namespace separator
        let prefix = b"namespace\x00";
        let next = next_binary_prefix(prefix).unwrap();
        assert_eq!(next, b"namespace\x01");
    }
}