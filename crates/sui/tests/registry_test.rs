#[cfg(test)]
mod tests {
    use anyhow::Result;
    use rand::Rng;
    use std::env;
    use sui::SilvanaSuiInterface;
    use tracing::info;

    /// Generate a random string with given prefix for testing
    fn generate_test_name(prefix: &str) -> String {
        let random_suffix: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();
        format!("{}_{}", prefix, random_suffix)
    }

    /// Initialize test environment
    async fn init_test() -> Result<()> {
        // Load environment variables from .env file if it exists
        let _ = dotenvy::dotenv();
        
        // Initialize tracing for better debug output
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        // Ensure required environment variables are set
        if env::var("SUI_ADDRESS").is_err() {
            return Err(anyhow::anyhow!("SUI_ADDRESS environment variable not set. Please set it or create a .env file"));
        }
        // Check for SUI_SECRET_KEY
        if env::var("SUI_SECRET_KEY").is_err() {
            return Err(anyhow::anyhow!("SUI_SECRET_KEY environment variable not set. Please set it or create a .env file"));
        }

        // Resolve RPC URL using the chain helper
        let rpc_url = sui::chain::resolve_rpc_url(None, None)?;
        
        // Initialize the global SharedSuiState (required for all registry operations)
        sui::SharedSuiState::initialize(&rpc_url).await?;

        // Log the configuration
        tracing::info!("Test environment initialized");
        tracing::info!("SUI RPC URL: {}", rpc_url);
        tracing::info!("SUI_ADDRESS: {}", env::var("SUI_ADDRESS")?);
        tracing::info!("SUI_CHAIN: {}", env::var("SUI_CHAIN").unwrap_or_else(|_| "testnet".to_string()));
        if let Ok(package) = env::var("SILVANA_REGISTRY_PACKAGE") {
            tracing::info!("SILVANA_REGISTRY_PACKAGE: {}", package);
        }

        Ok(())
    }

    /// Wait for a transaction to be confirmed on chain
    async fn wait_tx(tx_digest: &str) -> Result<()> {
        info!("Waiting for transaction {} to be confirmed...", tx_digest);
        // In a real implementation, you would poll the transaction status
        // For now, just wait a bit
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        Ok(())
    }

    #[tokio::test]
    async fn test_registry_operations() -> Result<()> {
        init_test().await?;

        // Create interface instance
        let mut interface = SilvanaSuiInterface::new();

        // Step 1: Create a new registry for testing
        info!("=== Step 1: Creating test registry ===");
        let registry_name = generate_test_name("test_registry");
        info!("Creating registry with name: {}", registry_name);

        let create_result = interface.create_silvana_registry(registry_name.clone(), None).await
            .map_err(|e| anyhow::anyhow!(e))?;
        info!("Registry created successfully!");
        info!("  Registry ID: {}", create_result.registry_id);
        info!("  Transaction: {}", create_result.tx_digest);
        
        wait_tx(&create_result.tx_digest).await?;

        // Use the actual registry object ID from the creation result
        let registry_id = create_result.registry_id.clone();

        // Step 2: Test Developer Operations
        info!("\n=== Step 2: Testing Developer Operations ===");
        
        // Add developer
        let developer_name = generate_test_name("developer");
        let github_username = generate_test_name("github");
        info!("Adding developer: {}", developer_name);
        
        let add_dev_tx = interface.add_developer_to_registry(
            &registry_id,
            developer_name.clone(),
            github_username.clone(),
            Some("https://example.com/image.png".to_string()),
            Some("Test developer description".to_string()),
            Some("https://example.com".to_string()),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Developer added, tx: {}", add_dev_tx);
        wait_tx(&add_dev_tx).await?;

        // Update developer
        info!("Updating developer: {}", developer_name);
        let update_dev_tx = interface.update_developer_in_registry(
            &registry_id,
            developer_name.clone(),
            github_username.clone(),
            Some("https://example.com/new-image.png".to_string()),
            Some("Updated developer description".to_string()),
            Some("https://newsite.com".to_string()),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Developer updated, tx: {}", update_dev_tx);
        wait_tx(&update_dev_tx).await?;

        // Step 3: Test Agent Operations
        info!("\n=== Step 3: Testing Agent Operations ===");
        
        // Add agent
        let agent_name = generate_test_name("agent");
        info!("Adding agent: {} to developer: {}", agent_name, developer_name);
        
        let add_agent_tx = interface.add_agent_to_developer(
            &registry_id,
            developer_name.clone(),
            agent_name.clone(),
            Some("https://example.com/agent-image.png".to_string()),
            Some("Test agent description".to_string()),
            Some("https://agent.example.com".to_string()),
            vec!["ethereum".to_string(), "polygon".to_string()],
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Agent added, tx: {}", add_agent_tx);
        wait_tx(&add_agent_tx).await?;

        // Update agent
        info!("Updating agent: {}", agent_name);
        let update_agent_tx = interface.update_agent_in_registry(
            &registry_id,
            developer_name.clone(),
            agent_name.clone(),
            Some("https://example.com/agent-new-image.png".to_string()),
            Some("Updated agent description".to_string()),
            Some("https://agent-new.example.com".to_string()),
            vec!["ethereum".to_string(), "polygon".to_string(), "arbitrum".to_string()],
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Agent updated, tx: {}", update_agent_tx);
        wait_tx(&update_agent_tx).await?;

        // Step 4: Test App Operations
        info!("\n=== Step 4: Testing App Operations ===");
        
        // Add app
        let app_name = generate_test_name("app");
        info!("Adding app: {}", app_name);
        
        let add_app_tx = interface.add_app_to_registry(
            &registry_id,
            app_name.clone(),
            Some("Test application description".to_string()),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("App added, tx: {}", add_app_tx);
        wait_tx(&add_app_tx).await?;

        // Update app
        info!("Updating app: {}", app_name);
        let update_app_tx = interface.update_app_in_registry(
            &registry_id,
            app_name.clone(),
            Some("Updated application description".to_string()),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("App updated, tx: {}", update_app_tx);
        wait_tx(&update_app_tx).await?;

        // Step 5: Test Removal Operations
        info!("\n=== Step 5: Testing Removal Operations ===");
        
        // Remove agent
        info!("Removing agent: {}", agent_name);
        let remove_agent_tx = interface.remove_agent_from_developer(
            &registry_id,
            developer_name.clone(),
            agent_name.clone(),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Agent removed, tx: {}", remove_agent_tx);
        wait_tx(&remove_agent_tx).await?;

        // Remove app
        info!("Removing app: {}", app_name);
        let remove_app_tx = interface.remove_app_from_registry(
            &registry_id,
            app_name.clone(),
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("App removed, tx: {}", remove_app_tx);
        wait_tx(&remove_app_tx).await?;

        // Remove developer (must provide agent names that need to be removed)
        info!("Removing developer: {}", developer_name);
        let remove_dev_tx = interface.remove_developer_from_registry(
            &registry_id,
            developer_name.clone(),
            vec![], // No agents left to remove
        ).await.map_err(|e| anyhow::anyhow!(e))?;
        info!("Developer removed, tx: {}", remove_dev_tx);
        wait_tx(&remove_dev_tx).await?;

        info!("\n=== All tests completed successfully! ===");
        Ok(())
    }

    #[tokio::test]
    async fn test_registry_with_multiple_entities() -> Result<()> {
        init_test().await?;

        // Create interface instance
        let mut interface = SilvanaSuiInterface::new();

        info!("=== Testing Registry with Multiple Entities ===");
        
        // Create registry
        let registry_name = generate_test_name("multi_test_registry");
        info!("Creating registry: {}", registry_name);
        let create_result = interface.create_silvana_registry(registry_name.clone(), None).await
            .map_err(|e| anyhow::anyhow!(e))?;
        let registry_id = create_result.registry_id.clone();
        wait_tx(&create_result.tx_digest).await?;

        // Add multiple developers
        let mut developers = Vec::new();
        for i in 0..3 {
            let dev_name = generate_test_name(&format!("developer_{}", i));
            let github = generate_test_name(&format!("github_{}", i));
            info!("Adding developer {}: {}", i, dev_name);
            
            let tx = interface.add_developer_to_registry(
                &registry_id,
                dev_name.clone(),
                github.clone(),
                None,
                Some(format!("Developer {} description", i)),
                None,
            ).await.map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
            developers.push((dev_name, github));
        }

        // Add multiple agents to first developer
        let (first_dev_name, _) = &developers[0];
        let mut agents = Vec::new();
        for i in 0..3 {
            let agent_name = generate_test_name(&format!("agent_{}", i));
            info!("Adding agent {} to developer {}", agent_name, first_dev_name);
            
            let tx = interface.add_agent_to_developer(
                &registry_id,
                first_dev_name.clone(),
                agent_name.clone(),
                None,
                Some(format!("Agent {} description", i)),
                None,
                vec!["ethereum".to_string()],
            ).await.map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
            agents.push(agent_name);
        }

        // Add multiple apps
        let mut apps = Vec::new();
        for i in 0..3 {
            let app_name = generate_test_name(&format!("app_{}", i));
            info!("Adding app {}: {}", i, app_name);
            
            let tx = interface.add_app_to_registry(
                &registry_id,
                app_name.clone(),
                Some(format!("App {} description", i)),
            ).await.map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
            apps.push(app_name);
        }

        info!("\n=== Summary ===");
        info!("Created registry: {}", registry_name);
        info!("Added {} developers", developers.len());
        info!("Added {} agents to first developer", agents.len());
        info!("Added {} apps", apps.len());

        // Clean up - remove everything
        info!("\n=== Cleaning up ===");
        
        // Remove all agents from first developer
        for agent_name in agents {
            info!("Removing agent: {}", agent_name);
            let tx = interface.remove_agent_from_developer(&registry_id, first_dev_name.clone(), agent_name).await
                .map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
        }

        // Remove all apps
        for app_name in apps {
            info!("Removing app: {}", app_name);
            let tx = interface.remove_app_from_registry(&registry_id, app_name).await
                .map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
        }

        // Remove all developers
        for (dev_name, _) in developers {
            info!("Removing developer: {}", dev_name);
            let tx = interface.remove_developer_from_registry(&registry_id, dev_name, vec![]).await
                .map_err(|e| anyhow::anyhow!(e))?;
            wait_tx(&tx).await?;
        }

        info!("\n=== Multiple entities test completed successfully! ===");
        Ok(())
    }

    #[tokio::test]
    async fn test_registry_with_env_package_id() -> Result<()> {
        init_test().await?;

        // Create interface instance
        let mut interface = SilvanaSuiInterface::new();

        // Test using SILVANA_REGISTRY_PACKAGE environment variable
        if let Ok(package_id) = env::var("SILVANA_REGISTRY_PACKAGE") {
            info!("=== Testing with SILVANA_REGISTRY_PACKAGE={} ===", package_id);
            
            let registry_name = generate_test_name("env_test_registry");
            info!("Creating registry with env package ID: {}", registry_name);
            
            // Should use the env var automatically
            let create_result = interface.create_silvana_registry(registry_name.clone(), None).await
                .map_err(|e| anyhow::anyhow!(e))?;
            info!("Registry created with env package ID!");
            info!("  Registry ID: {}", create_result.registry_id);
            info!("  Transaction: {}", create_result.tx_digest);
            
            wait_tx(&create_result.tx_digest).await?;
            
            // Test basic operation
            let registry_id = create_result.registry_id.clone();
            let dev_name = generate_test_name("env_test_developer");
            let github = generate_test_name("env_test_github");
            
            info!("Testing developer operation with env package registry");
            let tx = interface.add_developer_to_registry(
                &registry_id,
                dev_name.clone(),
                github,
                None,
                Some("Test with env package".to_string()),
                None,
            ).await.map_err(|e| anyhow::anyhow!(e))?;
            info!("Developer added to env package registry, tx: {}", tx);
            wait_tx(&tx).await?;
            
            info!("Env package test completed successfully!");
        } else {
            info!("SILVANA_REGISTRY_PACKAGE not set, skipping env package test");
        }
        
        Ok(())
    }
}