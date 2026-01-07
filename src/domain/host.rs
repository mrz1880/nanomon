use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    Container, CpuMetrics, Disk, LoadAverage, MemoryMetrics, MonitoredResource, NetworkInterface,
    Process, ResourceType,
};

/// Host aggregate root
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub hostname: String,
    pub uptime_seconds: u64,
    pub load_average: LoadAverage,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub network_interfaces: Vec<NetworkInterface>,
    pub disks: Vec<Disk>,
    pub containers: Vec<Container>,
    pub processes: Vec<Process>,
    pub timestamp: DateTime<Utc>,
}

impl Host {
    pub fn new(hostname: String) -> Self {
        Self {
            hostname,
            uptime_seconds: 0,
            load_average: LoadAverage::zero(),
            cpu: CpuMetrics::new(0.0, 0.0, 0.0),
            memory: MemoryMetrics::new(0, 0, 0),
            network_interfaces: Vec::new(),
            disks: Vec::new(),
            containers: Vec::new(),
            processes: Vec::new(),
            timestamp: Utc::now(),
        }
    }

    pub fn with_metrics(
        mut self,
        uptime_seconds: u64,
        load_average: LoadAverage,
        cpu: CpuMetrics,
        memory: MemoryMetrics,
    ) -> Self {
        self.uptime_seconds = uptime_seconds;
        self.load_average = load_average;
        self.cpu = cpu;
        self.memory = memory;
        self
    }

    pub fn with_network_interfaces(mut self, interfaces: Vec<NetworkInterface>) -> Self {
        self.network_interfaces = interfaces;
        self
    }

    pub fn with_disks(mut self, disks: Vec<Disk>) -> Self {
        self.disks = disks;
        self
    }

    pub fn with_containers(mut self, containers: Vec<Container>) -> Self {
        self.containers = containers;
        self
    }

    pub fn with_processes(mut self, processes: Vec<Process>) -> Self {
        self.processes = processes;
        self
    }

    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Get total network I/O across all interfaces
    pub fn total_network_rx_bytes(&self) -> u64 {
        self.network_interfaces
            .iter()
            .map(|i| i.metrics.rx_bytes)
            .sum()
    }

    pub fn total_network_tx_bytes(&self) -> u64 {
        self.network_interfaces
            .iter()
            .map(|i| i.metrics.tx_bytes)
            .sum()
    }
}

impl MonitoredResource for Host {
    fn resource_type(&self) -> ResourceType {
        ResourceType::Host
    }

    fn name(&self) -> &str {
        &self.hostname
    }

    fn cpu_percent(&self) -> Option<f64> {
        Some(self.cpu.usage_percent)
    }

    fn memory_bytes(&self) -> Option<u64> {
        Some(self.memory.used_bytes)
    }

    fn is_healthy(&self) -> bool {
        true
    }
}
