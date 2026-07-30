use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    CpuMetrics, IoMetrics, MemoryMetrics, MonitoredResource, NetworkMetrics, ResourceType,
};

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

#[cfg(test)]
mod tests {
    use super::{Container, ContainerId, ContainerState, Stack};
    use crate::domain::{
        CpuMetrics, IoMetrics, MemoryMetrics, MonitoredResource, NetworkMetrics, ResourceType,
    };
    use chrono::Utc;

    fn container(name: &str, state: ContainerState, cpu: f64, mem: u64) -> Container {
        Container::new(
            ContainerId::new(name),
            name.to_string(),
            "img:latest".to_string(),
            state,
            Utc::now(),
        )
        .with_metrics(
            CpuMetrics::new(cpu, 0.0, 0.0),
            MemoryMetrics::new(mem, 0, 0),
            NetworkMetrics::zero(),
            IoMetrics::zero(),
        )
    }

    #[test]
    fn container_id_roundtrips_from_str_and_string() {
        assert_eq!(ContainerId::new("abc").as_str(), "abc");
        assert_eq!(ContainerId::from("def").as_str(), "def");
        assert_eq!(ContainerId::from("ghi".to_string()).as_str(), "ghi");
    }

    #[test]
    fn only_running_state_is_running() {
        assert!(ContainerState::Running.is_running());
        for state in [
            ContainerState::Stopped,
            ContainerState::Paused,
            ContainerState::Restarting,
            ContainerState::Dead,
            ContainerState::Created,
        ] {
            assert!(!state.is_running(), "{state:?} must not count as running");
        }
    }

    #[test]
    fn new_container_has_no_stack_and_zero_metrics() {
        let c = Container::new(
            ContainerId::new("id"),
            "web".to_string(),
            "nginx".to_string(),
            ContainerState::Running,
            Utc::now(),
        );
        assert!(c.stack.is_none());
        assert_eq!(c.cpu.usage_percent, 0.0);
        assert_eq!(c.memory.used_bytes, 0);
        assert_eq!(c.network.rx_bytes, 0);
        assert_eq!(c.block_io.read_bytes, 0);
    }

    #[test]
    fn with_stack_and_with_metrics_set_fields() {
        let c = container("web", ContainerState::Running, 12.0, 4096)
            .with_stack(Some("mystack".to_string()));
        assert_eq!(c.stack.as_deref(), Some("mystack"));
        assert_eq!(c.cpu.usage_percent, 12.0);
        assert_eq!(c.memory.used_bytes, 4096);

        assert!(container("x", ContainerState::Running, 0.0, 0)
            .with_stack(None)
            .stack
            .is_none());
    }

    #[test]
    fn monitored_resource_reflects_container_state() {
        let running = container("web", ContainerState::Running, 5.0, 2048);
        assert_eq!(running.resource_type(), ResourceType::Container);
        assert_eq!(running.name(), "web");
        assert_eq!(running.cpu_percent(), Some(5.0));
        assert_eq!(running.memory_bytes(), Some(2048));
        assert!(running.is_healthy());

        // Anything not running is unhealthy.
        assert!(!container("web", ContainerState::Stopped, 0.0, 0).is_healthy());
        assert!(!container("web", ContainerState::Dead, 0.0, 0).is_healthy());
    }

    #[test]
    fn stack_aggregates_totals_and_running_count() {
        let containers = vec![
            container("a", ContainerState::Running, 10.0, 100),
            container("b", ContainerState::Running, 5.5, 200),
            container("c", ContainerState::Stopped, 0.0, 50),
        ];

        let stack = Stack::from_containers("mystack".to_string(), &containers);

        assert_eq!(stack.name, "mystack");
        assert_eq!(stack.containers_total, 3);
        assert_eq!(stack.containers_running, 2);
        assert_eq!(stack.cpu_percent, 15.5);
        // Stopped containers still contribute their reported memory.
        assert_eq!(stack.memory_bytes, 350);
    }

    #[test]
    fn stack_from_no_containers_is_all_zero() {
        let stack = Stack::from_containers("empty".to_string(), &[]);
        assert_eq!(stack.containers_total, 0);
        assert_eq!(stack.containers_running, 0);
        assert_eq!(stack.cpu_percent, 0.0);
        assert_eq!(stack.memory_bytes, 0);
    }
}
