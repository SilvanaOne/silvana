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
            let registry_id = registry.ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("👤 Adding developer to registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            println!("   GitHub: {}", github);
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
            let registry_id = registry.ok_or_else(|| {
                anyhow!("Registry ID not provided. Set SILVANA_REGISTRY environment variable or use --registry")
            })?;

            println!("📝 Updating developer in registry...\n");
            println!("   Registry: {}", registry_id);
            println!("   Name: {}", name);
            println!("   GitHub: {}", github);
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
            let registry_id = registry.ok_or_else(|| {
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
            let registry_id = registry.ok_or_else(|| {
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
            let registry_id = registry.ok_or_else(|| {
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
            let registry_id = registry.ok_or_else(|| {
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
            let registry_id = registry.ok_or_else(|| {
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
                .add_app_to_registry(&registry_id, name, description)
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
            let registry_id = registry.ok_or_else(|| {
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
            let registry_id = registry.ok_or_else(|| {
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
    }

    Ok(())
}