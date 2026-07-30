use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    Container, CpuMetrics, Disk, ExternalMetric, LoadAverage, MemoryMetrics, MonitoredResource,
    NetworkInterface, Process, ResourceType, SystemdService, Temperature,
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
    pub temperatures: Vec<Temperature>,
    #[serde(default)]
    pub services: Vec<SystemdService>,
    #[serde(default)]
    pub external: Vec<ExternalMetric>,
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
            temperatures: Vec::new(),
            services: Vec::new(),
            external: Vec::new(),
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

    pub fn with_temperatures(mut self, temperatures: Vec<Temperature>) -> Self {
        self.temperatures = temperatures;
        self
    }

    pub fn with_services(mut self, services: Vec<SystemdService>) -> Self {
        self.services = services;
        self
    }

    pub fn with_external(mut self, external: Vec<ExternalMetric>) -> Self {
        self.external = external;
        self
    }

    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Get total network I/O across all interfaces
    #[allow(dead_code)]
    pub fn total_network_rx_bytes(&self) -> u64 {
        self.network_interfaces
            .iter()
            .map(|i| i.metrics.rx_bytes)
            .sum()
    }

    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::Host;
    use crate::domain::{
        CpuMetrics, Disk, ExternalMetric, LoadAverage, MemoryMetrics, MonitoredResource,
        NetworkInterface, NetworkMetrics, Process, ProcessState, ResourceType, ServiceState,
        SystemdService, Temperature, TemperatureSource,
    };
    use chrono::{TimeZone, Utc};

    fn iface(name: &str, rx: u64, tx: u64) -> NetworkInterface {
        NetworkInterface::new(name.to_string(), true, NetworkMetrics::new(rx, tx, 0, 0))
    }

    #[test]
    fn new_starts_empty() {
        let host = Host::new("nas".to_string());
        assert_eq!(host.hostname, "nas");
        assert_eq!(host.uptime_seconds, 0);
        assert_eq!(host.cpu.usage_percent, 0.0);
        assert!(host.network_interfaces.is_empty());
        assert!(host.disks.is_empty());
        assert!(host.containers.is_empty());
        assert!(host.processes.is_empty());
        assert!(host.temperatures.is_empty());
        assert!(host.services.is_empty());
        assert!(host.external.is_empty());
    }

    #[test]
    fn with_metrics_sets_uptime_load_cpu_and_memory() {
        let host = Host::new("nas".to_string()).with_metrics(
            3600,
            LoadAverage::new(1.0, 2.0, 3.0),
            CpuMetrics::new(25.0, 10.0, 15.0),
            MemoryMetrics::new(512, 1024, 512),
        );

        assert_eq!(host.uptime_seconds, 3600);
        assert_eq!(host.load_average.one, 1.0);
        assert_eq!(host.load_average.five, 2.0);
        assert_eq!(host.load_average.fifteen, 3.0);
        assert_eq!(host.cpu.usage_percent, 25.0);
        assert_eq!(host.memory.total_bytes, 1024);
    }

    #[test]
    fn builders_attach_each_collection() {
        let host = Host::new("nas".to_string())
            .with_network_interfaces(vec![iface("eth0", 1, 2)])
            .with_disks(vec![Disk::new(
                "/dev/sda".to_string(),
                "/".to_string(),
                "ext4".to_string(),
                100,
                50,
                50,
            )])
            .with_processes(vec![Process::new(
                1,
                0,
                "root".to_string(),
                "init".to_string(),
                ProcessState::Running,
            )])
            .with_temperatures(vec![Temperature::new(
                "cpu".to_string(),
                TemperatureSource::Cpu,
                50.0,
            )])
            .with_services(vec![SystemdService::new(
                "nginx".to_string(),
                String::new(),
                ServiceState::Active,
                "running".to_string(),
            )])
            .with_external(vec![ExternalMetric {
                job: "app".to_string(),
                name: "queue".to_string(),
                value: 3.0,
            }]);

        assert_eq!(host.network_interfaces.len(), 1);
        assert_eq!(host.disks.len(), 1);
        assert_eq!(host.processes.len(), 1);
        assert_eq!(host.temperatures.len(), 1);
        assert_eq!(host.services.len(), 1);
        assert_eq!(host.external.len(), 1);
        assert_eq!(host.external[0].name, "queue");
    }

    #[test]
    fn with_timestamp_overrides_creation_time() {
        let when = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let host = Host::new("nas".to_string()).with_timestamp(when);
        assert_eq!(host.timestamp, when);
    }

    #[test]
    fn network_totals_sum_across_interfaces() {
        let host = Host::new("nas".to_string())
            .with_network_interfaces(vec![iface("eth0", 100, 10), iface("eth1", 5, 1)]);

        assert_eq!(host.total_network_rx_bytes(), 105);
        assert_eq!(host.total_network_tx_bytes(), 11);
    }

    #[test]
    fn network_totals_are_zero_without_interfaces() {
        let host = Host::new("nas".to_string());
        assert_eq!(host.total_network_rx_bytes(), 0);
        assert_eq!(host.total_network_tx_bytes(), 0);
    }

    #[test]
    fn monitored_resource_exposes_host_metrics() {
        let host = Host::new("nas".to_string()).with_metrics(
            1,
            LoadAverage::zero(),
            CpuMetrics::new(42.0, 0.0, 0.0),
            MemoryMetrics::new(700, 1000, 300),
        );

        assert_eq!(host.resource_type(), ResourceType::Host);
        assert_eq!(host.name(), "nas");
        assert_eq!(host.cpu_percent(), Some(42.0));
        assert_eq!(host.memory_bytes(), Some(700));
        // A reachable host is always reported healthy.
        assert!(host.is_healthy());
    }
}
