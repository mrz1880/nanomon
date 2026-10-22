use async_trait::async_trait;
use bollard::models::{
    ContainerNetworkStats, ContainerStatsResponse, ContainerSummary, ContainerSummaryStateEnum,
};
use bollard::query_parameters::{ListContainersOptions, StatsOptions};
use bollard::Docker;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Mutex;

use super::cpu::{CpuSample, CpuSamples};
use crate::domain::{
    Container, ContainerId, ContainerState, CpuMetrics, IoMetrics, MemoryMetrics, NetworkMetrics,
};
use crate::ports::{ContainerSource, ContainerStats};

// A std lock is "poisoned" when a thread panics while holding its guard;
// every later lock() then returns Err. The critical sections below only
// update a map and never panic, so poisoning cannot happen.
const LOCK_INVARIANT: &str = "CPU samples lock poisoned: no code panics while holding it";

/// Docker adapter using bollard client
pub struct DockerAdapter {
    client: Docker,
    /// CPU counters of the previous collection, per container. `Mutex` gives
    /// the `&self` trait methods mutable access, shared by the collector and
    /// the HTTP handlers that both list containers.
    cpu_samples: Mutex<CpuSamples>,
}

impl DockerAdapter {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = Docker::connect_with_local_defaults()?;
        Ok(Self::with_client(client))
    }

    fn with_client(client: Docker) -> Self {
        Self {
            client,
            cpu_samples: Mutex::new(CpuSamples::default()),
        }
    }

    #[allow(dead_code)]
    pub fn with_socket(
        socket_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let client = Docker::connect_with_socket(socket_path, 120, bollard::API_DEFAULT_VERSION)?;
        Ok(Self::with_client(client))
    }

    fn map_container_state(state: Option<ContainerSummaryStateEnum>) -> ContainerState {
        match state {
            Some(ContainerSummaryStateEnum::RUNNING) => ContainerState::Running,
            Some(ContainerSummaryStateEnum::PAUSED) => ContainerState::Paused,
            Some(ContainerSummaryStateEnum::RESTARTING) => ContainerState::Restarting,
            Some(ContainerSummaryStateEnum::DEAD) => ContainerState::Dead,
            Some(ContainerSummaryStateEnum::CREATED) => ContainerState::Created,
            Some(
                ContainerSummaryStateEnum::EXITED
                | ContainerSummaryStateEnum::REMOVING
                | ContainerSummaryStateEnum::STOPPING
                | ContainerSummaryStateEnum::EMPTY,
            )
            | None => ContainerState::Stopped,
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
        // The guard is a temporary dropped at the end of this statement, so the
        // lock is never held across an `.await`.
        Ok(stats_from_sample(
            id,
            stats,
            &mut self.cpu_samples.lock().expect(LOCK_INVARIANT),
        ))
    }
}

/// Maps a Docker stats sample to NanoMon's container metrics.
///
/// # Arguments
/// * `id` - Container the sample belongs to.
/// * `stats` - One sample from the Docker stats endpoint.
/// * `cpu_samples` - CPU counters of the previous collection; updated.
///
/// # Returns
/// CPU, memory, network and block I/O metrics of the container. CPU reads 0
/// on the first collection of a container, which has no reference yet.
fn stats_from_sample(
    id: &ContainerId,
    stats: ContainerStatsResponse,
    cpu_samples: &mut CpuSamples,
) -> ContainerStats {
    // Docker's CPU counters are cumulative: the load is measured against the
    // sample of the previous collection, since a one-shot sample has no precpu.
    let cpu_percent = cpu_samples.record(id.clone(), CpuSample::from_stats(&stats));

    // Since bollard 0.21 every field of the sample is optional: an absent
    // counter is read as 0, as Docker omits what the cgroup does not report.
    // Memory
    let memory = stats.memory_stats.as_ref();
    let memory_used = memory.and_then(|m| m.usage).unwrap_or(0);
    let memory_limit = memory.and_then(|m| m.limit).unwrap_or(memory_used);
    let memory_available = memory_limit.saturating_sub(memory_used);

    // Network I/O, summed over the container's interfaces
    let networks = stats.networks.unwrap_or_default();
    let sum = |field: fn(&ContainerNetworkStats) -> Option<u64>| -> u64 {
        networks.values().filter_map(field).sum()
    };
    let rx_bytes = sum(|n| n.rx_bytes);
    let tx_bytes = sum(|n| n.tx_bytes);
    let rx_errors = sum(|n| n.rx_errors);
    let tx_errors = sum(|n| n.tx_errors);

    // Block I/O
    let mut read_bytes = 0u64;
    let mut write_bytes = 0u64;

    let entries = stats
        .blkio_stats
        .and_then(|b| b.io_service_bytes_recursive)
        .unwrap_or_default();
    for entry in entries {
        let value = entry.value.unwrap_or(0);
        // cgroup v1 spells the op "Read", cgroup v2 "read".
        let op = entry.op.unwrap_or_default().to_ascii_lowercase();
        match op.as_str() {
            "read" => read_bytes += value,
            "write" => write_bytes += value,
            // Other ops (sync, async, discard, total) are not traffic directions.
            _ => {}
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
    let state = DockerAdapter::map_container_state(summary.state);
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
        let options = Some(ListContainersOptions {
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

        let listed: Vec<ContainerId> = containers.iter().map(|c| c.id.clone()).collect();
        self.cpu_samples
            .lock()
            .expect(LOCK_INVARIANT)
            .keep_only(&listed);

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
    fn sample(blkio_op_read: &str, blkio_op_write: &str) -> ContainerStatsResponse {
        sample_at(
            blkio_op_read,
            blkio_op_write,
            CpuCounters {
                total: 3000,
                system: 40000,
            },
        )
    }

    /// Cumulative CPU counters of a fixture, in nanoseconds.
    struct CpuCounters {
        total: u64,
        system: u64,
    }

    fn sample_at(
        blkio_op_read: &str,
        blkio_op_write: &str,
        cpu: CpuCounters,
    ) -> ContainerStatsResponse {
        let (total_usage, system_cpu_usage) = (cpu.total, cpu.system);
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
    "cpu_usage": {{ "total_usage": {total_usage}, "usage_in_kernelmode": 0, "usage_in_usermode": 0 }},
    "system_cpu_usage": {system_cpu_usage}, "online_cpus": 4,
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

    /// Maps a sample with no CPU history, for the tests that ignore CPU.
    fn map(stats: ContainerStatsResponse) -> ContainerStats {
        stats_from_sample(
            &ContainerId::new("abc123"),
            stats,
            &mut CpuSamples::default(),
        )
    }

    #[test]
    fn cpu_usage_is_measured_between_collections_not_from_docker_precpu() {
        let id = ContainerId::new("abc123");
        let mut cpu = CpuSamples::default();
        // The fixture's precpu (1000 / 20000) would read as 40% here, but a
        // one-shot sample has none in production: no reference yet.
        let earlier = sample_at(
            "Read",
            "Write",
            CpuCounters {
                total: 2000,
                system: 30000,
            },
        );
        let first = stats_from_sample(&id, earlier, &mut cpu);
        assert_eq!(first.cpu.usage_percent, 0.0);

        // Measured from the previous collection: (5000 - 2000) / (40000 - 30000)
        // * 4 cpus * 100. The precpu would have given 80%.
        let now = sample_at(
            "Read",
            "Write",
            CpuCounters {
                total: 5000,
                system: 40000,
            },
        );
        let second = stats_from_sample(&id, now, &mut cpu);
        assert!(
            (second.cpu.usage_percent - 120.0).abs() < 1e-9,
            "got {}",
            second.cpu.usage_percent
        );
    }

    #[test]
    fn memory_available_is_what_the_limit_leaves() {
        let stats = map(sample("Read", "Write"));
        assert_eq!(stats.memory.used_bytes, 600);
        assert_eq!(stats.memory.total_bytes, 1000);
        assert_eq!(stats.memory.available_bytes, 400);
    }

    #[test]
    fn network_counters_are_summed_across_interfaces() {
        let stats = map(sample("Read", "Write"));
        assert_eq!(stats.network.rx_bytes, 110);
        assert_eq!(stats.network.tx_bytes, 220);
        assert_eq!(stats.network.rx_errors, 1);
        assert_eq!(stats.network.tx_errors, 2);
    }

    #[test]
    fn block_io_is_summed_across_devices_per_direction() {
        let stats = map(sample("Read", "Write"));
        assert_eq!(stats.block_io.read_bytes, 1024);
        assert_eq!(stats.block_io.write_bytes, 400);
    }

    #[test]
    fn block_io_is_counted_with_cgroup_v2_lowercase_ops() {
        // cgroup v1 reports "Read"/"Write", cgroup v2 hosts "read"/"write".
        let stats = map(sample("read", "write"));
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
