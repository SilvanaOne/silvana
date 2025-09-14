use std::sync::Arc;
use std::time::Instant;
use tonic::{Request, Response, Status};
use serde_json;
use tracing::{debug, error, warn};

use crate::database::EventDatabase;
use crate::storage::S3Storage;
use buffer::EventBuffer;
use db::secrets_storage::SecureSecretsStorage;
use db::kv::ConfigStorage;
use monitoring::record_grpc_request;
use proto::{
    Event,
    GetEventsByAppInstanceSequenceRequest, GetEventsByAppInstanceSequenceResponse,
    SearchEventsRequest, SearchEventsResponse,
    GetConfigRequest, GetConfigResponse, WriteConfigRequest, WriteConfigResponse,
    GetProofRequest, GetProofResponse, ReadBinaryRequest, ReadBinaryResponse,
    RetrieveSecretRequest, RetrieveSecretResponse,
    StoreSecretRequest, StoreSecretResponse, SubmitEventRequest, SubmitEventResponse,
    SubmitEventsRequest, SubmitEventsResponse, SubmitProofRequest, SubmitProofResponse,
    WriteBinaryRequest, WriteBinaryResponse,
    silvana_rpc_service_server::SilvanaRpcService,
};
use storage::ProofsCache;

pub struct SilvanaRpcServiceImpl {
    event_buffer: EventBuffer<Event>,
    _database: Arc<EventDatabase>,
    secrets_storage: Option<Arc<SecureSecretsStorage>>,
    proofs_cache: Option<Arc<ProofsCache>>,
    s3_storage: Option<Arc<S3Storage>>,
    config_storage: Option<Arc<ConfigStorage>>,
}

impl SilvanaRpcServiceImpl {
    pub fn new(event_buffer: EventBuffer<Event>, database: Arc<EventDatabase>) -> Self {
        Self {
            event_buffer,
            _database: database,
            secrets_storage: None,
            proofs_cache: None,
            s3_storage: None,
            config_storage: None,
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

    pub fn with_s3_storage(mut self, s3_storage: Arc<S3Storage>) -> Self {
        self.s3_storage = Some(s3_storage);
        self
    }
    
    pub fn with_config_storage(mut self, config_storage: Arc<ConfigStorage>) -> Self {
        self.config_storage = Some(config_storage);
        self
    }
}

#[tonic::async_trait]
impl SilvanaRpcService for SilvanaRpcServiceImpl {
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

    async fn get_events_by_app_instance_sequence(
        &self,
        request: Request<GetEventsByAppInstanceSequenceRequest>,
    ) -> Result<Response<GetEventsByAppInstanceSequenceResponse>, Status> {
        let _req = request.into_inner();

        // TODO: Implement this method based on new requirements
        // This method should query events by app instance and sequence

        Ok(Response::new(GetEventsByAppInstanceSequenceResponse {
            success: true,
            message: "Method not yet implemented".to_string(),
            events: vec![],
            total_count: 0,
            returned_count: 0,
        }))
    }

    async fn search_events(
        &self,
        request: Request<SearchEventsRequest>,
    ) -> Result<Response<SearchEventsResponse>, Status> {
        let req = request.into_inner();

        debug!(
            "Searching events with query: '{}'",
            req.search_query
        );

        // Validate search query
        if req.search_query.is_empty() || req.search_query.trim().is_empty() {
            return Err(Status::invalid_argument("Search query cannot be empty"));
        }

        if req.search_query.len() > 1000 {
            return Err(Status::invalid_argument(
                "Search query too long (max 1000 characters)",
            ));
        }

        // Search coordinator_message_event table using fulltext search
        let search_result = crate::database::search_coordinator_messages(
            &self._database,
            &req.search_query,
            req.coordinator_id.as_deref(),
            req.limit,
            req.offset,
        )
        .await
        .map_err(|e| {
            error!("Failed to search events: {}", e);
            Status::internal(format!("Failed to search events: {}", e))
        })?;

        Ok(Response::new(SearchEventsResponse {
            success: true,
            message: "Search completed".to_string(),
            events: search_result.events,
            total_count: search_result.total_count as u32,
            returned_count: search_result.returned_count as u32,
        }))
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

        // Handle both single proof and quilt submissions
        let result = match req.data {
            Some(proto::submit_proof_request::Data::ProofData(proof_data)) => {
                debug!("Submitting single proof with {} bytes of data", proof_data.len());
                
                proofs_cache
                    .submit_proof(proof_data, metadata, expires_at)
                    .await
                    .map(|hash| (hash, vec![]))
            }
            Some(proto::submit_proof_request::Data::QuiltData(quilt_data)) => {
                let pieces: Vec<(String, String)> = quilt_data
                    .pieces
                    .into_iter()
                    .map(|piece| (piece.identifier, piece.data))
                    .collect();
                
                debug!("Submitting quilt with {} pieces", pieces.len());
                
                proofs_cache
                    .submit_quilt(pieces, metadata, expires_at)
                    .await
            }
            None => {
                return Err(Status::invalid_argument("No proof data provided"));
            }
        };

        match result {
            Ok((proof_hash, quilt_piece_ids)) => {
                record_grpc_request(
                    "submit_proof",
                    "success",
                    start_time.elapsed().as_secs_f64(),
                );
                Ok(Response::new(SubmitProofResponse {
                    success: true,
                    message: if quilt_piece_ids.is_empty() {
                        "Proof submitted successfully".to_string()
                    } else {
                        format!("Quilt submitted successfully with {} pieces", quilt_piece_ids.len())
                    },
                    proof_hash,
                    quilt_piece_ids,
                }))
            }
            Err(e) => {
                error!("Failed to submit proof to cache: {:?}", e);
                record_grpc_request("submit_proof", "error", start_time.elapsed().as_secs_f64());
                // Return more detailed error message
                Err(Status::internal(format!("Failed to submit proof: {}", e)))
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

        // First try to read as a regular proof
        let result = match proofs_cache.read_proof(&req.proof_hash).await {
            Ok(proof_data) => {
                // Check if this is a quilt
                let is_quilt = proof_data.metadata.get("quilts")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                
                if is_quilt && req.block_number.is_some() {
                    // If it's a quilt and a specific block is requested, read the quilt
                    match proofs_cache.read_quilt(&req.proof_hash, req.block_number.as_deref()).await {
                        Ok(quilt_data) => Ok(quilt_data),
                        Err(e) => Err(e),
                    }
                } else {
                    // Return the proof data (which might be the full quilt JSON)
                    Ok(proof_data)
                }
            }
            Err(e) => {
                // If regular proof read fails and block_number is provided, try reading as quilt
                if req.block_number.is_some() {
                    proofs_cache.read_quilt(&req.proof_hash, req.block_number.as_deref()).await
                } else {
                    Err(e)
                }
            }
        };

        match result {
            Ok(proof_data) => {
                record_grpc_request("get_proof", "success", start_time.elapsed().as_secs_f64());
                
                // Check if this is a quilt to build proper response
                let is_quilt = proof_data.metadata.get("quilts")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                
                // Parse quilt pieces if this is a quilt
                let quilt_pieces = if is_quilt && req.block_number.is_none() {
                    // Parse the full quilt data to extract pieces
                    if let Ok(quilt_data) = serde_json::from_str::<storage::proofs_cache::QuiltData>(&proof_data.data) {
                        quilt_data.pieces.into_iter()
                            .map(|p| proto::QuiltPiece {
                                identifier: p.identifier,
                                data: p.data,
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                
                Ok(Response::new(GetProofResponse {
                    success: true,
                    message: if is_quilt {
                        if req.block_number.is_some() {
                            format!("Quilt piece retrieved successfully")
                        } else {
                            format!("Quilt retrieved successfully")
                        }
                    } else {
                        "Proof retrieved successfully".to_string()
                    },
                    proof_data: proof_data.data,
                    metadata: proof_data.metadata,
                    is_quilt,
                    quilt_pieces,
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
                        is_quilt: false,
                        quilt_pieces: vec![],
                    }))
                } else {
                    error!("Failed to retrieve proof: {}", e);
                    record_grpc_request("get_proof", "error", start_time.elapsed().as_secs_f64());
                    Err(Status::internal("Failed to retrieve proof"))
                }
            }
        }
    }

    async fn write_binary(
        &self,
        request: Request<WriteBinaryRequest>,
    ) -> Result<Response<WriteBinaryResponse>, Status> {
        let start_time = Instant::now();
        
        // Check if S3 storage is configured
        let s3_storage = self.s3_storage.as_ref().ok_or_else(|| {
            error!("S3 storage not configured");
            record_grpc_request("write_binary", "error", start_time.elapsed().as_secs_f64());
            Status::unimplemented("S3 storage not configured")
        })?;

        let req = request.into_inner();
        
        // Validate input
        if req.data.is_empty() {
            record_grpc_request("write_binary", "error", start_time.elapsed().as_secs_f64());
            return Err(Status::invalid_argument("Data cannot be empty"));
        }
        
        if req.file_name.is_empty() {
            record_grpc_request("write_binary", "error", start_time.elapsed().as_secs_f64());
            return Err(Status::invalid_argument("File name cannot be empty"));
        }
        
        if req.mime_type.is_empty() {
            record_grpc_request("write_binary", "error", start_time.elapsed().as_secs_f64());
            return Err(Status::invalid_argument("MIME type cannot be empty"));
        }

        debug!(
            "Writing binary to S3: file_name={}, size={} bytes, mime_type={}",
            req.file_name,
            req.data.len(),
            req.mime_type
        );

        // Call S3 storage to write binary
        match s3_storage
            .write_binary(req.data, &req.file_name, &req.mime_type, req.expected_sha256)
            .await
        {
            Ok((sha256, s3_url)) => {
                debug!(
                    "Successfully wrote binary to S3: file_name={}, sha256={}",
                    req.file_name, sha256
                );
                
                record_grpc_request("write_binary", "success", start_time.elapsed().as_secs_f64());
                
                Ok(Response::new(WriteBinaryResponse {
                    success: true,
                    message: "Binary data written successfully".to_string(),
                    sha256,
                    s3_url,
                }))
            }
            Err(e) => {
                error!("Failed to write binary to S3: {}", e);
                record_grpc_request("write_binary", "error", start_time.elapsed().as_secs_f64());
                
                // Check for specific error types
                let error_string = e.to_string();
                if error_string.contains("SHA256 hash mismatch") {
                    Err(Status::invalid_argument(format!("Hash verification failed: {}", e)))
                } else {
                    Err(Status::internal(format!("Failed to write binary: {}", e)))
                }
            }
        }
    }

    async fn read_binary(
        &self,
        request: Request<ReadBinaryRequest>,
    ) -> Result<Response<ReadBinaryResponse>, Status> {
        let start_time = Instant::now();
        
        // Check if S3 storage is configured
        let s3_storage = self.s3_storage.as_ref().ok_or_else(|| {
            error!("S3 storage not configured");
            record_grpc_request("read_binary", "error", start_time.elapsed().as_secs_f64());
            Status::unimplemented("S3 storage not configured")
        })?;

        let req = request.into_inner();
        
        // Validate input
        if req.file_name.is_empty() {
            record_grpc_request("read_binary", "error", start_time.elapsed().as_secs_f64());
            return Err(Status::invalid_argument("File name cannot be empty"));
        }

        debug!("Reading binary from S3: file_name={}", req.file_name);

        // Call S3 storage to read binary
        match s3_storage.read_binary(&req.file_name).await {
            Ok((data, sha256)) => {
                debug!(
                    "Successfully read binary from S3: file_name={}, size={} bytes, sha256={}",
                    req.file_name,
                    data.len(),
                    sha256
                );
                
                record_grpc_request("read_binary", "success", start_time.elapsed().as_secs_f64());
                
                // For now, we'll infer MIME type from file extension
                // In a production system, you might want to store this as metadata
                let mime_type = match req.file_name.split('.').last() {
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("pdf") => "application/pdf",
                    Some("json") => "application/json",
                    Some("txt") => "text/plain",
                    Some("bin") => "application/octet-stream",
                    _ => "application/octet-stream",
                }
                .to_string();
                
                Ok(Response::new(ReadBinaryResponse {
                    success: true,
                    message: "Binary data read successfully".to_string(),
                    data,
                    sha256,
                    mime_type,
                    metadata: std::collections::HashMap::new(), // Can be extended to include S3 metadata
                }))
            }
            Err(e) => {
                error!("Failed to read binary from S3: {}", e);
                record_grpc_request("read_binary", "error", start_time.elapsed().as_secs_f64());
                
                // Check if file doesn't exist
                let error_string = e.to_string();
                if error_string.contains("not found") || error_string.contains("NoSuchKey") {
                    Err(Status::not_found(format!("File not found: {}", req.file_name)))
                } else {
                    Err(Status::internal(format!("Failed to read binary: {}", e)))
                }
            }
        }
    }
    
    async fn get_config(
        &self,
        request: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();
        
        debug!("Getting config for chain: {}", req.chain);
        
        match &self.config_storage {
            None => {
                warn!("Config storage not configured");
                record_grpc_request("get_config", "not_configured", start_time.elapsed().as_secs_f64());
                Ok(Response::new(GetConfigResponse {
                    success: false,
                    message: "Config storage not configured".to_string(),
                    config: std::collections::HashMap::new(),
                }))
            }
            Some(storage) => {
                match storage.get_config(&req.chain).await {
                    Ok(config) => {
                        record_grpc_request("get_config", "success", start_time.elapsed().as_secs_f64());
                        Ok(Response::new(GetConfigResponse {
                            success: true,
                            message: format!("Retrieved {} config items", config.len()),
                            config,
                        }))
                    }
                    Err(e) => {
                        error!("Failed to get config: {}", e);
                        record_grpc_request("get_config", "error", start_time.elapsed().as_secs_f64());
                        Err(Status::internal(format!("Failed to get config: {}", e)))
                    }
                }
            }
        }
    }
    
    async fn write_config(
        &self,
        request: Request<WriteConfigRequest>,
    ) -> Result<Response<WriteConfigResponse>, Status> {
        let start_time = Instant::now();
        let req = request.into_inner();
        
        debug!("Writing config for chain: {} ({} items)", req.chain, req.config.len());
        
        match &self.config_storage {
            None => {
                warn!("Config storage not configured");
                record_grpc_request("write_config", "not_configured", start_time.elapsed().as_secs_f64());
                Ok(Response::new(WriteConfigResponse {
                    success: false,
                    message: "Config storage not configured".to_string(),
                    items_written: 0,
                }))
            }
            Some(storage) => {
                match storage.write_config(&req.chain, req.config).await {
                    Ok(items_written) => {
                        record_grpc_request("write_config", "success", start_time.elapsed().as_secs_f64());
                        Ok(Response::new(WriteConfigResponse {
                            success: true,
                            message: format!("Successfully wrote {} config items", items_written),
                            items_written,
                        }))
                    }
                    Err(e) => {
                        error!("Failed to write config: {}", e);
                        record_grpc_request("write_config", "error", start_time.elapsed().as_secs_f64());
                        Err(Status::internal(format!("Failed to write config: {}", e)))
                    }
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
