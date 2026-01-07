use async_trait::async_trait;

use crate::domain::{Container, ContainerId, CpuMetrics, IoMetrics, MemoryMetrics, NetworkMetrics};

/// Stats for a single container
#[derive(Debug, Clone)]
pub struct ContainerStats {
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub network: NetworkMetrics,
    pub block_io: IoMetrics,
}

/// Port for fetching container information
#[async_trait]
pub trait ContainerSource: Send + Sync {
    /// List all containers (running and stopped)
    async fn list_containers(&self) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>>;

    /// Get real-time stats for a specific container
    async fn get_container_stats(
        &self,
        id: &ContainerId,
    ) -> Result<ContainerStats, Box<dyn std::error::Error + Send + Sync>>;
}
