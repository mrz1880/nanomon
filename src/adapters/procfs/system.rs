use std::fs;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::{CpuMetrics, Disk, LoadAverage, MemoryMetrics, NetworkInterface, NetworkMetrics};
use crate::ports::{HostInfo, SystemSource};

use super::parser::{self, CpuStat};
use super::ProcfsConfig;

/// System source implementation using procfs
pub struct ProcfsSystemSource {
    config: ProcfsConfig,
    last_cpu_stat: Mutex<Option<CpuStat>>,
}

impl ProcfsSystemSource {
    pub fn new(config: ProcfsConfig) -> Self {
        Self {
            config,
            last_cpu_stat: Mutex::new(None),
        }
    }

    fn read_file(&self, path: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(fs::read_to_string(path)?)
    }

    fn get_hostname(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let content = self.read_file("/etc/hostname")?;
        Ok(content.trim().to_string())
    }

    fn calculate_cpu_metrics(
        &self,
        current: &CpuStat,
        previous: Option<&CpuStat>,
    ) -> CpuMetrics {
        let prev = match previous {
            Some(p) => p,
            None => {
                // First call, return zeros
                return CpuMetrics::new(0.0, 0.0, 0.0).with_iowait(0.0);
            }
        };

        let total_delta = current.total().saturating_sub(prev.total());
        if total_delta == 0 {
            return CpuMetrics::new(0.0, 0.0, 0.0).with_iowait(0.0);
        }

        let user_delta = current.user.saturating_sub(prev.user) + current.nice.saturating_sub(prev.nice);
        let system_delta = current.system.saturating_sub(prev.system)
            + current.irq.saturating_sub(prev.irq)
            + current.softirq.saturating_sub(prev.softirq);
        let iowait_delta = current.iowait.saturating_sub(prev.iowait);
        let busy_delta = current.busy().saturating_sub(prev.busy());

        let user_percent = (user_delta as f64 / total_delta as f64) * 100.0;
        let system_percent = (system_delta as f64 / total_delta as f64) * 100.0;
        let iowait_percent = (iowait_delta as f64 / total_delta as f64) * 100.0;
        let usage_percent = (busy_delta as f64 / total_delta as f64) * 100.0;

        CpuMetrics::new(usage_percent, user_percent, system_percent).with_iowait(iowait_percent)
    }
}

#[async_trait]
impl SystemSource for ProcfsSystemSource {
    async fn get_host_info(&self) -> Result<HostInfo, Box<dyn std::error::Error + Send + Sync>> {
        let uptime_path = self.config.proc_path.join("uptime");
        let uptime_content = fs::read_to_string(&uptime_path)?;
        let uptime_seconds = parser::parse_uptime(&uptime_content)?;

        let hostname = self.get_hostname().unwrap_or_else(|_| "unknown".to_string());

        Ok(HostInfo {
            hostname,
            uptime_seconds,
        })
    }

    async fn get_cpu_metrics(&self) -> Result<CpuMetrics, Box<dyn std::error::Error + Send + Sync>> {
        let stat_path = self.config.proc_path.join("stat");
        let stat_content = fs::read_to_string(&stat_path)?;
        let current_stat = parser::parse_cpu_stat(&stat_content)?;

        let mut last_stat_lock = self.last_cpu_stat.lock().unwrap();
        let metrics = self.calculate_cpu_metrics(&current_stat, last_stat_lock.as_ref());
        *last_stat_lock = Some(current_stat);

        Ok(metrics)
    }

    async fn get_memory_metrics(&self) -> Result<MemoryMetrics, Box<dyn std::error::Error + Send + Sync>> {
        let meminfo_path = self.config.proc_path.join("meminfo");
        let meminfo_content = fs::read_to_string(&meminfo_path)?;
        let meminfo = parser::parse_meminfo(&meminfo_content)?;

        let total = *meminfo.get("MemTotal").unwrap_or(&0);
        let available = *meminfo.get("MemAvailable").unwrap_or(&0);
        let cached = *meminfo.get("Cached").unwrap_or(&0);
        let buffers = *meminfo.get("Buffers").unwrap_or(&0);
        let swap_total = *meminfo.get("SwapTotal").unwrap_or(&0);
        let swap_free = *meminfo.get("SwapFree").unwrap_or(&0);

        let used = total.saturating_sub(available);
        let swap_used = swap_total.saturating_sub(swap_free);

        Ok(MemoryMetrics::new(used, total, available)
            .with_cache(cached + buffers)
            .with_swap(swap_used))
    }

    async fn get_load_average(&self) -> Result<LoadAverage, Box<dyn std::error::Error + Send + Sync>> {
        let loadavg_path = self.config.proc_path.join("loadavg");
        let loadavg_content = fs::read_to_string(&loadavg_path)?;
        let (one, five, fifteen) = parser::parse_loadavg(&loadavg_content)?;

        Ok(LoadAverage::new(one, five, fifteen))
    }

    async fn list_disks(&self) -> Result<Vec<Disk>, Box<dyn std::error::Error + Send + Sync>> {
        let mounts_path = self.config.proc_path.join("mounts");
        let mounts_content = fs::read_to_string(&mounts_path)?;
        let mounts = parser::parse_mounts(&mounts_content)?;

        let mut disks = Vec::new();

        // Filter to only real filesystems and skip common virtual ones
        let skip_fs = ["proc", "sysfs", "tmpfs", "devtmpfs", "devpts", "cgroup", "cgroup2", "securityfs", "debugfs"];

        for mount in mounts {
            if skip_fs.contains(&mount.filesystem.as_str()) {
                continue;
            }

            // Try to get disk stats using statvfs
            if let Ok(stat) = nix::sys::statvfs::statvfs(mount.mount_point.as_str()) {
                let block_size = stat.block_size();
                let total_bytes = stat.blocks() as u64 * block_size;
                let available_bytes = stat.blocks_available() as u64 * block_size;
                let free_bytes = stat.blocks_free() as u64 * block_size;
                let used_bytes = total_bytes.saturating_sub(free_bytes);

                disks.push(Disk::new(
                    mount.device.clone(),
                    mount.mount_point.clone(),
                    mount.filesystem.clone(),
                    total_bytes,
                    used_bytes,
                    available_bytes,
                ));
            }
        }

        Ok(disks)
    }

    async fn list_network_interfaces(&self) -> Result<Vec<NetworkInterface>, Box<dyn std::error::Error + Send + Sync>> {
        let net_class_path = self.config.sys_path.join("class/net");
        let mut interfaces = Vec::new();

        let entries = fs::read_dir(&net_class_path)?;

        for entry in entries {
            let entry = entry?;
            let interface_name = entry.file_name().to_string_lossy().to_string();

            // Skip loopback
            if interface_name == "lo" {
                continue;
            }

            let stats_dir = entry.path().join("statistics");
            let operstate_path = entry.path().join("operstate");

            let is_up = fs::read_to_string(&operstate_path)
                .map(|s| s.trim() == "up")
                .unwrap_or(false);

            if let Ok((rx_bytes, tx_bytes, rx_errors, tx_errors)) = parser::parse_net_stats(&stats_dir) {
                interfaces.push(NetworkInterface::new(
                    interface_name,
                    is_up,
                    NetworkMetrics::new(rx_bytes, tx_bytes, rx_errors, tx_errors),
                ));
            }
        }

        Ok(interfaces)
    }
}

// Need nix for statvfs
use nix;
