//! Multi-coordination layer manager using wrapper trait

use crate::coordination_wrapper::{CoordinationWrapper, EthereumCoordinationWrapper, PrivateCoordinationWrapper, SuiCoordinationWrapper};
use crate::error::Result;
use crate::layer_config::{CoordinatorConfig, LayerInfo, LayerType, OperationMode};
use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Manages multiple coordination layers using the wrapper trait
pub struct CoordinationManager {
    /// Map from layer_id to coordination implementation
    layers: HashMap<String, Arc<Box<dyn CoordinationWrapper>>>,

    /// Map from app_instance to layer_id for routing cache
    app_instance_routing: Arc<RwLock<HashMap<String, String>>>,

    /// Layer metadata
    layer_info: HashMap<String, LayerInfo>,

    /// Configuration
    config: CoordinatorConfig,

    /// Registry client (from Sui layer)
    sui_layer_id: String,
}

impl CoordinationManager {
    /// Create a new CoordinationManager from configuration
    pub async fn from_config(config: CoordinatorConfig) -> Result<Self> {
        // Validate configuration
        config.validate()?;

        let mut layers: HashMap<String, Arc<Box<dyn CoordinationWrapper>>> = HashMap::new();
        let mut layer_info = HashMap::new();

        info!("Initializing coordination layers...");

        // Initialize Sui coordination (always present for registry)
        if config.is_layer_enabled(&config.sui.layer_id) {
            info!("Initializing Sui layer: {}", config.sui.layer_id);

            // Initialize SharedSuiState if not already initialized
            if !sui::state::SharedSuiState::is_initialized() {
                sui::state::SharedSuiState::initialize(&config.sui.rpc_url).await?;
            }

            let sui = SuiCoordinationWrapper::new(config.sui.layer_id.clone());

            layers.insert(
                config.sui.layer_id.clone(),
                Arc::new(Box::new(sui) as Box<dyn CoordinationWrapper>),
            );

            layer_info.insert(
                config.sui.layer_id.clone(),
                LayerInfo {
                    layer_id: config.sui.layer_id.clone(),
                    layer_type: LayerType::Sui,
                    operation_mode: OperationMode::from(config.sui.operation_mode.as_str()),
                    app_instance_filter: config.sui.app_instance_filter.clone(),
                },
            );

            info!("✅ Sui layer initialized: {}", config.sui.layer_id);
        }

        // Initialize all Private layers
        for private_config in &config.private {
            if config.is_layer_enabled(&private_config.layer_id) {
                info!("Initializing Private layer: {}", private_config.layer_id);

                let private = PrivateCoordinationWrapper::new(
                    private_config.layer_id.clone(),
                    silvana_coordination_private::PrivateCoordinationConfig {
                        database_url: private_config.database_url.clone(),
                        jwt_secret: private_config.jwt_secret.clone(),
                        chain_id: private_config.layer_id.clone(),
                        max_connections: private_config.max_connections,
                        connection_timeout_secs: private_config.connection_timeout,
                        enable_sql_logging: false,
                    },
                ).await?;

                layers.insert(
                    private_config.layer_id.clone(),
                    Arc::new(Box::new(private) as Box<dyn CoordinationWrapper>),
                );

                layer_info.insert(
                    private_config.layer_id.clone(),
                    LayerInfo {
                        layer_id: private_config.layer_id.clone(),
                        layer_type: LayerType::Private,
                        operation_mode: OperationMode::from(private_config.operation_mode.as_str()),
                        app_instance_filter: private_config.app_instance_filter.clone(),
                    },
                );

                info!("✅ Private layer initialized: {}", private_config.layer_id);
            }
        }

        // Initialize all Ethereum layers
        for ethereum_config in &config.ethereum {
            if config.is_layer_enabled(&ethereum_config.layer_id) {
                info!("Initializing Ethereum layer: {}", ethereum_config.layer_id);

                // Resolve private key from environment variable
                let private_key = std::env::var(&ethereum_config.private_key_env)
                    .ok(); // Optional - for read-only operations

                let ethereum = EthereumCoordinationWrapper::new(
                    ethereum_config.layer_id.clone(),
                    silvana_coordination_ethereum::EthereumCoordinationConfig {
                        rpc_url: ethereum_config.rpc_url.clone(),
                        chain_id: ethereum_config.chain_id.unwrap_or(31337),
                        contract_address: ethereum_config.contract_address.clone(),
                        private_key,
                        multicall_enabled: false, // Not supported yet
                        multicall_interval_secs: ethereum_config.multicall_interval_secs,
                        gas_limit_multiplier: ethereum_config.gas_price_multiplier,
                        max_gas_price_gwei: Some(ethereum_config.max_gas as f64 / 1_000_000_000.0),
                        confirmation_blocks: 1,
                    },
                )?;

                layers.insert(
                    ethereum_config.layer_id.clone(),
                    Arc::new(Box::new(ethereum) as Box<dyn CoordinationWrapper>),
                );

                layer_info.insert(
                    ethereum_config.layer_id.clone(),
                    LayerInfo {
                        layer_id: ethereum_config.layer_id.clone(),
                        layer_type: LayerType::Ethereum,
                        operation_mode: OperationMode::from(ethereum_config.operation_mode.as_str()),
                        app_instance_filter: ethereum_config.app_instance_filter.clone(),
                    },
                );

                info!("✅ Ethereum layer initialized: {}", ethereum_config.layer_id);
            }
        }

        info!(
            "Coordination manager initialized with {} layers",
            layers.len()
        );

        Ok(Self {
            layers,
            app_instance_routing: Arc::new(RwLock::new(HashMap::new())),
            layer_info,
            sui_layer_id: config.sui.layer_id.clone(),
            config,
        })
    }

    /// Get the Sui coordination layer (for registry operations)
    pub fn get_sui_layer(&self) -> Result<Arc<Box<dyn CoordinationWrapper>>> {
        self.layers
            .get(&self.sui_layer_id)
            .cloned()
            .ok_or_else(|| anyhow!("Sui layer not found").into())
    }

    /// Get a specific coordination layer by ID
    pub fn get_layer(&self, layer_id: &str) -> Option<Arc<Box<dyn CoordinationWrapper>>> {
        self.layers.get(layer_id).cloned()
    }

    /// Get all active layer IDs
    pub fn get_layer_ids(&self) -> Vec<String> {
        self.layers.keys().cloned().collect()
    }

    /// Get coordination layer for a specific app_instance
    pub async fn get_layer_for_app(&self, app_instance: &str) -> Result<(String, Arc<Box<dyn CoordinationWrapper>>)> {
        // Check routing cache
        {
            let cache = self.app_instance_routing.read().await;
            if let Some(layer_id) = cache.get(app_instance) {
                if let Some(layer) = self.layers.get(layer_id) {
                    debug!("Found app_instance {} in cached layer {}", app_instance, layer_id);
                    return Ok((layer_id.clone(), layer.clone()));
                }
            }
        }

        // Query each layer to find which owns this app_instance
        for (layer_id, coordination) in &self.layers {
            // Check if this layer has an app_instance filter
            if let Some(info) = self.layer_info.get(layer_id) {
                if let Some(ref filter) = info.app_instance_filter {
                    // If filter exists and doesn't match, skip this layer
                    if !app_instance.contains(filter) {
                        continue;
                    }
                }
            }

            match coordination.fetch_app_instance(app_instance).await {
                Ok(_) => {
                    debug!("Found app_instance {} in layer {}", app_instance, layer_id);

                    // Cache the routing
                    {
                        let mut cache = self.app_instance_routing.write().await;
                        cache.insert(app_instance.to_string(), layer_id.clone());
                    }

                    return Ok((layer_id.clone(), coordination.clone()));
                }
                Err(e) => {
                    debug!(
                        "App_instance {} not found in layer {}: {}",
                        app_instance, layer_id, e
                    );
                    continue;
                }
            }
        }

        Err(anyhow!(
            "App instance {} not found in any coordination layer",
            app_instance
        ).into())
    }

    /// Clear routing cache for an app_instance
    pub async fn clear_routing_cache(&self, app_instance: &str) {
        let mut cache = self.app_instance_routing.write().await;
        cache.remove(app_instance);
    }

    /// Clear entire routing cache
    pub async fn clear_all_routing_cache(&self) {
        let mut cache = self.app_instance_routing.write().await;
        cache.clear();
    }

    /// Get layer information
    pub fn get_layer_info(&self, layer_id: &str) -> Option<&LayerInfo> {
        self.layer_info.get(layer_id)
    }

    /// Check if a layer supports multicall operations
    pub fn supports_multicall(&self, layer_id: &str) -> bool {
        self.layer_info
            .get(layer_id)
            .map(|info| info.operation_mode == OperationMode::Multicall)
            .unwrap_or(false)
    }

    /// Get all layers that support multicall
    pub fn get_multicall_layers(&self) -> Vec<String> {
        self.layer_info
            .iter()
            .filter(|(_, info)| info.operation_mode == OperationMode::Multicall)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all layers with Direct operation mode
    pub fn get_direct_layers(&self) -> Vec<String> {
        self.layer_info
            .iter()
            .filter(|(_, info)| info.operation_mode == OperationMode::Direct)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get metrics for all layers
    pub async fn get_metrics(&self) -> HashMap<String, LayerMetrics> {
        let mut metrics = HashMap::new();

        for (layer_id, _) in &self.layers {
            let layer_type = self.layer_info
                .get(layer_id)
                .map(|info| info.layer_type)
                .unwrap_or(LayerType::Sui);

            let operation_mode = self.layer_info
                .get(layer_id)
                .map(|info| info.operation_mode)
                .unwrap_or(OperationMode::Direct);

            metrics.insert(
                layer_id.clone(),
                LayerMetrics {
                    layer_id: layer_id.clone(),
                    layer_type,
                    operation_mode,
                    pending_jobs: 0,
                    running_jobs: 0,
                    completed_jobs: 0,
                    failed_jobs: 0,
                },
            );
        }

        metrics
    }

    /// Get the configuration
    pub fn config(&self) -> &CoordinatorConfig {
        &self.config
    }
}

/// Metrics for a coordination layer
#[derive(Debug, Clone)]
pub struct LayerMetrics {
    pub layer_id: String,
    pub layer_type: LayerType,
    pub operation_mode: OperationMode,
    pub pending_jobs: usize,
    pub running_jobs: usize,
    pub completed_jobs: usize,
    pub failed_jobs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_coordination_manager_creation() {
        let config = CoordinatorConfig::from_env().unwrap();
        // Note: This test may fail without proper environment setup
        // let manager = CoordinationManager::from_config(config).await.unwrap();
        // assert!(!manager.get_layer_ids().is_empty());
    }
}