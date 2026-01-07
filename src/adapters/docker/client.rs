use async_trait::async_trait;
use bollard::container::{ListContainersOptions, StatsOptions};
use bollard::models::ContainerStateStatusEnum;
use bollard::Docker;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::domain::{Container, ContainerId, ContainerState, CpuMetrics, IoMetrics, MemoryMetrics, NetworkMetrics};
use crate::ports::{ContainerSource, ContainerStats};

/// Docker adapter using bollard client
pub struct DockerAdapter {
    client: Docker,
}

impl DockerAdapter {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = Docker::connect_with_local_defaults()?;
        Ok(Self { client })
    }

    pub fn with_socket(socket_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = Docker::connect_with_socket(socket_path, 120, bollard::API_DEFAULT_VERSION)?;
        Ok(Self { client })
    }

    fn map_container_state(state: &Option<String>) -> ContainerState {
        match state.as_deref() {
            Some("running") => ContainerState::Running,
            Some("paused") => ContainerState::Paused,
            Some("restarting") => ContainerState::Restarting,
            Some("dead") => ContainerState::Dead,
            Some("created") => ContainerState::Created,
            Some("exited") | Some("removing") => ContainerState::Stopped,
            _ => ContainerState::Stopped,
        }
    }

    fn extract_stack_name(labels: &HashMap<String, String>) -> Option<String> {
        // Try com.docker.compose.project label
        labels
            .get("com.docker.compose.project")
            .cloned()
            .or_else(|| {
                // Fallback: try label without "com." prefix
                labels.get("docker.compose.project").cloned()
            })
    }

    fn parse_container_name(names: &Option<Vec<String>>) -> String {
        names
            .as_ref()
            .and_then(|n| n.first())
            .map(|s| s.trim_start_matches('/').to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    async fn calculate_stats_from_stream(
        &self,
        id: &ContainerId,
    ) -> Result<ContainerStats, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream::StreamExt;

        let mut stream = self.client.stats(
            id.as_str(),
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );

        let stats = stream
            .next()
            .await
            .ok_or("No stats available")??;

        // Calculate CPU percentage
        let cpu_delta = stats.cpu_stats.cpu_usage.total_usage as f64
            - stats.precpu_stats.cpu_usage.total_usage as f64;
        let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0) as f64
            - stats.precpu_stats.system_cpu_usage.unwrap_or(0) as f64;

        let num_cpus = stats.cpu_stats.online_cpus.unwrap_or(1) as f64;
        let cpu_percent = if system_delta > 0.0 && cpu_delta > 0.0 {
            (cpu_delta / system_delta) * num_cpus * 100.0
        } else {
            0.0
        };

        // Memory
        let memory_used = stats.memory_stats.usage.unwrap_or(0);
        let memory_limit = stats.memory_stats.limit.unwrap_or(memory_used);
        let memory_available = memory_limit.saturating_sub(memory_used);

        // Network I/O
        let mut rx_bytes = 0u64;
        let mut tx_bytes = 0u64;
        let mut rx_errors = 0u64;
        let mut tx_errors = 0u64;

        if let Some(networks) = stats.networks {
            for (_name, net_stats) in networks {
                rx_bytes += net_stats.rx_bytes;
                tx_bytes += net_stats.tx_bytes;
                rx_errors += net_stats.rx_errors;
                tx_errors += net_stats.tx_errors;
            }
        }

        // Block I/O
        let mut read_bytes = 0u64;
        let mut write_bytes = 0u64;

        if let Some(blkio_stats) = stats.blkio_stats.io_service_bytes_recursive {
            for entry in blkio_stats {
                match entry.op.as_str() {
                    "Read" => read_bytes += entry.value,
                    "Write" => write_bytes += entry.value,
                    _ => {}
                }
            }
        }

        Ok(ContainerStats {
            cpu: CpuMetrics::new(cpu_percent, 0.0, 0.0),
            memory: MemoryMetrics::new(memory_used, memory_limit, memory_available),
            network: NetworkMetrics::new(rx_bytes, tx_bytes, rx_errors, tx_errors),
            block_io: IoMetrics::new(read_bytes, write_bytes),
        })
    }
}

#[async_trait]
impl ContainerSource for DockerAdapter {
    async fn list_containers(&self) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>> {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        let containers_list = self.client.list_containers(options).await?;
        let mut containers = Vec::new();

        for container_summary in containers_list {
            let id = ContainerId::new(container_summary.id.unwrap_or_default());
            let name = Self::parse_container_name(&container_summary.names);
            let image = container_summary.image.unwrap_or_else(|| "unknown".to_string());
            let state = Self::map_container_state(&container_summary.state);
            let created = container_summary.created.unwrap_or(0);
            let created_at = DateTime::<Utc>::from_timestamp(created, 0).unwrap_or_else(|| Utc::now());

            let labels = container_summary.labels.unwrap_or_default();
            let stack = Self::extract_stack_name(&labels);

            let mut container = Container::new(id.clone(), name, image, state, created_at).with_stack(stack);

            // Get stats for running containers only
            if state.is_running() {
                if let Ok(stats) = self.get_container_stats(&id).await {
                    container = container.with_metrics(
                        stats.cpu,
                        stats.memory,
                        stats.network,
                        stats.block_io,
                    );
                }
            }

            containers.push(container);
        }

        Ok(containers)
    }

    async fn get_container_stats(
        &self,
        id: &ContainerId,
    ) -> Result<ContainerStats, Box<dyn std::error::Error + Send + Sync>> {
        self.calculate_stats_from_stream(id).await
    }
}
