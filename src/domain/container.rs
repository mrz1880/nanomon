use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{CpuMetrics, IoMetrics, MemoryMetrics, MonitoredResource, NetworkMetrics, ResourceType};

/// Unique identifier for a container
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContainerId(String);

impl ContainerId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ContainerId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl From<&str> for ContainerId {
    fn from(id: &str) -> Self {
        Self(id.to_string())
    }
}

/// Container state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    Running,
    Stopped,
    Paused,
    Restarting,
    Dead,
    Created,
}

impl ContainerState {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// Container entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub id: ContainerId,
    pub name: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>, // com.docker.compose.project label
    pub state: ContainerState,
    pub created_at: DateTime<Utc>,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub network: NetworkMetrics,
    pub block_io: IoMetrics,
}

impl Container {
    pub fn new(
        id: ContainerId,
        name: String,
        image: String,
        state: ContainerState,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            name,
            image,
            stack: None,
            state,
            created_at,
            cpu: CpuMetrics::new(0.0, 0.0, 0.0),
            memory: MemoryMetrics::new(0, 0, 0),
            network: NetworkMetrics::zero(),
            block_io: IoMetrics::zero(),
        }
    }

    pub fn with_stack(mut self, stack: Option<String>) -> Self {
        self.stack = stack;
        self
    }

    pub fn with_metrics(
        mut self,
        cpu: CpuMetrics,
        memory: MemoryMetrics,
        network: NetworkMetrics,
        block_io: IoMetrics,
    ) -> Self {
        self.cpu = cpu;
        self.memory = memory;
        self.network = network;
        self.block_io = block_io;
        self
    }
}

impl MonitoredResource for Container {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Container
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn cpu_percent(&self) -> Option<f64> {
        Some(self.cpu.usage_percent)
    }

    fn memory_bytes(&self) -> Option<u64> {
        Some(self.memory.used_bytes)
    }

    fn is_healthy(&self) -> bool {
        self.state.is_running()
    }
}

/// Stack aggregation (multiple containers sharing a compose project)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub name: String,
    pub containers_total: usize,
    pub containers_running: usize,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
}

impl Stack {
    pub fn from_containers(name: String, containers: &[Container]) -> Self {
        let containers_total = containers.len();
        let containers_running = containers.iter().filter(|c| c.state.is_running()).count();
        let cpu_percent = containers.iter().map(|c| c.cpu.usage_percent).sum();
        let memory_bytes = containers.iter().map(|c| c.memory.used_bytes).sum();

        Self {
            name,
            containers_total,
            containers_running,
            cpu_percent,
            memory_bytes,
        }
    }
}
