use crate::config::Config;
use crate::constants::{
    BALANCE_CHECK_INTERVAL_SECS, COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS,
    GRACEFUL_SHUTDOWN_TIMEOUT_SECS, JOB_SEARCHER_SHUTDOWN_TIMEOUT_SECS,
    PROOF_ANALYSIS_INTERVAL_SECS, RECONCILIATION_INTERVAL_SECS, SHUTDOWN_CLEANUP_DELAY_MS,
    SHUTDOWN_PROGRESS_INTERVAL_SECS, STARTUP_RECONCILIATION_DELAY_SECS,
};
use crate::coordination_manager::CoordinationManager;
use crate::coordination_wrapper::CoordinationWrapper;
use crate::error::Result;
use crate::job_searcher::JobSearcher;
use crate::layer_config::{CoordinatorConfig, GeneralConfig, SuiConfig};
use crate::merge::{start_periodic_block_creation, start_periodic_proof_analysis};
use crate::metrics::{CoordinatorMetrics, start_metrics_reporter};
use crate::processor::EventProcessor;
use crate::state::SharedState;
// use crate::stuck_jobs::StuckJobMonitor; // Removed - reconciliation handles stuck jobs
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::task;
use tracing::{debug, error, info, warn};

/// Try to load coordinator configuration from file or environment
async fn try_load_coordinator_config(config_path: Option<&str>) -> Result<Option<CoordinatorConfig>> {
    // If explicit path provided, use it
    if let Some(path) = config_path {
        info!("Loading configuration from: {}", path);
        return Ok(Some(CoordinatorConfig::from_file(path)?));
    }

    // Check for default config file
    if Path::new("coordinator.toml").exists() {
        info!("Found coordinator.toml, loading multi-layer configuration");
        return Ok(Some(CoordinatorConfig::from_file("coordinator.toml")?));
    }

    // No config file found
    debug!("No configuration file found, will use default single-layer Sui configuration");
    Ok(None)
}

/// Create default configuration for single Sui layer
fn create_default_sui_config(rpc_url: String, package_id: String) -> Result<CoordinatorConfig> {
    Ok(CoordinatorConfig {
        coordinator: GeneralConfig {
            enable_layers: vec!["sui".to_string()],
            max_parallel_jobs: 10,
            reconciliation_interval: 30,
            enable_metrics: true,
            metrics_interval: 60,
        },
        sui: SuiConfig {
            layer_id: "sui".to_string(),
            rpc_url,
            package_id: package_id.clone(),
            registry_id: package_id, // Use same for registry
            operation_mode: "multicall".to_string(),
            multicall_interval_secs: 3,
            multicall_max_operations: 100,
            app_instance_filter: None,
        },
        private: vec![],
        ethereum: vec![],
    })
}

/// Check a single coordination layer for stuck jobs
async fn reconcile_layer(
    layer: Arc<Box<dyn CoordinationWrapper>>,
    layer_id: &str,
    state: &SharedState,
) -> Result<usize> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut stuck_count = 0;
    let current_time_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Get all app instances tracked by the coordinator
    let app_instances = state.get_app_instances().await;

    for app_instance in app_instances {
        // Check if this layer owns this app instance
        match layer.fetch_app_instance(&app_instance).await {
            Ok(_) => {
                // Fetch pending jobs for this app instance
                match layer.fetch_pending_jobs(&app_instance).await {
                    Ok(jobs) => {
                        for job in jobs {
                            if is_job_stuck(&job, current_time_ms) {
                                stuck_count += 1;
                                let error_msg = format!(
                                    "Job timed out after {} minutes in layer {}",
                                    (current_time_ms - job.updated_at) / 60000,
                                    layer_id
                                );

                                // For Sui layer, use job_sequence (numeric ID)
                                // For other layers, we'd need to handle string IDs
                                if let Ok(job_sequence) = job.id.parse::<u64>() {
                                    state.add_fail_job_request(
                                        app_instance.clone(),
                                        job_sequence,
                                        error_msg,
                                    ).await;
                                }

                                info!(
                                    "Found stuck job {} in layer {} for app_instance {}",
                                    job.id, layer_id, app_instance
                                );
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Failed to fetch jobs for {} in layer {}: {}", app_instance, layer_id, e);
                    }
                }
            }
            Err(_) => {
                // App instance not in this layer, skip
                continue;
            }
        }
    }

    Ok(stuck_count)
}

/// Check if a job is stuck (running for too long)
fn is_job_stuck(job: &silvana_coordination_trait::Job<String>, current_time_ms: u64) -> bool {
    use silvana_coordination_trait::JobStatus;

    // Only check running jobs
    if !matches!(job.status, JobStatus::Running) {
        return false;
    }

    // Job is stuck if it's been running for more than 10 minutes (600000 ms)
    const STUCK_TIMEOUT_MS: u64 = 600000;
    let running_duration_ms = current_time_ms.saturating_sub(job.updated_at);

    running_duration_ms > STUCK_TIMEOUT_MS
}

/// Multi-layer reconciliation that checks all coordination layers
async fn start_multi_layer_reconciliation(
    manager: Arc<CoordinationManager>,
    state: SharedState,
) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    info!(
        "🔄 Starting multi-layer reconciliation task (runs every {} minutes)...",
        RECONCILIATION_INTERVAL_SECS / 60
    );

    // Run immediate reconciliation on startup to catch stuck jobs
    info!("🔍 Running immediate reconciliation check on startup...");
    tokio::time::sleep(Duration::from_secs(STARTUP_RECONCILIATION_DELAY_SECS)).await; // Small delay to let the system initialize

    // Perform initial multi-layer reconciliation
    let layer_ids = manager.get_layer_ids();
    info!("Checking {} coordination layers for stuck jobs", layer_ids.len());

    let mut total_stuck = 0;
    for layer_id in &layer_ids {
        if let Some(layer) = manager.get_layer(&layer_id) {
            match reconcile_layer(layer.clone(), &layer_id, &state).await {
                Ok(stuck_count) => {
                    total_stuck += stuck_count;
                    if stuck_count > 0 {
                        info!("Found {} stuck jobs in layer {}", stuck_count, layer_id);
                    }
                }
                Err(e) => {
                    error!("Failed to reconcile layer {}: {}", layer_id, e);
                }
            }
        }
    }

    if total_stuck > 0 {
        info!("✅ Initial reconciliation complete - found {} stuck jobs across all layers", total_stuck);
    } else {
        info!("✅ Initial reconciliation complete - no stuck jobs found");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(RECONCILIATION_INTERVAL_SECS));

    // Skip the immediate first tick since we just ran reconciliation
    interval.tick().await;

    loop {
        // Check for shutdown
        if state.is_shutting_down() {
            info!("Multi-layer reconciliation task shutting down...");
            break;
        }

        interval.tick().await;

        if state.is_shutting_down() {
            break;
        }

        info!("📊 Running periodic multi-layer reconciliation check...");

        // Log current stats before reconciliation
        let stats = state.get_jobs_tracker().get_stats().await;
        debug!(
            "Starting periodic reconciliation (currently tracking {} app_instances, {} agent methods)",
            stats.app_instances_count, stats.agent_methods_count
        );

        // Iterate through all layers for reconciliation
        let layer_ids = manager.get_layer_ids();
        let mut total_stuck = 0;
        let mut has_any_pending = false;

        for layer_id in &layer_ids {
            if let Some(layer) = manager.get_layer(&layer_id) {
                match reconcile_layer(layer.clone(), &layer_id, &state).await {
                    Ok(stuck_count) => {
                        total_stuck += stuck_count;
                        if stuck_count > 0 {
                            has_any_pending = true;
                            debug!("Layer {} reconciliation: {} stuck jobs", layer_id, stuck_count);
                        }
                    }
                    Err(e) => {
                        error!("Failed to reconcile layer {}: {}", layer_id, e);
                    }
                }
            }
        }

        if total_stuck > 0 {
            debug!("Reconciliation complete - found {} stuck jobs across {} layers", total_stuck, layer_ids.len());
        } else {
            debug!("Reconciliation complete - no stuck jobs in any layer");
        }

        // Always refresh the has_pending_jobs flag so the job_searcher
        // is notified when pending jobs exist after reconciliation.
        if has_any_pending {
            state.update_pending_jobs_flag().await;
        }

        info!("✅ Multi-layer reconciliation check complete");
    }

    Ok(())
}

pub async fn start_coordinator(
    rpc_url: String,
    package_id: String,
    use_tee: bool,
    container_timeout: u64,
    grpc_socket_path: String,
    app_instance_filter: Option<String>,
    settle_only: bool,
    config_path: Option<String>,
) -> Result<()> {
    info!("--------------------------------");
    info!("🚀 Starting Silvana Coordinator");
    info!("--------------------------------");

    // ALWAYS load or create configuration for CoordinationManager
    let coordinator_config = if let Some(config) = try_load_coordinator_config(config_path.as_deref()).await? {
        info!("📊 Multi-layer mode: {} layer(s) configured", config.coordinator.enable_layers.len());
        config
    } else {
        info!("📊 Single-layer mode: Using default Sui configuration");
        create_default_sui_config(rpc_url.clone(), package_id.clone())?
    };

    // Validate configuration
    coordinator_config.validate()?;

    // ALWAYS initialize CoordinationManager (even for single Sui layer)
    let coordination_manager = Arc::new(CoordinationManager::from_config(coordinator_config.clone()).await?);
    info!("✅ Initialized coordination manager with {} layer(s)", coordination_manager.get_layer_ids().len());

    // Get the Sui layer configuration for backward compatibility
    let sui_rpc_url = coordinator_config.sui.rpc_url.clone();
    let sui_package_id = coordinator_config.sui.package_id.clone();

    info!("🔗 Primary Sui RPC URL: {}", sui_rpc_url);
    info!("📦 Registry package: {}", sui_package_id);

    if let Some(ref instance) = app_instance_filter {
        info!("🎯 Filtering jobs for app instance: {}", instance);
    }

    if settle_only {
        info!("⚖️ Running as dedicated settlement node");
    }

    // Initialize the global SharedSuiState with Sui layer's RPC URL
    sui::SharedSuiState::initialize(&sui_rpc_url).await?;

    // Get network and address info
    let env_network = sui::get_network_name();
    let address = sui::get_current_address();

    info!("✅ Connected to Sui RPC");

    // Verify chain configuration and get actual chain name
    match sui::network_info::get_service_info_full().await {
        Ok(service_info) => {
            // Use RPC chain name if provided and not "unknown", otherwise use env
            let network = service_info.chain_name.as_ref().unwrap_or(&env_network);

            info!("🌐 Network: {}", network);

            if let Some(id) = service_info.chain_id {
                info!("Chain ID: {}", id);
            }

            if let Some(version) = service_info.server_version {
                debug!("Server: {}", version);
            }

            if let Some(height) = service_info.checkpoint_height {
                info!("Checkpoint height: {}", height);
            }
        }
        Err(e) => {
            debug!("Could not get service info: {}", e);
            info!("🌐 Network: {} (from env)", env_network);
        }
    }

    info!("👤 Address: {}", address);

    // Try to get and display network stats
    match sui::get_network_summary().await {
        Ok(summary) => info!("📊 Network Stats: {}", summary),
        Err(e) => debug!("Could not fetch network stats: {}", e),
    }

    // Check balance and request from faucet if needed
    info!("🚰 Checking balance and faucet availability...");
    match sui::faucet::initialize_faucet().await {
        Ok(()) => info!("✅ Balance check complete"),
        Err(e) => warn!(
            "⚠️ Failed to check/request faucet tokens: {}. Continuing with existing balance.",
            e
        ),
    }

    // Initialize gas coin pool for better parallel transaction performance
    info!("🪙 Initializing gas coin pool...");
    match sui::coin_management::initialize_gas_coin_pool().await {
        Ok(()) => info!("✅ Gas coin pool initialized"),
        Err(e) => warn!(
            "⚠️ Failed to initialize gas coin pool: {}. System will continue with existing coins.",
            e
        ),
    }

    let config = Config {
        package_id: sui_package_id.clone(),
        modules: vec!["jobs".to_string()],
    };

    // Create shared state with coordination manager
    let state = SharedState::new_with_manager(coordination_manager.clone());

    // Set the settle_only flag
    state.set_settle_only(settle_only);

    // Set the app instance filter if provided
    if let Some(instance) = app_instance_filter {
        state.set_app_instance_filter(Some(instance)).await;
    }

    // Setup signal handlers for graceful shutdown (Ctrl-C and SIGTERM for system reboot)
    let shutdown_state = state.clone();
    let force_shutdown_state = state.clone();
    tokio::spawn(async move {
        let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
            Ok(sig) => sig,
            Err(e) => {
                error!(
                    "Failed to create SIGINT handler: {}. Graceful shutdown disabled.",
                    e
                );
                return;
            }
        };
        let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(e) => {
                error!(
                    "Failed to create SIGTERM handler: {}. Graceful shutdown disabled.",
                    e
                );
                return;
            }
        };

        tokio::select! {
            _ = sigint.recv() => {
                if shutdown_state.is_shutting_down() {
                    // Second Ctrl-C - force shutdown
                    eprintln!("🛑 Second SIGINT received - forcing immediate shutdown!");
                    error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                    shutdown_state.set_force_shutdown();
                } else {
                    // First Ctrl-C - graceful shutdown
                    eprintln!("⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    eprintln!("    Press Ctrl-C again to force immediate shutdown");
                    warn!("⚠️  Received SIGINT (Ctrl-C), initiating graceful shutdown...");
                    warn!("    Press Ctrl-C again to force immediate shutdown");
                    shutdown_state.set_shutdown();

                    // Spawn a task to listen for second Ctrl-C
                    tokio::spawn(async move {
                        match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
                            Ok(mut sigint2) => {
                                sigint2.recv().await;
                                eprintln!("🛑 Second SIGINT received - forcing immediate shutdown!");
                                error!("🛑 Second SIGINT received - forcing immediate shutdown!");
                                force_shutdown_state.set_force_shutdown();
                            }
                            Err(e) => {
                                error!("Failed to create second SIGINT handler: {}", e);
                            }
                        }
                    });
                }
            }
            _ = sigterm.recv() => {
                eprintln!("⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                warn!("⚠️  Received SIGTERM (system shutdown/reboot), initiating graceful shutdown...");
                shutdown_state.set_shutdown();
            }
        }
    });

    info!("✅ Coordinator initialized, starting services...");

    // Get coordinator ID and ethereum address for events
    let coordinator_id = sui::get_current_address();
    let ethereum_address = std::env::var("ETHEREUM_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    // Start periodic coordinator active events
    let active_state = state.clone();
    let active_coordinator_id = coordinator_id.clone();
    let active_handle = task::spawn(async move {
        info!(
            "📡 Starting coordinator active event task (sends every {} minutes)...",
            COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS / 60
        );

        let mut interval =
            tokio::time::interval(Duration::from_secs(COORDINATOR_ACTIVE_EVENT_INTERVAL_SECS));
        interval.tick().await; // Skip the first immediate tick

        loop {
            if active_state.is_shutting_down() {
                info!("Coordinator active event task shutting down");
                break;
            }

            interval.tick().await;

            let rpc_client = active_state.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::CoordinatorActive(
                        proto::CoordinatorActiveEvent {
                            coordinator_id: active_coordinator_id.clone(),
                            event_timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            debug!("Sent coordinator active event");
                        } else {
                            warn!("Failed to send coordinator active event: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send coordinator active event: {}", e);
                    }
                }
            }
        }
    });

    // 1. Start gRPC server in a separate thread
    let grpc_state = state.clone();
    let grpc_handle = task::spawn(async move {
        info!("🔌 Starting gRPC server...");
        if let Err(e) = crate::grpc::start_grpc_server(&grpc_socket_path, grpc_state).await {
            error!("gRPC server error: {}", e);
        }
    });

    // 2. Start multi-layer reconciliation task in a separate thread
    let reconciliation_state = state.clone();
    let reconciliation_manager = coordination_manager.clone();

    let reconciliation_handle = task::spawn(async move {
        if let Err(e) = start_multi_layer_reconciliation(reconciliation_manager, reconciliation_state).await {
            error!("Multi-layer reconciliation task error: {}", e);
        }
    });
    info!(
        "🔄 Started multi-layer reconciliation task (runs every {} minutes)",
        RECONCILIATION_INTERVAL_SECS / 60
    );

    // Note: Stuck job monitoring is handled by reconciliation task
    // to avoid overloading Sui nodes with duplicate checks

    // 4. Start periodic block creation task (runs every minute)
    let block_creation_state = state.clone();
    let block_creation_handle = task::spawn(async move {
        start_periodic_block_creation(block_creation_state).await;
    });
    info!("📦 Started periodic block creation task (runs every minute)");

    // 4.5 Start periodic proof analysis task
    let proof_analysis_state = state.clone();
    let proof_analysis_handle = task::spawn(async move {
        start_periodic_proof_analysis(proof_analysis_state).await;
    });
    info!(
        "🔬 Started proof completion analysis task (runs every {} minutes)",
        PROOF_ANALYSIS_INTERVAL_SECS / 60
    );

    // 5. Start metrics reporter for New Relic
    let metrics = std::sync::Arc::new(CoordinatorMetrics::new());
    let metrics_state = state.clone();
    let metrics_clone = metrics.clone();
    let jobs_tracker_clone = state.get_jobs_tracker().clone();
    let _metrics_handle = task::spawn(async move {
        start_metrics_reporter(metrics_state, metrics_clone, jobs_tracker_clone).await;
    });
    info!("📊 Started New Relic metrics reporter");

    // 6. Start periodic balance check task (runs every 30 minutes)
    let balance_check_handle = task::spawn(async move {
        info!("💰 Starting periodic balance check task (runs every 30 minutes)...");
        let mut interval = tokio::time::interval(Duration::from_secs(BALANCE_CHECK_INTERVAL_SECS));

        // Skip the immediate first tick since we already checked on startup
        interval.tick().await;

        loop {
            interval.tick().await;

            info!("💰 Running periodic balance check...");

            // Check balance and request from faucet if below 5 SUI (topup early to avoid running out)
            match sui::faucet::ensure_sufficient_balance(10.0).await {
                Ok(requested) => {
                    if requested {
                        info!("✅ Balance was low, requested tokens from faucet");
                    } else {
                        debug!("✅ Balance check complete - sufficient balance available");
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to check/request faucet tokens: {}", e);
                }
            }

            // Also check if we need to split coins for the pool
            if let Err(e) = sui::coin_management::ensure_gas_coin_pool().await {
                warn!("⚠️ Failed to maintain gas coin pool: {}", e);
            }
        }
    });
    info!("💰 Started periodic balance check task (runs every 30 minutes)");

    // 6. Start job searcher in a separate thread (only collects jobs and adds to multicall queue)
    let job_searcher_state = state.clone();
    let job_searcher_metrics = metrics.clone();
    let job_searcher_handle = task::spawn(async move {
        let mut job_searcher = match JobSearcher::new(job_searcher_state) {
            Ok(searcher) => searcher,
            Err(e) => {
                error!("Failed to create job searcher: {}", e);
                return;
            }
        };

        // Set metrics reference
        job_searcher.set_metrics(job_searcher_metrics);

        // Delay job searcher startup by 10 seconds to allow system initialization
        info!("Job searcher waiting 10 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(10)).await;
        info!("Job searcher starting now");

        if let Err(e) = job_searcher.run().await {
            error!("Job searcher error: {}", e);
        }
    });
    info!("🔍 Started job searcher thread");

    // 7. Start multicall processor in a separate thread (processes multicall queue and executes batches)
    let multicall_state = state.clone();
    let multicall_metrics = metrics.clone();
    let multicall_handle = task::spawn(async move {
        let mut multicall_processor = crate::multicall::MulticallProcessor::new(multicall_state);
        multicall_processor.set_metrics(multicall_metrics);

        // Delay multicall processor startup by 5 seconds
        info!("Multicall processor waiting 5 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(5)).await;
        info!("Multicall processor starting now");

        if let Err(e) = multicall_processor.run().await {
            error!("Multicall processor error: {}", e);
        }
    });
    info!("🚀 Started multicall processor thread");

    // 8. Start docker buffer processor in a separate thread (processes started jobs buffer and launches containers)
    let docker_state = state.clone();
    let docker_metrics = metrics.clone();
    let docker_handle = task::spawn(async move {
        let mut docker_processor = match crate::docker::DockerBufferProcessor::new(
            docker_state,
            use_tee,
            container_timeout,
        ) {
            Ok(processor) => processor,
            Err(e) => {
                error!("Failed to create docker buffer processor: {}", e);
                return;
            }
        };
        docker_processor.set_metrics(docker_metrics);

        // Delay docker processor startup by 8 seconds
        info!("Docker buffer processor waiting 8 seconds before starting...");
        tokio::time::sleep(Duration::from_secs(8)).await;
        info!("Docker buffer processor starting now");

        if let Err(e) = docker_processor.run().await {
            error!("Docker buffer processor error: {}", e);
        }
    });
    info!("🐳 Started docker buffer processor thread");

    // 7. Start event processor in main thread (processes events and updates shared state)
    let mut processor = EventProcessor::new(config, state.clone()).await?;
    processor.set_metrics(metrics.clone());
    info!("👁️ Starting event monitoring...");

    // Run processor in a task so we can monitor shutdown
    let processor_handle = task::spawn(async move { processor.run().await });

    // Send coordinator started event now that everything is initialized
    {
        // Wait for RPC client to be initialized (up to 10 seconds)
        let mut rpc_client_initialized = false;
        for _ in 0..20 {
            if state.get_rpc_client().await.is_some() {
                rpc_client_initialized = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        if rpc_client_initialized {
            let rpc_client = state.get_rpc_client().await;
            if let Some(mut client) = rpc_client {
                let event = proto::Event {
                    event: Some(proto::event::Event::CoordinatorStarted(
                        proto::CoordinatorStartedEvent {
                            coordinator_id: coordinator_id.clone(),
                            ethereum_address: ethereum_address.clone(),
                            event_timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        },
                    )),
                };

                let request = proto::SubmitEventRequest { event: Some(event) };

                match client.submit_event(request).await {
                    Ok(response) => {
                        let resp = response.into_inner();
                        if resp.success {
                            info!("✅ Sent coordinator started event");
                        } else {
                            warn!(
                                "⚠️ Failed to send coordinator started event: {}",
                                resp.message
                            );
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to send coordinator started event: {}", e);
                    }
                }
            }
        } else {
            warn!("⚠️ RPC client initialization timed out, skipping coordinator started event");
        }
    }

    // Monitor for shutdown or processor exit
    loop {
        if state.is_shutting_down() {
            info!("🛑 Shutdown requested, starting graceful shutdown sequence...");
            break;
        }

        // Check if processor exited
        if processor_handle.is_finished() {
            match processor_handle.await {
                Ok(processor_result) => {
                    if let Err(e) = processor_result {
                        error!("Fatal error in event processor: {}", e);
                        return Err(e.into());
                    }
                }
                Err(e) => {
                    error!("Processor task panicked: {}", e);
                    return Err(anyhow::anyhow!("Processor task panicked: {}", e).into());
                }
            }
            break;
        }

        tokio::time::sleep(Duration::from_millis(SHUTDOWN_CLEANUP_DELAY_MS)).await;
    }

    info!("🔄 Starting graceful shutdown sequence...");
    info!("    Shutdown order:");
    info!("    1️⃣ Stop searching for new jobs");
    info!("    2️⃣ Process all buffered jobs and wait for running containers");
    info!("    3️⃣ Send all pending multicall operations to blockchain");
    info!(
        "    ⏱️  Maximum wait time: {} minutes",
        GRACEFUL_SHUTDOWN_TIMEOUT_SECS / 60
    );
    let mut wait_time = 0;
    let max_wait = GRACEFUL_SHUTDOWN_TIMEOUT_SECS;

    while wait_time < max_wait {
        // Check for force shutdown
        if state.is_force_shutdown() {
            error!("  🛑 Force shutdown requested!");
            break;
        }

        let current_agents = state.get_current_agent_count().await;
        if current_agents == 0 {
            info!("  ✅ All jobs completed");
            break;
        }

        // Show progress periodically
        if wait_time % SHUTDOWN_PROGRESS_INTERVAL_SECS == 0 {
            if wait_time < 60 {
                info!(
                    "  ⏳ {} agents still running, waiting... ({}/{}s)",
                    current_agents, wait_time, max_wait
                );
            } else {
                let minutes = wait_time / 60;
                let seconds = wait_time % 60;
                let max_minutes = GRACEFUL_SHUTDOWN_TIMEOUT_SECS / 60;
                info!(
                    "  ⏳ {} agents still running, waiting... ({}m {}s / {}m)",
                    current_agents, minutes, seconds, max_minutes
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
        wait_time += 1;
    }

    // Phase 3: If jobs are still running, try to get their status and logs
    let remaining_agents = state.get_current_agent_count().await;
    if remaining_agents > 0 {
        if state.is_force_shutdown() {
            error!(
                "  🛑 Force shutdown - terminating {} running agents...",
                remaining_agents
            );
            // Clear phantom agents on force shutdown
            state.clear_all_current_agents().await;
        } else {
            warn!(
                "  ⚠️ {} agents still running after timeout, collecting logs...",
                remaining_agents
            );
        }

        // Get information about running agents
        let running_agents = state.get_all_current_agents().await;

        // Try to get Docker container logs and terminate if force shutdown
        let docker_manager = match docker::DockerManager::new(false) {
            Ok(dm) => Some(dm),
            Err(e) => {
                error!("Failed to create Docker manager for shutdown: {}", e);
                None
            }
        };

        for (session_id, agent_info) in running_agents {
            warn!(
                "    - Session {}: {}/{}/{}",
                session_id, agent_info.developer, agent_info.agent, agent_info.agent_method
            );
        }

        // Try to get container logs and optionally stop them
        if let Some(ref dm) = docker_manager {
            info!("  📋 Fetching logs from running Docker containers...");
            let container_results = dm
                .fetch_and_stop_silvana_containers(state.is_force_shutdown())
                .await;

            for (container_id, container_name, logs) in container_results {
                info!(
                    "  📦 Container: {} ({})",
                    &container_id[..12.min(container_id.len())],
                    container_name
                );
                if !logs.is_empty() {
                    // Print full container logs during shutdown
                    info!("  📄 Container logs:\n{}", logs);
                } else {
                    info!("  ⚠️ No logs available from container");
                }
            }
        }
    }

    // Phase 4: Cancel non-critical background tasks (but keep gRPC running for containers)
    info!("  3️⃣ Stopping non-critical background tasks...");
    // Don't abort gRPC yet - containers need it to communicate
    reconciliation_handle.abort();
    block_creation_handle.abort();
    proof_analysis_handle.abort();
    balance_check_handle.abort();
    active_handle.abort();

    // Shutdown sequence:
    // 1. Job searcher stops immediately (no new jobs)
    // 2. Docker processor processes all buffered jobs and waits for containers
    // 3. Multicall processor continues until docker is done, then processes final operations
    // 4. gRPC server stays running until Docker containers complete

    // Job searcher should stop immediately
    if !job_searcher_handle.is_finished() {
        info!("  ⏳ Waiting for job searcher to stop...");
        let _ = tokio::time::timeout(
            Duration::from_secs(JOB_SEARCHER_SHUTDOWN_TIMEOUT_SECS),
            job_searcher_handle,
        )
        .await;
        info!("  ✅ Job searcher stopped (no new jobs will be searched)");
    }

    // Docker and Multicall processors work together
    // Docker processes buffered jobs and waits for containers
    // Multicall continues processing requests from containers
    let mut docker_done = false;
    let mut multicall_done = false;
    let shutdown_start = tokio::time::Instant::now();

    while (!docker_done || !multicall_done)
        && shutdown_start.elapsed().as_secs() < GRACEFUL_SHUTDOWN_TIMEOUT_SECS
        && !state.is_force_shutdown()
    {
        // Check for force shutdown inside the loop
        if state.is_force_shutdown() {
            error!("  🛑 Force shutdown detected - exiting shutdown progress loop");
            // Clear any phantom agents that might be stuck
            state.clear_all_current_agents().await;
            break;
        }

        // Check docker status
        if !docker_done && docker_handle.is_finished() {
            docker_done = true;
            info!("  ✅ Docker processor completed all jobs and containers");
        }

        // Check multicall status
        if !multicall_done && multicall_handle.is_finished() {
            multicall_done = true;
            info!("  ✅ Multicall processor completed all operations");
        }

        // Show progress
        if !docker_done || !multicall_done {
            let buffer_size = state.get_started_jobs_buffer_size().await;
            let current_agents = state.get_current_agent_count().await;
            let pending_ops = state.get_total_operations_count().await;

            if shutdown_start.elapsed().as_secs() % 5 == 0 {
                info!(
                    "  ⏳ Shutdown progress: {} buffered jobs, {} containers, {} pending operations",
                    buffer_size, current_agents, pending_ops
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    // Force abort if timeout
    if !docker_done {
        warn!("  ⚠️ Docker processor timeout - forcing shutdown");
        docker_handle.abort();
    }
    if !multicall_done {
        warn!("  ⚠️ Multicall processor timeout - forcing shutdown");
        multicall_handle.abort();
    }

    // Now that Docker containers are done, we can safely stop the gRPC server
    info!("  ⏳ Stopping gRPC server...");
    grpc_handle.abort();
    info!("  ✅ gRPC server stopped");

    // Send coordinator shutdown event
    {
        let rpc_client = state.get_rpc_client().await;
        if let Some(mut client) = rpc_client {
            let event = proto::Event {
                event: Some(proto::event::Event::CoordinatorShutdown(
                    proto::CoordinatorShutdownEvent {
                        coordinator_id: coordinator_id.clone(),
                        event_timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    },
                )),
            };

            let request = proto::SubmitEventRequest { event: Some(event) };

            match client.submit_event(request).await {
                Ok(response) => {
                    let resp = response.into_inner();
                    if resp.success {
                        info!("✅ Sent coordinator shutdown event");
                    } else {
                        warn!(
                            "⚠️ Failed to send coordinator shutdown event: {}",
                            resp.message
                        );
                    }
                }
                Err(e) => {
                    warn!("⚠️ Failed to send coordinator shutdown event: {}", e);
                }
            }
        }
    }

    info!("✅ Graceful shutdown complete");
    println!("✅ Graceful shutdown complete");
    Ok(())
}
