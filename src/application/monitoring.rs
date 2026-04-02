use std::sync::Arc;

use chrono::Utc;

use crate::domain::{Container, Host, Process, Stack, SystemdService};
use crate::ports::{ContainerSource, MetricStore, ProcessSource, ServiceSource, SystemSource};

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
        let mut stacks_map = std::collections::HashMap::new();

        for container in containers {
            if let Some(stack_name) = &container.stack {
                stacks_map
                    .entry(stack_name.clone())
                    .or_insert_with(Vec::new)
                    .push(container);
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

        Ok(stacks)
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
