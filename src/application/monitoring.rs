use std::sync::Arc;

use chrono::Utc;
use tracing::warn;

use crate::domain::{
    Container, CpuMetrics, Disk, Host, LoadAverage, MemoryMetrics, Process, Stack, SystemdService,
    Temperature,
};
use crate::ports::{
    ContainerSource, HostInfo, MetricStore, ProcessSource, ServiceSource, SystemSource,
};

/// Slow-tier resources collected less frequently than host metrics.
pub struct SlowResources {
    pub disks: Vec<Disk>,
    pub temperatures: Vec<Temperature>,
    pub containers: Vec<Container>,
    pub processes: Vec<Process>,
    pub services: Vec<SystemdService>,
}

/// Main application service for monitoring
pub struct MonitoringService {
    system_source: Arc<dyn SystemSource>,
    container_source: Arc<dyn ContainerSource>,
    process_source: Arc<dyn ProcessSource>,
    service_source: Option<Arc<dyn ServiceSource>>,
    metric_store: Arc<dyn MetricStore>,
}

impl MonitoringService {
    pub fn new(
        system_source: Arc<dyn SystemSource>,
        container_source: Arc<dyn ContainerSource>,
        process_source: Arc<dyn ProcessSource>,
        metric_store: Arc<dyn MetricStore>,
    ) -> Self {
        Self {
            system_source,
            container_source,
            process_source,
            service_source: None,
            metric_store,
        }
    }

    pub fn with_service_source(mut self, source: Arc<dyn ServiceSource>) -> Self {
        if source.is_available() {
            self.service_source = Some(source);
        }
        self
    }

    /// Collect a complete host snapshot with all metrics
    pub async fn collect_all(&self) -> Result<Host, Box<dyn std::error::Error + Send + Sync>> {
        // Collect all metrics in parallel
        let (host_info, cpu, memory, load_avg, disks, interfaces, containers, processes) = tokio::try_join!(
            self.system_source.get_host_info(),
            self.system_source.get_cpu_metrics(),
            self.system_source.get_memory_metrics(),
            self.system_source.get_load_average(),
            self.system_source.list_disks(),
            self.system_source.list_network_interfaces(),
            self.container_source.list_containers(),
            self.process_source.list_processes(),
        )?;

        // Temperatures are optional - don't fail the whole collection
        let temperatures = self
            .system_source
            .get_temperatures()
            .await
            .unwrap_or_default();

        let host = Host::new(host_info.hostname)
            .with_metrics(host_info.uptime_seconds, load_avg, cpu, memory)
            .with_network_interfaces(interfaces)
            .with_disks(disks)
            .with_containers(containers)
            .with_processes(processes)
            .with_temperatures(temperatures)
            .with_timestamp(Utc::now());

        Ok(host)
    }

    /// Collect fast-tier host metrics only (cpu, memory, load, network).
    ///
    /// This is the sole reader of the CPU `/proc/stat` delta baseline, so the
    /// fast loop must be the only caller in the hot path; read handlers serve
    /// from the store instead of re-collecting.
    pub async fn collect_host(&self) -> Result<Host, Box<dyn std::error::Error + Send + Sync>> {
        // Collect each metric independently so a single unreadable source
        // (e.g. a permission error) degrades that metric instead of blanking
        // the whole snapshot.
        let host_info = self
            .system_source
            .get_host_info()
            .await
            .unwrap_or_else(|e| {
                warn!("Host info unavailable: {}", e);
                HostInfo {
                    hostname: "unknown".to_string(),
                    uptime_seconds: 0,
                }
            });
        let cpu = self
            .system_source
            .get_cpu_metrics()
            .await
            .unwrap_or_else(|e| {
                warn!("CPU metrics unavailable: {}", e);
                CpuMetrics::new(0.0, 0.0, 0.0)
            });
        let memory = self
            .system_source
            .get_memory_metrics()
            .await
            .unwrap_or_else(|e| {
                warn!("Memory metrics unavailable: {}", e);
                MemoryMetrics::new(0, 0, 0)
            });
        let load_avg = self
            .system_source
            .get_load_average()
            .await
            .unwrap_or_else(|e| {
                warn!("Load average unavailable: {}", e);
                LoadAverage::zero()
            });
        let interfaces = self
            .system_source
            .list_network_interfaces()
            .await
            .unwrap_or_else(|e| {
                warn!("Network interfaces unavailable: {}", e);
                Vec::new()
            });

        let host = Host::new(host_info.hostname)
            .with_metrics(host_info.uptime_seconds, load_avg, cpu, memory)
            .with_network_interfaces(interfaces)
            .with_timestamp(Utc::now());

        Ok(host)
    }

    /// Collect slow-tier resources (disks, temperatures, containers, processes).
    pub async fn collect_slow(
        &self,
    ) -> Result<SlowResources, Box<dyn std::error::Error + Send + Sync>> {
        // Each source degrades independently: an unavailable one yields an empty
        // list (logged) rather than failing the whole slow-tier collection.
        let disks = self.system_source.list_disks().await.unwrap_or_else(|e| {
            warn!("Disks unavailable: {}", e);
            Vec::new()
        });
        let containers = self
            .container_source
            .list_containers()
            .await
            .unwrap_or_else(|e| {
                warn!("Containers unavailable: {}", e);
                Vec::new()
            });
        let processes = self
            .process_source
            .list_processes()
            .await
            .unwrap_or_else(|e| {
                warn!("Processes unavailable: {}", e);
                Vec::new()
            });

        // Temperatures are optional - don't fail the whole collection.
        let temperatures = self
            .system_source
            .get_temperatures()
            .await
            .unwrap_or_default();

        // Services are optional (systemd may be disabled/unavailable).
        let services = self.get_services().await.unwrap_or_default();

        Ok(SlowResources {
            disks,
            temperatures,
            containers,
            processes,
            services,
        })
    }

    /// Store a snapshot in the metric store
    pub fn store_snapshot(&self, snapshot: Host) {
        self.metric_store.store(snapshot);
    }

    /// Get history from the metric store
    pub fn get_history(&self, duration: std::time::Duration) -> Vec<Arc<Host>> {
        self.metric_store.get_history(duration)
    }

    /// Get the latest stored snapshot
    pub fn get_latest_snapshot(&self) -> Option<Arc<Host>> {
        self.metric_store.get_latest()
    }

    /// Get all containers
    pub async fn get_containers(
        &self,
    ) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>> {
        self.container_source.list_containers().await
    }

    /// Get containers grouped by stack
    pub async fn get_stacks(&self) -> Result<Vec<Stack>, Box<dyn std::error::Error + Send + Sync>> {
        let containers = self.get_containers().await?;
        Ok(Self::group_stacks(&containers))
    }

    /// Group containers into stacks, sorted by CPU usage (top consumers first).
    ///
    /// Pure function so it can be reused against a stored snapshot without a
    /// fresh Docker call.
    pub fn group_stacks(containers: &[Container]) -> Vec<Stack> {
        let mut stacks_map: std::collections::HashMap<String, Vec<Container>> =
            std::collections::HashMap::new();

        for container in containers {
            if let Some(stack_name) = &container.stack {
                stacks_map
                    .entry(stack_name.clone())
                    .or_default()
                    .push(container.clone());
            }
        }

        let mut stacks: Vec<Stack> = stacks_map
            .into_iter()
            .map(|(name, containers)| Stack::from_containers(name, &containers))
            .collect();

        // Sort stacks by CPU usage (descending) to highlight top consumers
        stacks.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        stacks
    }

    /// Get top N processes sorted by CPU
    pub async fn get_top_processes_by_cpu(
        &self,
        n: usize,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        self.process_source.get_top_by_cpu(n).await
    }

    /// Get top N processes sorted by memory
    pub async fn get_top_processes_by_memory(
        &self,
        n: usize,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        self.process_source.get_top_by_memory(n).await
    }

    /// Get all processes
    #[allow(dead_code)]
    pub async fn get_all_processes(
        &self,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        self.process_source.list_processes().await
    }

    /// Get systemd services (returns empty vec if unavailable)
    pub async fn get_services(
        &self,
    ) -> Result<Vec<SystemdService>, Box<dyn std::error::Error + Send + Sync>> {
        match &self.service_source {
            Some(source) => source.list_services().await,
            None => Ok(Vec::new()),
        }
    }

    /// Check if systemd monitoring is available
    pub fn has_services(&self) -> bool {
        self.service_source.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::MonitoringService;
    use crate::domain::{
        Container, ContainerId, ContainerState, CpuMetrics, IoMetrics, MemoryMetrics,
        NetworkMetrics,
    };
    use chrono::Utc;

    fn container(name: &str, stack: Option<&str>, cpu: f64, mem: u64, running: bool) -> Container {
        let state = if running {
            ContainerState::Running
        } else {
            ContainerState::Stopped
        };
        Container::new(
            ContainerId::new(name),
            name.to_string(),
            "img".to_string(),
            state,
            Utc::now(),
        )
        .with_stack(stack.map(|s| s.to_string()))
        .with_metrics(
            CpuMetrics::new(cpu, 0.0, 0.0),
            MemoryMetrics::new(mem, 0, 0),
            NetworkMetrics::zero(),
            IoMetrics::zero(),
        )
    }

    #[test]
    fn group_stacks_groups_sums_and_sorts_by_cpu() {
        let containers = vec![
            container("c1", Some("a"), 10.0, 100, true),
            container("c2", Some("a"), 5.0, 200, false),
            container("c3", Some("b"), 30.0, 50, true),
            container("standalone", None, 99.0, 999, true),
        ];

        let stacks = MonitoringService::group_stacks(&containers);

        // Containers without a stack are not grouped into any stack.
        assert_eq!(stacks.len(), 2);

        // Sorted by CPU usage descending: "b" (30) before "a" (15).
        assert_eq!(stacks[0].name, "b");
        assert_eq!(stacks[0].cpu_percent, 30.0);

        assert_eq!(stacks[1].name, "a");
        assert_eq!(stacks[1].cpu_percent, 15.0);
        assert_eq!(stacks[1].memory_bytes, 300);
        assert_eq!(stacks[1].containers_total, 2);
        assert_eq!(stacks[1].containers_running, 1);
    }

    // --- Graceful degradation ---

    use crate::domain::{Disk, Host, LoadAverage, NetworkInterface, Process};
    use crate::ports::{
        ContainerSource, ContainerStats, HostInfo, MetricStore, ProcessSource, SystemSource,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::time::Duration;

    type BoxErr = Box<dyn std::error::Error + Send + Sync>;

    struct MockSys {
        fail_cpu: bool,
    }

    #[async_trait]
    impl SystemSource for MockSys {
        async fn get_host_info(&self) -> Result<HostInfo, BoxErr> {
            Ok(HostInfo {
                hostname: "h".to_string(),
                uptime_seconds: 42,
            })
        }
        async fn get_cpu_metrics(&self) -> Result<CpuMetrics, BoxErr> {
            if self.fail_cpu {
                Err("cpu boom".into())
            } else {
                Ok(CpuMetrics::new(50.0, 0.0, 0.0))
            }
        }
        async fn get_memory_metrics(&self) -> Result<MemoryMetrics, BoxErr> {
            Ok(MemoryMetrics::new(100, 200, 100))
        }
        async fn get_load_average(&self) -> Result<LoadAverage, BoxErr> {
            Ok(LoadAverage::new(1.0, 2.0, 3.0))
        }
        async fn list_disks(&self) -> Result<Vec<Disk>, BoxErr> {
            Ok(vec![])
        }
        async fn list_network_interfaces(&self) -> Result<Vec<NetworkInterface>, BoxErr> {
            Ok(vec![])
        }
    }

    struct MockContainers;
    #[async_trait]
    impl ContainerSource for MockContainers {
        async fn list_containers(&self) -> Result<Vec<Container>, BoxErr> {
            Ok(vec![])
        }
        async fn get_container_stats(&self, _id: &ContainerId) -> Result<ContainerStats, BoxErr> {
            Err("n/a".into())
        }
    }

    struct MockProcs;
    #[async_trait]
    impl ProcessSource for MockProcs {
        async fn list_processes(&self) -> Result<Vec<Process>, BoxErr> {
            Ok(vec![])
        }
        async fn get_top_by_cpu(&self, _n: usize) -> Result<Vec<Process>, BoxErr> {
            Ok(vec![])
        }
        async fn get_top_by_memory(&self, _n: usize) -> Result<Vec<Process>, BoxErr> {
            Ok(vec![])
        }
    }

    struct MockStore;
    impl MetricStore for MockStore {
        fn store(&self, _snapshot: Host) {}
        fn get_latest(&self) -> Option<Arc<Host>> {
            None
        }
        fn get_history(&self, _d: Duration) -> Vec<Arc<Host>> {
            vec![]
        }
        fn len(&self) -> usize {
            0
        }
    }

    #[tokio::test]
    async fn collect_host_degrades_on_partial_failure() {
        let svc = MonitoringService::new(
            Arc::new(MockSys { fail_cpu: true }),
            Arc::new(MockContainers),
            Arc::new(MockProcs),
            Arc::new(MockStore),
        );

        let host = svc.collect_host().await.unwrap();

        // The CPU source failed -> defaulted to zero, but every other metric
        // was still collected.
        assert_eq!(host.cpu.usage_percent, 0.0);
        assert_eq!(host.memory.total_bytes, 200);
        assert_eq!(host.load_average.one, 1.0);
        assert_eq!(host.hostname, "h");
    }
}
