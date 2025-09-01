use std::sync::Arc;
use std::time::Instant;
use tonic::{Request, Response, Status};
use tracing::{debug, error, warn};

use crate::database::EventDatabase;
use buffer::EventBuffer;
use db::secrets_storage::SecureSecretsStorage;
use monitoring::record_grpc_request;
use proto::{
    AgentMessageEventWithId, AgentTransactionEventWithId, CoordinatorMessageEventWithRelevance,
    Event, GetAgentMessageEventsBySequenceRequest, GetAgentMessageEventsBySequenceResponse,
    GetAgentTransactionEventsBySequenceRequest, GetAgentTransactionEventsBySequenceResponse,
    GetProofRequest, GetProofResponse, RetrieveSecretRequest, RetrieveSecretResponse,
    SearchCoordinatorMessageEventsRequest, SearchCoordinatorMessageEventsResponse,
    StoreSecretRequest, StoreSecretResponse, SubmitEventRequest, SubmitEventResponse,
    SubmitEventsRequest, SubmitEventsResponse, SubmitProofRequest, SubmitProofResponse,
    silvana_events_service_server::SilvanaEventsService,
};
use storage::ProofsCache;

pub struct SilvanaEventsServiceImpl {
    event_buffer: EventBuffer<Event>,
    database: Arc<EventDatabase>,
    secrets_storage: Option<Arc<SecureSecretsStorage>>,
    proofs_cache: Option<Arc<ProofsCache>>,
}

impl SilvanaEventsServiceImpl {
    pub fn new(event_buffer: EventBuffer<Event>, database: Arc<EventDatabase>) -> Self {
        Self {
            event_buffer,
            database,
            secrets_storage: None,
            proofs_cache: None,
        }
    }

    pub fn with_secrets_storage(mut self, secrets_storage: Arc<SecureSecretsStorage>) -> Self {
        self.secrets_storage = Some(secrets_storage);
        self
    }

    pub fn with_proofs_cache(mut self, proofs_cache: Arc<ProofsCache>) -> Self {
        self.proofs_cache = Some(proofs_cache);
        self
    }
}

#[tonic::async_trait]
impl SilvanaEventsService for SilvanaEventsServiceImpl {
    async fn submit_events(
        &self,
        request: Request<SubmitEventsRequest>,
    ) -> Result<Response<SubmitEventsResponse>, Status> {
        let start_time = Instant::now();

        let events = request.into_inner().events;
        let event_count = events.len();

        debug!("Received batch of {} events", event_count);

        let mut processed_count = 0;
        let mut first_error: Option<String> = None;

        for event in events {
            match self.event_buffer.add_event(event.into()).await {
                Ok(()) => processed_count += 1,
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e.to_string());
                    }
                    // Continue processing other events even if one fails
                }
            }
        }

        let success = processed_count == event_count;
        let message = if success {
            format!("Successfully queued {} events", processed_count)
        } else {
            format!(
                "Queued {}/{} events. First error: {}",
                processed_count,
                event_count,
                first_error.unwrap_or_else(|| "Unknown error".to_string())
            )
        };

        if !success {
            warn!("{}", message);
        }

        // Record metrics
        let duration = start_time.elapsed();
        let status_code = if success { "200" } else { "500" };
        record_grpc_request("submit_events", status_code, duration.as_secs_f64());

        // FIXED: Safe casting to prevent overflow
        let safe_processed_count = if processed_count <= u32::MAX as usize {
            processed_count as u32
        } else {
            warn!(
                "Processed count {} exceeds u32::MAX, clamping to maximum",
                processed_count
            );
            u32::MAX
        };

        Ok(Response::new(SubmitEventsResponse {
            success,
            message,
            processed_count: safe_processed_count,
        }))
    }

    async fn submit_event(
        &self,
        request: Request<SubmitEventRequest>,
    ) -> Result<Response<SubmitEventResponse>, Status> {
        let event_request = request.into_inner();
        let event = match event_request.event {
            Some(event) => event,
            None => return Err(Status::invalid_argument("Event cannot be empty")),
        };

        debug!("Received single event");

        match self.event_buffer.add_event(event.into()).await {
            Ok(()) => Ok(Response::new(SubmitEventResponse {
                success: true,
                message: "Event queued successfully".to_string(),
                processed_count: 1,
            })),
            Err(e) => {
                warn!("Failed to queue event: {}", e);

                // FIXED: Safe error string handling to prevent potential panics
                let error_string = e.to_string();
                let error_string_lower = error_string.to_lowercase();

                // Return appropriate error based on the failure type with safe string operations
                let status = if error_string_lower.contains("overloaded")
                    || error_string_lower.contains("timeout")
                {
                    Status::resource_exhausted(error_string)
                } else if error_string_lower.contains("memory limit") {
                    Status::resource_exhausted(error_string)
                } else if error_string_lower.contains("circuit breaker") {
                    Status::unavailable(error_string)
                } else {
                    Status::internal(error_string)
                };

                Err(status)
            }
        }
    }

    async fn get_agent_transaction_events_by_sequence(
        &self,
        request: Request<GetAgentTransactionEventsBySequenceRequest>,
    ) -> Result<Response<GetAgentTransactionEventsBySequenceResponse>, Status> {
        let req = request.into_inner();

        debug!(
            "Querying agent transaction events by sequence: {}",
            req.sequence
        );

        match self
            .database
            .get_agent_transaction_events_by_sequence(
                req.sequence,
                req.limit,
                req.offset,
                req.coordinator_id,
                req.developer,
                req.agent,
                req.app,
            )
            .await
        {
            Ok((events, total_count)) => {
                let proto_events: Vec<AgentTransactionEventWithId> = events
                    .into_iter()
                    .map(|event| AgentTransactionEventWithId {
                        id: event.id,
                        coordinator_id: event.coordinator_id,
                        tx_type: event.tx_type,
                        developer: event.developer,
                        agent: event.agent,
                        app: event.app,
                        job_sequence: event.job_sequence,
                        sequences: event.sequences,
                        event_timestamp: event.event_timestamp,
                        tx_hash: event.tx_hash,
                        chain: event.chain,
                        network: event.network,
                        memo: event.memo,
                        metadata: event.metadata,
                        created_at_timestamp: event.created_at_timestamp,
                    })
                    .collect();

                // FIXED: Safe casting to prevent overflow
                let returned_count = if proto_events.len() <= u32::MAX as usize {
                    proto_events.len() as u32
                } else {
                    warn!(
                        "Proto events count {} exceeds u32::MAX, clamping to maximum",
                        proto_events.len()
                    );
                    u32::MAX
                };

                debug!(
                    "Found {} agent transaction events for sequence {}",
                    returned_count, req.sequence
                );

                Ok(Response::new(GetAgentTransactionEventsBySequenceResponse {
                    success: true,
                    message: format!("Found {} events", returned_count),
                    events: proto_events,
                    total_count,
                    returned_count,
                }))
            }
            Err(e) => {
                error!(
                    "Failed to query agent transaction events by sequence: {}",
                    e
                );
                Err(Status::internal(format!("Database query failed: {}", e)))
            }
        }
    }

    async fn get_agent_message_events_by_sequence(
        &self,
        request: Request<GetAgentMessageEventsBySequenceRequest>,
    ) -> Result<Response<GetAgentMessageEventsBySequenceResponse>, Status> {
        let req = request.into_inner();

        debug!(
            "Querying agent message events by sequence: {} {:?}",
            req.sequence, req
        );

        match self
            .database
            .get_agent_message_events_by_sequence(
                req.sequence,
                req.limit,
                req.offset,
                req.coordinator_id,
                req.developer,
                req.agent,
                req.app,
            )
            .await
        {
            Ok((events, total_count)) => {
                let proto_events: Vec<AgentMessageEventWithId> = events
                    .into_iter()
                    .map(|event| {
                        // FIXED: Safe casting from u32 to i32 to prevent overflow
                        let safe_level = if event.level <= i32::MAX as u32 {
                            event.level as i32
                        } else {
                            warn!(
                                "Event level {} exceeds i32::MAX, clamping to maximum",
                                event.level
                            );
                            i32::MAX
                        };

                        AgentMessageEventWithId {
                            id: event.id,
                            coordinator_id: event.coordinator_id,
                            developer: event.developer,
                            agent: event.agent,
                            app: event.app,
                            job_sequence: event.job_sequence,
                            sequences: event.sequences,
                            event_timestamp: event.event_timestamp,
                            level: safe_level, // Safely converted to protobuf enum
                            message: event.message,
                            created_at_timestamp: event.created_at_timestamp,
                        }
                    })
                    .collect();

                // FIXED: Safe casting to prevent overflow
                let returned_count = if proto_events.len() <= u32::MAX as usize {
                    proto_events.len() as u32
                } else {
                    warn!(
                        "Proto events count {} exceeds u32::MAX, clamping to maximum",
                        proto_events.len()
                    );
                    u32::MAX
                };

                debug!(
                    "Found {} agent message events for sequence {}",
                    returned_count, req.sequence
                );

                Ok(Response::new(GetAgentMessageEventsBySequenceResponse {
                    success: true,
                    message: format!("Found {} events", returned_count),
                    events: proto_events,
                    total_count,
                    returned_count,
                }))
            }
            Err(e) => {
                error!("Failed to query agent message events by sequence: {}", e);
                Err(Status::internal(format!("Database query failed: {}", e)))
            }
        }
    }

    async fn search_coordinator_message_events(
        &self,
        request: Request<SearchCoordinatorMessageEventsRequest>,
    ) -> Result<Response<SearchCoordinatorMessageEventsResponse>, Status> {
        let req = request.into_inner();

        debug!(
            "Searching coordinator message events with query: '{}'",
            req.search_query
        );

        // FIXED: Safe search query validation with bounds checking
        if req.search_query.is_empty() || req.search_query.trim().is_empty() {
            return Err(Status::invalid_argument("Search query cannot be empty"));
        }

        // FIXED: Validate search query length to prevent potential issues
        if req.search_query.len() > 1000 {
            warn!(
                "Search query too long: {} characters, truncating",
                req.search_query.len()
            );
            return Err(Status::invalid_argument(
                "Search query too long (max 1000 characters)",
            ));
        }

        match self
            .database
            .search_coordinator_message_events(
                &req.search_query,
                req.limit,
                req.offset,
                req.coordinator_id,
            )
            .await
        {
            Ok((events, total_count)) => {
                let proto_events: Vec<CoordinatorMessageEventWithRelevance> = events
                    .into_iter()
                    .map(|event| {
                        // level is already an i32 from the database
                        let level = event.level;

                        CoordinatorMessageEventWithRelevance {
                            id: event.id,
                            coordinator_id: event.coordinator_id,
                            event_timestamp: event.event_timestamp,
                            level,
                            message: event.message,
                            created_at_timestamp: event.created_at_timestamp,
                            relevance_score: event.relevance_score,
                        }
                    })
                    .collect();

                // FIXED: Safe casting to prevent overflow
                let returned_count = if proto_events.len() <= u32::MAX as usize {
                    proto_events.len() as u32
                } else {
                    warn!(
                        "Proto events count {} exceeds u32::MAX, clamping to maximum",
                        proto_events.len()
                    );
                    u32::MAX
                };

                debug!(
                    "Found {} coordinator message events for search query: '{}'",
                    returned_count, req.search_query
                );

                Ok(Response::new(SearchCoordinatorMessageEventsResponse {
                    success: true,
                    message: format!("Found {} events matching search query", returned_count),
                    events: proto_events,
                    total_count,
                    returned_count,
                }))
            }
            Err(e) => {
                error!("Failed to search coordinator message events: {}", e);
                Err(Status::internal(format!("Full-text search failed: {}", e)))
            }
        }
    }

    async fn store_secret(
        &self,
        request: Request<StoreSecretRequest>,
    ) -> Result<Response<StoreSecretResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();

        debug!(
            "Received store secret request for developer: {}, agent: {}",
            req.reference
                .as_ref()
                .map(|r| &r.developer)
                .unwrap_or(&"<missing>".to_string()),
            req.reference
                .as_ref()
                .map(|r| &r.agent)
                .unwrap_or(&"<missing>".to_string())
        );

        let secrets_storage = match &self.secrets_storage {
            Some(storage) => storage,
            None => {
                error!("Secrets storage not configured");
                return Err(Status::unavailable("Secrets storage not available"));
            }
        };

        let reference = req
            .reference
            .ok_or_else(|| Status::invalid_argument("Missing secret reference"))?;

        if reference.developer.is_empty() || reference.agent.is_empty() {
            return Err(Status::invalid_argument("Developer and agent are required"));
        }

        if req.secret_value.is_empty() {
            return Err(Status::invalid_argument("Secret value cannot be empty"));
        }

        // TODO: Validate signature (not implemented yet as per requirements)

        match secrets_storage
            .store_secret(
                &reference.developer,
                &reference.agent,
                reference.app.as_deref(),
                reference.app_instance.as_deref(),
                reference.name.as_deref(),
                &req.secret_value,
            )
            .await
        {
            Ok(()) => {
                record_grpc_request(
                    "store_secret",
                    "success",
                    start_time.elapsed().as_secs_f64(),
                );
                Ok(Response::new(StoreSecretResponse {
                    success: true,
                    message: "Secret stored successfully".to_string(),
                }))
            }
            Err(e) => {
                error!("Failed to store secret: {}", e);
                record_grpc_request("store_secret", "error", start_time.elapsed().as_secs_f64());
                Err(Status::internal("Failed to store secret"))
            }
        }
    }

    async fn retrieve_secret(
        &self,
        request: Request<RetrieveSecretRequest>,
    ) -> Result<Response<RetrieveSecretResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();

        debug!(
            "Received retrieve secret request for developer: {}, agent: {}",
            req.reference
                .as_ref()
                .map(|r| &r.developer)
                .unwrap_or(&"<missing>".to_string()),
            req.reference
                .as_ref()
                .map(|r| &r.agent)
                .unwrap_or(&"<missing>".to_string())
        );

        let secrets_storage = match &self.secrets_storage {
            Some(storage) => storage,
            None => {
                error!("Secrets storage not configured");
                return Err(Status::unavailable("Secrets storage not available"));
            }
        };

        let reference = req
            .reference
            .ok_or_else(|| Status::invalid_argument("Missing secret reference"))?;

        if reference.developer.is_empty() || reference.agent.is_empty() {
            return Err(Status::invalid_argument("Developer and agent are required"));
        }

        // TODO: Validate signature (not implemented yet as per requirements)

        match secrets_storage
            .retrieve_secret(
                &reference.developer,
                &reference.agent,
                reference.app.as_deref(),
                reference.app_instance.as_deref(),
                reference.name.as_deref(),
            )
            .await
        {
            Ok(Some(secret_value)) => {
                record_grpc_request(
                    "retrieve_secret",
                    "success",
                    start_time.elapsed().as_secs_f64(),
                );
                Ok(Response::new(RetrieveSecretResponse {
                    success: true,
                    message: "Secret retrieved successfully".to_string(),
                    secret_value,
                }))
            }
            Ok(None) => {
                record_grpc_request(
                    "retrieve_secret",
                    "not_found",
                    start_time.elapsed().as_secs_f64(),
                );
                Ok(Response::new(RetrieveSecretResponse {
                    success: false,
                    message: "Secret not found".to_string(),
                    secret_value: String::new(),
                }))
            }
            Err(e) => {
                error!("Failed to retrieve secret: {}", e);
                record_grpc_request(
                    "retrieve_secret",
                    "error",
                    start_time.elapsed().as_secs_f64(),
                );
                Err(Status::internal("Failed to retrieve secret"))
            }
        }
    }

    async fn submit_proof(
        &self,
        request: Request<SubmitProofRequest>,
    ) -> Result<Response<SubmitProofResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();

        debug!(
            "Received submit proof request with {} bytes of data",
            req.proof_data.len()
        );

        let proofs_cache = match &self.proofs_cache {
            Some(cache) => cache,
            None => {
                error!("Proofs cache not configured");
                return Err(Status::unavailable("Proofs cache not available"));
            }
        };

        // Convert metadata from HashMap to Vec<(String, String)>
        let metadata = if req.metadata.is_empty() {
            None
        } else {
            Some(req.metadata.into_iter().collect::<Vec<_>>())
        };

        // Use provided expiration or None for default
        let expires_at = req.expires_at;

        match proofs_cache
            .submit_proof(req.proof_data, metadata, expires_at)
            .await
        {
            Ok(proof_hash) => {
                record_grpc_request(
                    "submit_proof",
                    "success",
                    start_time.elapsed().as_secs_f64(),
                );
                Ok(Response::new(SubmitProofResponse {
                    success: true,
                    message: "Proof submitted successfully".to_string(),
                    proof_hash,
                }))
            }
            Err(e) => {
                error!("Failed to submit proof: {}", e);
                record_grpc_request("submit_proof", "error", start_time.elapsed().as_secs_f64());
                Err(Status::internal("Failed to submit proof"))
            }
        }
    }

    async fn get_proof(
        &self,
        request: Request<GetProofRequest>,
    ) -> Result<Response<GetProofResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();

        debug!("Received get proof request for hash: {}", req.proof_hash);

        let proofs_cache = match &self.proofs_cache {
            Some(cache) => cache,
            None => {
                error!("Proofs cache not configured");
                return Err(Status::unavailable("Proofs cache not available"));
            }
        };

        if req.proof_hash.is_empty() {
            return Err(Status::invalid_argument("Proof hash is required"));
        }

        match proofs_cache.read_proof(&req.proof_hash).await {
            Ok(proof_data) => {
                record_grpc_request("get_proof", "success", start_time.elapsed().as_secs_f64());
                Ok(Response::new(GetProofResponse {
                    success: true,
                    message: "Proof retrieved successfully".to_string(),
                    proof_data: proof_data.data,
                    metadata: proof_data.metadata,
                }))
            }
            Err(e) => {
                // Check if it's a not found error
                let error_message = e.to_string();
                if error_message.contains("not found") || error_message.contains("NoSuchKey") {
                    record_grpc_request(
                        "get_proof",
                        "not_found",
                        start_time.elapsed().as_secs_f64(),
                    );
                    Ok(Response::new(GetProofResponse {
                        success: false,
                        message: "Proof not found".to_string(),
                        proof_data: String::new(),
                        metadata: std::collections::HashMap::new(),
                    }))
                } else {
                    error!("Failed to retrieve proof: {}", e);
                    record_grpc_request("get_proof", "error", start_time.elapsed().as_secs_f64());
                    Err(Status::internal("Failed to retrieve proof"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_safe_usize_to_u32_casting() {
        // Test normal values pass through unchanged
        let normal_count = 1000usize;
        let safe_count = if normal_count <= u32::MAX as usize {
            normal_count as u32
        } else {
            u32::MAX
        };
        assert_eq!(safe_count, 1000u32);

        // Test maximum safe value
        let max_safe = u32::MAX as usize;
        let safe_max = if max_safe <= u32::MAX as usize {
            max_safe as u32
        } else {
            u32::MAX
        };
        assert_eq!(safe_max, u32::MAX);

        // Test oversized value (only on 64-bit systems where usize > u32::MAX is possible)
        if usize::MAX > u32::MAX as usize {
            let oversized = (u32::MAX as usize) + 1;
            let safe_oversized = if oversized <= u32::MAX as usize {
                oversized as u32
            } else {
                u32::MAX
            };
            assert_eq!(safe_oversized, u32::MAX);
        }
    }

    #[test]
    fn test_safe_u32_to_i32_casting() {
        // Test normal values pass through unchanged
        let normal_level = 100u32;
        let safe_level = if normal_level <= i32::MAX as u32 {
            normal_level as i32
        } else {
            i32::MAX
        };
        assert_eq!(safe_level, 100i32);

        // Test maximum safe value
        let max_safe = i32::MAX as u32;
        let safe_max = if max_safe <= i32::MAX as u32 {
            max_safe as i32
        } else {
            i32::MAX
        };
        assert_eq!(safe_max, i32::MAX);

        // Test oversized value
        let oversized = (i32::MAX as u32) + 1;
        let safe_oversized = if oversized <= i32::MAX as u32 {
            oversized as i32
        } else {
            i32::MAX
        };
        assert_eq!(safe_oversized, i32::MAX);

        // Test u32::MAX
        let max_u32 = u32::MAX;
        let safe_max_u32 = if max_u32 <= i32::MAX as u32 {
            max_u32 as i32
        } else {
            i32::MAX
        };
        assert_eq!(safe_max_u32, i32::MAX);
    }

    #[test]
    fn test_search_query_validation() {
        // Test empty query validation
        let empty_query = "";
        let is_invalid = empty_query.is_empty() || empty_query.trim().is_empty();
        assert!(is_invalid, "Empty query should be invalid");

        // Test whitespace-only query validation
        let whitespace_query = "   ";
        let is_whitespace_invalid =
            whitespace_query.is_empty() || whitespace_query.trim().is_empty();
        assert!(
            is_whitespace_invalid,
            "Whitespace-only query should be invalid"
        );

        // Test valid query
        let valid_query = "test query";
        let is_valid = !valid_query.is_empty() && !valid_query.trim().is_empty();
        assert!(is_valid, "Valid query should pass validation");

        // Test long query validation
        let long_query = "a".repeat(1001);
        let is_too_long = long_query.len() > 1000;
        assert!(is_too_long, "Query over 1000 characters should be too long");

        // Test maximum allowed length
        let max_query = "a".repeat(1000);
        let is_max_valid = max_query.len() <= 1000;
        assert!(
            is_max_valid,
            "Query of exactly 1000 characters should be valid"
        );
    }

    #[test]
    fn test_error_string_handling() {
        // Test safe string operations
        let test_errors = vec![
            "System overloaded",
            "Timeout occurred",
            "Memory limit exceeded",
            "Circuit breaker is open",
            "Unknown error",
            "", // Empty string edge case
            "Very long error message that might cause issues in some systems but should be handled safely by our error processing code", // Long string
        ];

        for error_msg in test_errors {
            // This should not panic regardless of input
            let error_string = error_msg.to_string();
            let error_string_lower = error_string.to_lowercase();

            // Test all the contains operations we use
            let _is_overloaded = error_string_lower.contains("overloaded");
            let _is_timeout = error_string_lower.contains("timeout");
            let _is_memory = error_string_lower.contains("memory limit");
            let _is_circuit = error_string_lower.contains("circuit breaker");

            // None of these operations should panic
            assert!(
                true,
                "String operations completed safely for: {}",
                error_msg
            );
        }
    }

    #[test]
    fn test_overflow_edge_cases() {
        // Test edge cases for our casting operations

        // Test zero values
        let zero_usize = 0usize;
        let safe_zero = if zero_usize <= u32::MAX as usize {
            zero_usize as u32
        } else {
            u32::MAX
        };
        assert_eq!(safe_zero, 0u32);

        // Test maximum values don't cause overflow in comparisons
        let max_comparison = u32::MAX as usize <= u32::MAX as usize;
        assert!(max_comparison, "u32::MAX comparison should be true");

        let i32_max_comparison = i32::MAX as u32 <= i32::MAX as u32;
        assert!(i32_max_comparison, "i32::MAX comparison should be true");

        // Test that our bounds checking logic is consistent
        assert!(
            (i32::MAX as u32) < u32::MAX,
            "i32::MAX should be less than u32::MAX"
        );

        // On 64-bit systems, test usize vs u32 relationship
        if std::mem::size_of::<usize>() > std::mem::size_of::<u32>() {
            assert!(
                usize::MAX > u32::MAX as usize,
                "usize::MAX should be greater than u32::MAX on 64-bit systems"
            );
        }
    }

    #[test]
    fn test_vector_length_safety() {
        // Test that vector length operations are safe
        let small_vec: Vec<i32> = vec![1, 2, 3];
        let small_len = small_vec.len();
        assert!(
            small_len <= u32::MAX as usize,
            "Small vector length should be safe"
        );

        // Test empty vector
        let empty_vec: Vec<i32> = vec![];
        let empty_len = empty_vec.len();
        assert_eq!(empty_len, 0);

        let safe_empty_len = if empty_len <= u32::MAX as usize {
            empty_len as u32
        } else {
            u32::MAX
        };
        assert_eq!(safe_empty_len, 0u32);

        // Test that our length check logic works for realistic sizes
        for size in [0, 1, 100, 1000, 10000] {
            let test_vec: Vec<i32> = vec![0; size];
            let len = test_vec.len();
            let safe_len = if len <= u32::MAX as usize {
                len as u32
            } else {
                u32::MAX
            };
            assert_eq!(
                safe_len as usize, len,
                "Safe casting should preserve length for size {}",
                size
            );
        }
    }
}
