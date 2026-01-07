use async_trait::async_trait;

use crate::domain::{CpuMetrics, Disk, LoadAverage, MemoryMetrics, NetworkInterface};

/// Host information
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub hostname: String,
    pub uptime_seconds: u64,
}

/// Port for fetching system-level information
#[async_trait]
pub trait SystemSource: Send + Sync {
    /// Get basic host information (hostname, uptime)
    async fn get_host_info(&self) -> Result<HostInfo, Box<dyn std::error::Error + Send + Sync>>;

    /// Get CPU metrics for the host
    async fn get_cpu_metrics(&self) -> Result<CpuMetrics, Box<dyn std::error::Error + Send + Sync>>;

    /// Get memory metrics for the host
    async fn get_memory_metrics(&self) -> Result<MemoryMetrics, Box<dyn std::error::Error + Send + Sync>>;

    /// Get system load average
    async fn get_load_average(&self) -> Result<LoadAverage, Box<dyn std::error::Error + Send + Sync>>;

    /// List all mounted disks
    async fn list_disks(&self) -> Result<Vec<Disk>, Box<dyn std::error::Error + Send + Sync>>;

    /// List all network interfaces with statistics
    async fn list_network_interfaces(&self) -> Result<Vec<NetworkInterface>, Box<dyn std::error::Error + Send + Sync>>;
}
