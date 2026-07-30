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
    pub fn new(pid: u32, ppid: u32, user: String, command: String, state: ProcessState) -> Self {
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

    pub fn with_metrics(
        mut self,
        cpu_percent: f64,
        memory_percent: f64,
        memory_bytes: u64,
    ) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{Process, ProcessState};
    use crate::domain::{ContainerId, MonitoredResource, ResourceType};

    fn process(state: ProcessState) -> Process {
        Process::new(42, 1, "root".to_string(), "nginx".to_string(), state)
    }

    #[test]
    fn state_maps_every_procfs_char() {
        assert_eq!(ProcessState::from_char('R'), ProcessState::Running);
        assert_eq!(ProcessState::from_char('S'), ProcessState::Sleeping);
        assert_eq!(ProcessState::from_char('D'), ProcessState::Waiting);
        assert_eq!(ProcessState::from_char('Z'), ProcessState::Zombie);
        assert_eq!(ProcessState::from_char('T'), ProcessState::Stopped);
        assert_eq!(ProcessState::from_char('t'), ProcessState::TracingStop);
        assert_eq!(ProcessState::from_char('X'), ProcessState::Dead);
        assert_eq!(ProcessState::from_char('x'), ProcessState::Dead);
        // Unknown characters must not be mistaken for a real state.
        assert_eq!(ProcessState::from_char('?'), ProcessState::Unknown);
        assert_eq!(ProcessState::from_char('r'), ProcessState::Unknown);
    }

    #[test]
    fn new_starts_with_zeroed_metrics_and_no_container() {
        let p = process(ProcessState::Running);
        assert_eq!(p.pid, 42);
        assert_eq!(p.ppid, 1);
        assert_eq!(p.user, "root");
        assert_eq!(p.cpu_percent, 0.0);
        assert_eq!(p.memory_percent, 0.0);
        assert_eq!(p.memory_bytes, 0);
        assert!(!p.is_containerized());
    }

    #[test]
    fn with_metrics_sets_each_field() {
        let p = process(ProcessState::Running).with_metrics(12.5, 3.5, 2048);
        assert_eq!(p.cpu_percent, 12.5);
        assert_eq!(p.memory_percent, 3.5);
        assert_eq!(p.memory_bytes, 2048);
    }

    #[test]
    fn with_container_marks_process_as_containerized() {
        let p = process(ProcessState::Running).with_container(Some(ContainerId::new("abc")));
        assert!(p.is_containerized());
        assert_eq!(p.container_id.as_ref().map(|c| c.as_str()), Some("abc"));

        assert!(!process(ProcessState::Running)
            .with_container(None)
            .is_containerized());
    }

    #[test]
    fn monitored_resource_exposes_process_metrics() {
        let p = process(ProcessState::Running).with_metrics(7.0, 1.0, 999);
        assert_eq!(p.resource_type(), ResourceType::Process);
        assert_eq!(p.name(), "nginx");
        assert_eq!(p.cpu_percent(), Some(7.0));
        assert_eq!(p.memory_bytes(), Some(999));
    }

    #[test]
    fn only_zombie_and_dead_are_unhealthy() {
        for state in [
            ProcessState::Running,
            ProcessState::Sleeping,
            ProcessState::Waiting,
            ProcessState::Stopped,
            ProcessState::TracingStop,
            ProcessState::Unknown,
        ] {
            assert!(process(state).is_healthy(), "{state:?} should be healthy");
        }
        assert!(!process(ProcessState::Zombie).is_healthy());
        assert!(!process(ProcessState::Dead).is_healthy());
    }
}
