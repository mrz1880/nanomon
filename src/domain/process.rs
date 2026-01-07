use serde::{Deserialize, Serialize};

use super::{ContainerId, MonitoredResource, ResourceType};

/// Process state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState {
    Running,
    Sleeping,
    Waiting,
    Zombie,
    Stopped,
    TracingStop,
    Dead,
    Unknown,
}

impl ProcessState {
    pub fn from_char(c: char) -> Self {
        match c {
            'R' => Self::Running,
            'S' => Self::Sleeping,
            'D' => Self::Waiting,
            'Z' => Self::Zombie,
            'T' => Self::Stopped,
            't' => Self::TracingStop,
            'X' | 'x' => Self::Dead,
            _ => Self::Unknown,
        }
    }
}

/// Process entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub user: String,
    pub command: String,
    pub state: ProcessState,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<ContainerId>,
}

impl Process {
    pub fn new(
        pid: u32,
        ppid: u32,
        user: String,
        command: String,
        state: ProcessState,
    ) -> Self {
        Self {
            pid,
            ppid,
            user,
            command,
            state,
            cpu_percent: 0.0,
            memory_percent: 0.0,
            memory_bytes: 0,
            container_id: None,
        }
    }

    pub fn with_metrics(mut self, cpu_percent: f64, memory_percent: f64, memory_bytes: u64) -> Self {
        self.cpu_percent = cpu_percent;
        self.memory_percent = memory_percent;
        self.memory_bytes = memory_bytes;
        self
    }

    pub fn with_container(mut self, container_id: Option<ContainerId>) -> Self {
        self.container_id = container_id;
        self
    }

    #[allow(dead_code)]
    pub fn is_containerized(&self) -> bool {
        self.container_id.is_some()
    }
}

impl MonitoredResource for Process {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Process
    }

    fn name(&self) -> &str {
        &self.command
    }

    fn cpu_percent(&self) -> Option<f64> {
        Some(self.cpu_percent)
    }

    fn memory_bytes(&self) -> Option<u64> {
        Some(self.memory_bytes)
    }

    fn is_healthy(&self) -> bool {
        !matches!(self.state, ProcessState::Zombie | ProcessState::Dead)
    }
}
