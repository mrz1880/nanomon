use async_trait::async_trait;
use bollard::container::{ListContainersOptions, Stats, StatsOptions};
use bollard::models::ContainerSummary;
use bollard::Docker;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::domain::{
    Container, ContainerId, ContainerState, CpuMetrics, IoMetrics, MemoryMetrics, NetworkMetrics,
};
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

    #[allow(dead_code)]
    pub fn with_socket(
        socket_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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

        let stats = stream.next().await.ok_or("No stats available")??;
        Ok(stats_from_sample(stats))
    }
}

/// Maps a Docker stats sample to NanoMon's container metrics.
///
/// # Arguments
/// * `stats` - One sample from the Docker stats endpoint.
///
/// # Returns
/// CPU, memory, network and block I/O metrics of the container.
fn stats_from_sample(stats: Stats) -> ContainerStats {
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

    ContainerStats {
        cpu: CpuMetrics::new(cpu_percent, 0.0, 0.0),
        memory: MemoryMetrics::new(memory_used, memory_limit, memory_available),
        network: NetworkMetrics::new(rx_bytes, tx_bytes, rx_errors, tx_errors),
        block_io: IoMetrics::new(read_bytes, write_bytes),
    }
}

/// Maps a Docker container summary to a NanoMon container, without metrics.
///
/// # Arguments
/// * `summary` - One entry of the Docker container list.
///
/// # Returns
/// The container with its identity, image, state and compose stack.
fn container_from_summary(summary: ContainerSummary) -> Container {
    let id = ContainerId::new(summary.id.unwrap_or_default());
    let name = DockerAdapter::parse_container_name(&summary.names);
    let image = summary.image.unwrap_or_else(|| "unknown".to_string());
    let state = DockerAdapter::map_container_state(&summary.state);
    let created = summary.created.unwrap_or(0);
    let created_at = DateTime::<Utc>::from_timestamp(created, 0).unwrap_or_else(Utc::now);

    let labels = summary.labels.unwrap_or_default();
    let stack = DockerAdapter::extract_stack_name(&labels);

    Container::new(id, name, image, state, created_at).with_stack(stack)
}

#[async_trait]
impl ContainerSource for DockerAdapter {
    async fn list_containers(
        &self,
    ) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>> {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        let containers_list = self.client.list_containers(options).await?;
        let mut containers = Vec::new();

        for container_summary in containers_list {
            let mut container = container_from_summary(container_summary);

            // Get stats for running containers only
            if container.state.is_running() {
                if let Ok(stats) = self.get_container_stats(&container.id).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A stats sample shaped like the Docker Engine API answers on a cgroup v2
    /// host, with round numbers. Fixtures are JSON rather than bollard structs
    /// so they survive bollard's type changes between versions.
    fn sample(blkio_op_read: &str, blkio_op_write: &str) -> Stats {
        let json = format!(
            r#"{{
  "id": "abc123", "name": "/web",
  "read": "2026-10-22T16:14:55.689221158Z", "preread": "0001-01-01T00:00:00Z",
  "num_procs": 0,
  "pids_stats": {{ "current": 14, "limit": 9166 }},
  "storage_stats": {{}},
  "blkio_stats": {{
    "io_service_bytes_recursive": [
      {{ "major": 8, "minor": 0, "op": "{blkio_op_read}", "value": 1000 }},
      {{ "major": 8, "minor": 0, "op": "{blkio_op_write}", "value": 400 }},
      {{ "major": 8, "minor": 16, "op": "{blkio_op_read}", "value": 24 }}
    ],
    "io_serviced_recursive": null, "io_queue_recursive": null,
    "io_service_time_recursive": null, "io_wait_time_recursive": null,
    "io_merged_recursive": null, "io_time_recursive": null,
    "sectors_recursive": null
  }},
  "cpu_stats": {{
    "cpu_usage": {{ "total_usage": 3000, "usage_in_kernelmode": 1000, "usage_in_usermode": 2000 }},
    "system_cpu_usage": 40000, "online_cpus": 4,
    "throttling_data": {{ "periods": 0, "throttled_periods": 0, "throttled_time": 0 }}
  }},
  "precpu_stats": {{
    "cpu_usage": {{ "total_usage": 1000, "usage_in_kernelmode": 0, "usage_in_usermode": 0 }},
    "system_cpu_usage": 20000, "online_cpus": 4,
    "throttling_data": {{ "periods": 0, "throttled_periods": 0, "throttled_time": 0 }}
  }},
  "memory_stats": {{ "usage": 600, "limit": 1000 }},
  "networks": {{
    "eth0": {{ "rx_bytes": 100, "rx_packets": 1, "rx_errors": 1, "rx_dropped": 0,
              "tx_bytes": 200, "tx_packets": 1, "tx_errors": 2, "tx_dropped": 0 }},
    "eth1": {{ "rx_bytes": 10, "rx_packets": 1, "rx_errors": 0, "rx_dropped": 0,
              "tx_bytes": 20, "tx_packets": 1, "tx_errors": 0, "tx_dropped": 0 }}
  }}
}}"#
        );
        serde_json::from_str(&json).expect("fixture matches the Docker stats schema")
    }

    #[test]
    fn cpu_is_the_share_of_host_time_scaled_by_online_cpus() {
        // (3000 - 1000) / (40000 - 20000) * 4 cpus * 100
        let stats = stats_from_sample(sample("Read", "Write"));
        assert!((stats.cpu.usage_percent - 40.0).abs() < 1e-9);
    }

    #[test]
    fn memory_available_is_what_the_limit_leaves() {
        let stats = stats_from_sample(sample("Read", "Write"));
        assert_eq!(stats.memory.used_bytes, 600);
        assert_eq!(stats.memory.total_bytes, 1000);
        assert_eq!(stats.memory.available_bytes, 400);
    }

    #[test]
    fn network_counters_are_summed_across_interfaces() {
        let stats = stats_from_sample(sample("Read", "Write"));
        assert_eq!(stats.network.rx_bytes, 110);
        assert_eq!(stats.network.tx_bytes, 220);
        assert_eq!(stats.network.rx_errors, 1);
        assert_eq!(stats.network.tx_errors, 2);
    }

    #[test]
    fn block_io_is_summed_across_devices_per_direction() {
        let stats = stats_from_sample(sample("Read", "Write"));
        assert_eq!(stats.block_io.read_bytes, 1024);
        assert_eq!(stats.block_io.write_bytes, 400);
    }

    #[test]
    fn summary_maps_name_image_state_and_compose_stack() {
        let summary: ContainerSummary = serde_json::from_str(
            r#"{
  "Id": "abc123", "Names": ["/web"], "Image": "nginx:1.27",
  "Created": 1792685202, "State": "running", "Status": "Up 8 minutes",
  "Labels": { "com.docker.compose.project": "site" }
}"#,
        )
        .expect("fixture matches the Docker container summary schema");

        let container = container_from_summary(summary);

        assert_eq!(container.id.as_str(), "abc123");
        assert_eq!(container.name, "web");
        assert_eq!(container.image, "nginx:1.27");
        assert!(container.state.is_running());
        assert_eq!(container.stack.as_deref(), Some("site"));
        assert_eq!(container.created_at.timestamp(), 1792685202);
    }
}
