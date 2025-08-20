use crate::error::{DockerError, Result};
use bollard::auth::DockerCredentials;
use bollard::container::LogOutput;
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, ImportImageOptions,
    InspectContainerOptions, ListImagesOptions, LogsOptions, 
    PruneContainersOptions, PruneImagesOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions, WaitContainerOptions
};
use bollard::Docker;
use bytes::Bytes;
use futures_util::stream::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub image_name: String,
    pub image_source: String,
    pub command: Vec<String>,
    pub env_vars: HashMap<String, String>,
    pub port_bindings: HashMap<String, u16>,
    pub volume_binds: Vec<String>,
    pub timeout_seconds: u64,
    pub memory_limit_mb: Option<u64>,
    pub cpu_cores: Option<f64>,
    pub network_mode: Option<String>,
    pub requires_tee: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerResult {
    pub exit_code: i64,
    pub logs: String,
    pub duration_ms: u128,
    pub container_id: String,
}

pub struct DockerManager {
    docker: Docker,
    use_tee: bool,
}

impl DockerManager {
    pub fn new(use_tee: bool) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| DockerError::ConnectionError(e.to_string()))?;
        
        Ok(Self { docker, use_tee })
    }

    pub async fn load_image(&self, image_source: &str, use_local: bool) -> Result<String> {
        if use_local {
            self.load_image_from_tar(image_source).await
        } else {
            self.pull_image_from_registry(image_source).await
        }
    }

    async fn load_image_from_tar(&self, tar_path: &str) -> Result<String> {
        info!("Loading Docker image from tar file: {}", tar_path);
        
        let path = Path::new(tar_path);
        if !path.exists() {
            return Err(DockerError::ImageNotFound(format!(
                "Tar file not found at: {}",
                tar_path
            )));
        }

        let tar_data = std::fs::read(path)?;
        let bytes = Bytes::from(tar_data);

        let options = ImportImageOptions {
            quiet: false,
            platform: None,
        };

        let mut import_stream = self.docker.import_image(options, bollard::body_full(bytes), None);
        
        while let Some(progress) = import_stream.try_next().await? {
            if let Some(status) = progress.status {
                debug!("Import progress: {}", status);
            }
        }

        info!("Image loaded successfully from tar");
        self.get_image_digest(tar_path).await
    }

    async fn pull_image_from_registry(&self, image_source: &str) -> Result<String> {
        info!("Pulling Docker image from registry: {}", image_source);

        let options = Some(CreateImageOptions {
            from_image: Some(image_source.to_string()),
            ..Default::default()
        });

        let credentials = self.get_docker_credentials();

        let mut pull_stream = self.docker.create_image(options, None, credentials);

        while let Some(progress) = pull_stream.try_next().await? {
            if let Some(status) = progress.status {
                debug!("Pull progress: {}", status);
            }
        }

        info!("Image pulled successfully");
        self.get_image_digest(image_source).await
    }

    fn get_docker_credentials(&self) -> Option<DockerCredentials> {
        match (
            std::env::var("DOCKER_USERNAME").ok(),
            std::env::var("DOCKER_PASSWORD").ok(),
        ) {
            (Some(username), Some(password)) => Some(DockerCredentials {
                username: Some(username),
                password: Some(password),
                serveraddress: Some("https://index.docker.io/v1/".to_string()),
                ..Default::default()
            }),
            _ => None,
        }
    }

    async fn get_image_digest(&self, image_name: &str) -> Result<String> {
        let image_inspect = self.docker.inspect_image(image_name).await?;
        
        if let Some(repo_digests) = image_inspect.repo_digests {
            if let Some(first_digest) = repo_digests.first() {
                if let Some(sha_part) = first_digest.split('@').nth(1) {
                    return Ok(sha_part.to_string());
                }
            }
        }
        
        if let Some(id) = image_inspect.id {
            if let Some(sha_part) = id.split(':').nth(1) {
                return Ok(format!("sha256:{}", sha_part));
            }
            return Ok(id);
        }
        
        Err(DockerError::ContainerError(
            "Unable to determine image digest".to_string(),
        ))
    }

    pub async fn run_container(&self, config: &ContainerConfig) -> Result<ContainerResult> {
        let start_time = Instant::now();
        
        info!("Creating container from image: {}", config.image_name);
        
        let container_name = format!(
            "silvana-{}-{}",
            config.image_name.replace(['/', ':'], "-"),
            chrono::Utc::now().timestamp()
        );

        let container_config = self.build_container_config(config)?;
        let create_options = Some(CreateContainerOptions {
            name: Some(container_name.clone()),
            platform: String::new(), // Empty string for default platform
        });

        let container = self
            .docker
            .create_container(create_options, container_config)
            .await?;

        info!("Starting container: {}", container.id);
        self.docker
            .start_container(&container.id, None::<StartContainerOptions>)
            .await?;

        let result = timeout(
            Duration::from_secs(config.timeout_seconds),
            self.wait_for_container(&container.id),
        )
        .await;

        let (exit_code, logs) = match result {
            Ok(Ok((code, logs))) => {
                info!("Container completed with exit code: {}", code);
                (code, logs)
            }
            Ok(Err(e)) => {
                error!("Container error: {}", e);
                self.stop_and_remove_container(&container.id).await?;
                return Err(e);
            }
            Err(_) => {
                warn!(
                    "Container timeout after {} seconds, stopping...",
                    config.timeout_seconds
                );
                self.stop_and_remove_container(&container.id).await?;
                return Err(DockerError::Timeout(config.timeout_seconds));
            }
        };

        self.remove_container(&container.id).await?;

        #[cfg(feature = "tee")]
        if self.use_tee {
            self.cleanup_tee_resources().await?;
        }

        Ok(ContainerResult {
            exit_code,
            logs,
            duration_ms: start_time.elapsed().as_millis(),
            container_id: container.id,
        })
    }

    fn build_container_config(&self, config: &ContainerConfig) -> Result<ContainerCreateBody> {
        let mut env = Vec::new();
        for (key, value) in &config.env_vars {
            env.push(format!("{}={}", key, value));
        }

        let mut port_bindings = HashMap::new();
        for (container_port, host_port) in &config.port_bindings {
            port_bindings.insert(
                container_port.clone(),
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(host_port.to_string()),
                }]),
            );
        }

        let mut host_config = HostConfig {
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            network_mode: config.network_mode.clone(),
            binds: if config.volume_binds.is_empty() {
                None
            } else {
                Some(config.volume_binds.clone())
            },
            ..Default::default()
        };

        if let Some(memory_mb) = config.memory_limit_mb {
            host_config.memory = Some((memory_mb * 1024 * 1024) as i64);
        }

        if let Some(cpu_cores) = config.cpu_cores {
            host_config.cpu_quota = Some((cpu_cores * 100000.0) as i64);
            host_config.cpu_period = Some(100000);
        }

        Ok(ContainerCreateBody {
            image: Some(config.image_name.clone()),
            cmd: Some(config.command.clone()),
            env: Some(env),
            host_config: Some(host_config),
            ..Default::default()
        })
    }

    async fn wait_for_container(&self, container_id: &str) -> Result<(i64, String)> {
        if self.use_tee {
            self.wait_for_container_polling(container_id).await
        } else {
            self.wait_for_container_stream(container_id).await
        }
    }

    async fn wait_for_container_stream(&self, container_id: &str) -> Result<(i64, String)> {
        let wait_options = Some(WaitContainerOptions {
            // Use default options for wait
            ..Default::default()
        });

        let mut status_stream = self.docker.wait_container(container_id, wait_options);

        let exit_code = if let Some(status) = status_stream.try_next().await? {
            status.status_code
        } else {
            0
        };

        let logs = self.get_container_logs(container_id).await?;
        Ok((exit_code, logs))
    }

    async fn wait_for_container_polling(&self, container_id: &str) -> Result<(i64, String)> {
        info!("Polling container status (TEE mode)...");
        
        loop {
            let details = self
                .docker
                .inspect_container(container_id, None::<InspectContainerOptions>)
                .await?;

            if let Some(state) = details.state {
                if state.running == Some(false) {
                    let exit_code = state.exit_code.unwrap_or(0);
                    info!("Container exited with code: {}", exit_code);
                    
                    let logs = self.get_container_logs(container_id).await?;
                    
                    if exit_code != 0 {
                        return Err(DockerError::ContainerError(format!(
                            "Container exited with non-zero status: {}",
                            exit_code
                        )));
                    }
                    
                    return Ok((exit_code, logs));
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn get_container_logs(&self, container_id: &str) -> Result<String> {
        let logs_options = Some(LogsOptions {
            stdout: true,
            stderr: true,
            since: 0,
            until: 0,
            timestamps: false,
            follow: false,
            tail: "all".to_string(),
        });

        let mut logs_stream = self.docker.logs(container_id, logs_options);
        let mut all_logs = String::new();

        while let Some(log_result) = logs_stream.try_next().await? {
            match log_result {
                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                    all_logs.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }

        Ok(all_logs)
    }

    async fn stop_and_remove_container(&self, container_id: &str) -> Result<()> {
        let stop_options = Some(StopContainerOptions { 
            t: Some(30),
            ..Default::default()
        });
        
        if let Err(e) = self.docker.stop_container(container_id, stop_options).await {
            warn!("Failed to stop container: {}", e);
        }

        self.remove_container(container_id).await
    }

    async fn remove_container(&self, container_id: &str) -> Result<()> {
        let remove_options = Some(RemoveContainerOptions {
            force: true,
            ..Default::default()
        });

        self.docker
            .remove_container(container_id, remove_options)
            .await?;
        
        Ok(())
    }

    #[cfg(feature = "tee")]
    async fn cleanup_tee_resources(&self) -> Result<()> {
        crate::tee::cleanup_resources().await
    }

    #[cfg(not(feature = "tee"))]
    #[allow(dead_code)]
    async fn cleanup_tee_resources(&self) -> Result<()> {
        Ok(())
    }

    pub async fn list_images(&self) -> Result<Vec<String>> {
        let options = Some(ListImagesOptions::default());
        let images = self.docker.list_images(options).await?;
        
        let mut image_names = Vec::new();
        for img in images {
            // In bollard 0.19, repo_tags is Vec<String>
            for tag in img.repo_tags {
                image_names.push(tag);
            }
        }
        
        Ok(image_names)
    }

    pub async fn prune_images(&self) -> Result<()> {
        let options: Option<PruneImagesOptions> = None;
        let _ = self.docker.prune_images(options).await?;
        Ok(())
    }

    pub async fn prune_containers(&self) -> Result<()> {
        let options: Option<PruneContainersOptions> = None;
        let _ = self.docker.prune_containers(options).await?;
        Ok(())
    }
}