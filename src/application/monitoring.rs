use std::sync::Arc;

use chrono::Utc;

use crate::domain::{Container, Host, Process, Stack};
use crate::ports::{ContainerSource, ProcessSource, SystemSource};

/// Main application service for monitoring
pub struct MonitoringService {
    system_source: Arc<dyn SystemSource>,
    container_source: Arc<dyn ContainerSource>,
    process_source: Arc<dyn ProcessSource>,
}

impl MonitoringService {
    pub fn new(
        system_source: Arc<dyn SystemSource>,
        container_source: Arc<dyn ContainerSource>,
        process_source: Arc<dyn ProcessSource>,
    ) -> Self {
        Self {
            system_source,
            container_source,
            process_source,
        }
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

        let host = Host::new(host_info.hostname)
            .with_metrics(host_info.uptime_seconds, load_avg, cpu, memory)
            .with_network_interfaces(interfaces)
            .with_disks(disks)
            .with_containers(containers)
            .with_processes(processes)
            .with_timestamp(Utc::now());

        Ok(host)
    }

    /// Get all containers
    pub async fn get_containers(&self) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>> {
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

        let stacks = stacks_map
            .into_iter()
            .map(|(name, containers)| Stack::from_containers(name, &containers))
            .collect();

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
    pub async fn get_all_processes(&self) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        self.process_source.list_processes().await
    }
}
