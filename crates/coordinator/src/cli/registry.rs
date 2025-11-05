use crate::cli::RegistryCommands;
use crate::error::{CoordinatorError, Result};
use anyhow::anyhow;
use tracing_subscriber::prelude::*;

pub async fn handle_registry_command(
    rpc_url: Option<String>,
    subcommand: RegistryCommands,
    chain_override: Option<String>,
) -> Result<()> {
    // Initialize minimal logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve and initialize Sui connection
    let rpc_url = sui::resolve_rpc_url(rpc_url, chain_override)
        .map_err(CoordinatorError::Other)?;
    sui::SharedSuiState::initialize(&rpc_url)
        .await
        .map_err(CoordinatorError::Other)?;

    // Get the current Sui address to use as owner
    let owner_address = sui::SharedSuiState::get_instance().get_sui_address_required();

    // Create interface
    let mut interface = sui::SilvanaSuiInterface::new();

    match subcommand {
        RegistryCommands::Create { name, package_id } => {
            println!("📝 Creating new Silvana registry...\n");
            println!("   Name: {}", name);
            if let Some(ref pkg) = package_id {
                println!("   Package: {}", pkg);
            }
            println!();

            match interface
                .create_silvana_registry(name.clone(), package_id)
                .await
            {
                Ok(result) => {
                    println!("✅ Registry created successfully!\n");
                    println!("   Registry ID: {}", result.registry_id);
                    println!("   Transaction: {}", result.tx_digest);
                    println!();
                    println!("💡 To use this registry, set the environment variable:");
                    println!("   export SILVANA_REGISTRY={}", result.registry_id);
                }
                Err(e) => {
                    eprintln!("❌ Failed to create registry: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::AddDeveloper {
            registry,
            name,
            github,
            image,
            description,
            site,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("👤 Adding developer to registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            if let Some(ref gh) = github {
                println!("   GitHub: {}", gh);
            }
            if let Some(ref img) = image {
                println!("   Image: {}", img);
            }
            if let Some(ref desc) = description {
                println!("   Description: {}", desc);
            }
            if let Some(ref s) = site {
                println!("   Site: {}", s);
            }
            println!();

            match interface
                .add_developer_to_registry(
                    &registry_id,
                    owner_address,
                    name,
                    github,
                    image,
                    description,
                    site,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Developer added successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to add developer: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::UpdateDeveloper {
            registry,
            name,
            github,
            image,
            description,
            site,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Updating developer in registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            if let Some(ref gh) = github {
                println!("   GitHub: {}", gh);
            }
            println!();

            match interface
                .update_developer_in_registry(
                    &registry_id,
                    name,
                    github,
                    image,
                    description,
                    site,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Developer updated successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to update developer: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::RemoveDeveloper {
            registry,
            name,
            agents,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("🗑️  Removing developer from registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            if !agents.is_empty() {
                println!("   Agents to remove: {}", agents.join(", "));
            }
            println!();

            match interface
                .remove_developer_from_registry(&registry_id, name, agents)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Developer removed successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to remove developer: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::AddAgent {
            registry,
            developer,
            name,
            image,
            description,
            site,
            chains,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("🤖 Adding agent to developer...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", name);
            if !chains.is_empty() {
                println!("   Chains: {}", chains.join(", "));
            }
            if let Some(ref img) = image {
                println!("   Image: {}", img);
            }
            if let Some(ref desc) = description {
                println!("   Description: {}", desc);
            }
            if let Some(ref s) = site {
                println!("   Site: {}", s);
            }
            println!();

            match interface
                .add_agent_to_developer(
                    &registry_id,
                    developer,
                    name,
                    image,
                    description,
                    site,
                    chains,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Agent added successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to add agent: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::UpdateAgent {
            registry,
            developer,
            name,
            image,
            description,
            site,
            chains,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Updating agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", name);
            if !chains.is_empty() {
                println!("   Chains: {}", chains.join(", "));
            }
            println!();

            match interface
                .update_agent_in_registry(
                    &registry_id,
                    developer,
                    name,
                    image,
                    description,
                    site,
                    chains,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Agent updated successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to update agent: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::RemoveAgent {
            registry,
            developer,
            name,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("🗑️  Removing agent from developer...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", name);
            println!();

            match interface
                .remove_agent_from_developer(&registry_id, developer, name)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Agent removed successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to remove agent: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::AddApp {
            registry,
            name,
            description,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📱 Adding app to registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            if let Some(ref desc) = description {
                println!("   Description: {}", desc);
            }
            println!();

            match interface
                .add_app_to_registry(&registry_id, name, owner_address, description)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ App added successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to add app: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::UpdateApp {
            registry,
            name,
            description,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Updating app in registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            if let Some(ref desc) = description {
                println!("   Description: {}", desc);
            }
            println!();

            match interface
                .update_app_in_registry(&registry_id, name, description)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ App updated successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to update app: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::RemoveApp { registry, name } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("🗑️  Removing app from registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            println!();

            match interface.remove_app_from_registry(&registry_id, name).await {
                Ok(tx_digest) => {
                    println!("✅ App removed successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to remove app: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::AddMethod {
            registry,
            developer,
            agent,
            method,
            docker_image,
            docker_sha256,
            min_memory_gb,
            min_cpu_cores,
            requires_tee,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Adding method to agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", agent);
            println!("   Method: {}", method);
            println!("   Docker Image: {}", docker_image);
            if let Some(ref sha) = docker_sha256 {
                println!("   Docker SHA256: {}", sha);
            }
            println!("   Min Memory: {} GB", min_memory_gb);
            println!("   Min CPU Cores: {}", min_cpu_cores);
            println!("   Requires TEE: {}", requires_tee);
            println!();

            match interface
                .add_method_to_agent(
                    &registry_id,
                    developer,
                    agent,
                    method,
                    docker_image,
                    docker_sha256,
                    min_memory_gb,
                    min_cpu_cores,
                    requires_tee,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Method added successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to add method: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::UpdateMethod {
            registry,
            developer,
            agent,
            method,
            docker_image,
            docker_sha256,
            min_memory_gb,
            min_cpu_cores,
            requires_tee,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Updating method on agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", agent);
            println!("   Method: {}", method);
            println!("   Docker Image: {}", docker_image);
            if let Some(ref sha) = docker_sha256 {
                println!("   Docker SHA256: {}", sha);
            }
            println!("   Min Memory: {} GB", min_memory_gb);
            println!("   Min CPU Cores: {}", min_cpu_cores);
            println!("   Requires TEE: {}", requires_tee);
            println!();

            match interface
                .update_method_on_agent(
                    &registry_id,
                    developer,
                    agent,
                    method,
                    docker_image,
                    docker_sha256,
                    min_memory_gb,
                    min_cpu_cores,
                    requires_tee,
                )
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Method updated successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to update method: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::RemoveMethod {
            registry,
            developer,
            agent,
            method,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("🗑️  Removing method from agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", agent);
            println!("   Method: {}", method);
            println!();

            match interface
                .remove_method_from_agent(&registry_id, developer, agent, method)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Method removed successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to remove method: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::SetDefaultMethod {
            registry,
            developer,
            agent,
            method,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("⭐ Setting default method for agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", agent);
            println!("   Method: {}", method);
            println!();

            match interface
                .set_default_method_on_agent(&registry_id, developer, agent, method)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Default method set successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to set default method: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::RemoveDefaultMethod {
            registry,
            developer,
            agent,
        } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("⭐ Removing default method from agent...\n");
            println!("   Registry: {}", registry_id);
            println!("   Developer: {}", developer);
            println!("   Agent: {}", agent);
            println!();

            match interface
                .remove_default_method_from_agent(&registry_id, developer, agent)
                .await
            {
                Ok(tx_digest) => {
                    println!("✅ Default method removed successfully!");
                    println!("   Transaction: {}", tx_digest);
                }
                Err(e) => {
                    eprintln!("❌ Failed to remove default method: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }

        RegistryCommands::List { registry, json } => {
            let registry_id = registry.or_else(|| {
                std::env::var("SILVANA_REGISTRY").ok()
            }).ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            match interface.list_registry(&registry_id).await {
                Ok(registry_data) => {
                    if json {
                        // Output as JSON
                        let json_str = serde_json::to_string_pretty(&registry_data)
                            .map_err(|e| anyhow!("Failed to serialize to JSON: {}", e))?;
                        println!("{}", json_str);
                    } else {
                        // Formatted output
                        println!("📋 Registry: {}\n", registry_data.name);
                        println!("   ID: {}", registry_data.registry_id);
                        println!("   Version: {}", registry_data.version);
                        println!("   Admin: {}", registry_data.admin);
                        println!();

                        // Developers
                        println!("👥 Developers ({})", registry_data.developers.len());
                        for developer in &registry_data.developers {
                            println!();
                            println!("   📦 {} ({})", developer.name, developer.owner);
                            println!("      GitHub: {}", developer.github);
                            if let Some(ref image) = developer.image {
                                println!("      Image: {}", image);
                            }
                            if let Some(ref description) = developer.description {
                                println!("      Description: {}", description);
                            }
                            if let Some(ref site) = developer.site {
                                println!("      Site: {}", site);
                            }

                            // Agents
                            if !developer.agents.is_empty() {
                                println!();
                                println!("      🤖 Agents ({})", developer.agents.len());
                                for agent in &developer.agents {
                                    println!("         • {}", agent.name);
                                    if !agent.chains.is_empty() {
                                        println!("           Chains: {}", agent.chains.join(", "));
                                    }
                                    if let Some(ref image) = agent.image {
                                        println!("           Image: {}", image);
                                    }
                                    if let Some(ref description) = agent.description {
                                        println!("           Description: {}", description);
                                    }
                                    if let Some(ref site) = agent.site {
                                        println!("           Site: {}", site);
                                    }
                                    if let Some(ref default_method) = agent.default_method {
                                        println!("           Default Method: {}", default_method);
                                    }

                                    // Methods
                                    if !agent.methods.is_empty() {
                                        println!();
                                        println!("           📝 Methods ({})", agent.methods.len());
                                        for method in &agent.methods {
                                            println!("              • {}", method.name);
                                            println!("                Docker: {}", method.docker_image);
                                            if let Some(ref sha) = method.docker_sha256 {
                                                println!("                SHA256: {}", sha);
                                            }
                                            println!(
                                                "                Resources: {} GB RAM, {} CPU cores{}",
                                                method.min_memory_gb,
                                                method.min_cpu_cores,
                                                if method.requires_tee { ", TEE required" } else { "" }
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Apps
                        if !registry_data.apps.is_empty() {
                            println!();
                            println!("📱 Apps ({})", registry_data.apps.len());
                            for app in &registry_data.apps {
                                println!("   • {} ({})", app.name, app.owner);
                                if let Some(ref description) = app.description {
                                    println!("     Description: {}", description);
                                }
                            }
                        }

                        println!();
                    }
                }
                Err(e) => {
                    eprintln!("❌ Failed to list registry: {}", e);
                    return Err(anyhow!(e).into());
                }
            }
        }
    }

    Ok(())
}