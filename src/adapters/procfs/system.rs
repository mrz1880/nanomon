use std::fs;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::{
    CpuMetrics, Disk, LoadAverage, MemoryMetrics, NetworkInterface, NetworkMetrics, Temperature,
    TemperatureSource,
};
use crate::ports::{HostInfo, SystemSource};

use super::parser::{self, CpuStat};
use super::ProcfsConfig;

// A std lock is "poisoned" when a thread panics while holding its guard;
// every later lock() then returns Err. The critical sections below only
// read or swap plain data and never panic, so poisoning cannot happen.
const LOCK_INVARIANT: &str = "last CPU stat lock poisoned: no code panics while holding it";

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
        if let Some(configured) = &self.config.hostname {
            return Ok(configured.clone());
        }
        let content = self.read_file("/etc/hostname")?;
        Ok(content.trim().to_string())
    }

    fn calculate_cpu_metrics(&self, current: &CpuStat, previous: Option<&CpuStat>) -> CpuMetrics {
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

        let user_delta =
            current.user.saturating_sub(prev.user) + current.nice.saturating_sub(prev.nice);
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

        let hostname = self
            .get_hostname()
            .unwrap_or_else(|_| "unknown".to_string());

        Ok(HostInfo {
            hostname,
            uptime_seconds,
        })
    }

    async fn get_cpu_metrics(
        &self,
    ) -> Result<CpuMetrics, Box<dyn std::error::Error + Send + Sync>> {
        let stat_path = self.config.proc_path.join("stat");
        let stat_content = fs::read_to_string(&stat_path)?;
        let current_stat = parser::parse_cpu_stat(&stat_content)?;

        let mut last_stat_lock = self.last_cpu_stat.lock().expect(LOCK_INVARIANT);
        let metrics = self.calculate_cpu_metrics(&current_stat, last_stat_lock.as_ref());
        *last_stat_lock = Some(current_stat);

        Ok(metrics)
    }

    async fn get_memory_metrics(
        &self,
    ) -> Result<MemoryMetrics, Box<dyn std::error::Error + Send + Sync>> {
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

    async fn get_load_average(
        &self,
    ) -> Result<LoadAverage, Box<dyn std::error::Error + Send + Sync>> {
        let loadavg_path = self.config.proc_path.join("loadavg");
        let loadavg_content = fs::read_to_string(&loadavg_path)?;
        let (one, five, fifteen) = parser::parse_loadavg(&loadavg_content)?;

        Ok(LoadAverage::new(one, five, fifteen))
    }

    // statvfs field types (fsblkcnt_t) vary by platform: the `as u64` casts are
    // redundant on Linux but required on macOS, so silence the Linux-only lint.
    #[allow(clippy::unnecessary_cast)]
    async fn list_disks(&self) -> Result<Vec<Disk>, Box<dyn std::error::Error + Send + Sync>> {
        let mounts_path = self.config.proc_path.join("mounts");
        let mounts_content = fs::read_to_string(&mounts_path)?;
        let mounts = parser::parse_mounts(&mounts_content)?;

        let mut disks = Vec::new();

        // Filter to only real filesystems and skip common virtual ones
        let skip_fs = [
            "proc",
            "sysfs",
            "tmpfs",
            "devtmpfs",
            "devpts",
            "cgroup",
            "cgroup2",
            "securityfs",
            "debugfs",
            "mqueue",
            "binfmt_misc",
            "pstore",
            "efivarfs",
            "bpf",
            "tracefs",
            "fuse",
        ];

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

                // Skip disks with zero capacity (virtual filesystems)
                if total_bytes == 0 {
                    continue;
                }

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

    async fn list_network_interfaces(
        &self,
    ) -> Result<Vec<NetworkInterface>, Box<dyn std::error::Error + Send + Sync>> {
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

            if let Ok((rx_bytes, tx_bytes, rx_errors, tx_errors)) =
                parser::parse_net_stats(&stats_dir)
            {
                interfaces.push(NetworkInterface::new(
                    interface_name,
                    is_up,
                    NetworkMetrics::new(rx_bytes, tx_bytes, rx_errors, tx_errors),
                ));
            }
        }

        Ok(interfaces)
    }

    async fn get_temperatures(
        &self,
    ) -> Result<Vec<Temperature>, Box<dyn std::error::Error + Send + Sync>> {
        let mut temps = Vec::new();

        // Try hwmon sensors first (more reliable on NAS)
        let hwmon_path = self.config.sys_path.join("class/hwmon");
        if let Ok(entries) = fs::read_dir(&hwmon_path) {
            for entry in entries.flatten() {
                let hwmon_dir = entry.path();
                let device_name = fs::read_to_string(hwmon_dir.join("name"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();

                // Scan temp*_input files
                for i in 1..=16 {
                    let input_path = hwmon_dir.join(format!("temp{}_input", i));
                    let millidegrees = match fs::read_to_string(&input_path) {
                        Ok(s) => match s.trim().parse::<i64>() {
                            Ok(v) => v,
                            Err(_) => continue,
                        },
                        Err(_) => break, // No more temp sensors in this hwmon
                    };

                    let label = fs::read_to_string(hwmon_dir.join(format!("temp{}_label", i)))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| {
                            if device_name.is_empty() {
                                format!("temp{}", i)
                            } else {
                                format!("{} temp{}", device_name, i)
                            }
                        });

                    let source = if device_name.contains("coretemp")
                        || device_name.contains("k10temp")
                        || device_name.contains("cpu")
                        || label.to_lowercase().contains("cpu")
                        || label.to_lowercase().contains("core")
                        || label.to_lowercase().contains("package")
                    {
                        TemperatureSource::Cpu
                    } else if device_name.contains("nvme")
                        || device_name.contains("drivetemp")
                        || label.to_lowercase().contains("disk")
                    {
                        TemperatureSource::Disk
                    } else {
                        TemperatureSource::Other
                    };

                    let high = fs::read_to_string(hwmon_dir.join(format!("temp{}_max", i)))
                        .ok()
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .map(|v| v as f64 / 1000.0);

                    let critical = fs::read_to_string(hwmon_dir.join(format!("temp{}_crit", i)))
                        .ok()
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .map(|v| v as f64 / 1000.0);

                    temps.push(
                        Temperature::new(label, source, millidegrees as f64 / 1000.0)
                            .with_thresholds(high, critical),
                    );
                }
            }
        }

        // Fallback: thermal zones (less detailed but widely available)
        if temps.is_empty() {
            let thermal_path = self.config.sys_path.join("class/thermal");
            if let Ok(entries) = fs::read_dir(&thermal_path) {
                for entry in entries.flatten() {
                    let zone_dir = entry.path();
                    let name = zone_dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if !name.starts_with("thermal_zone") {
                        continue;
                    }

                    let temp_millidegrees = match fs::read_to_string(zone_dir.join("temp")) {
                        Ok(s) => match s.trim().parse::<i64>() {
                            Ok(v) => v,
                            Err(_) => continue,
                        },
                        Err(_) => continue,
                    };

                    let zone_type = fs::read_to_string(zone_dir.join("type"))
                        .map(|s| s.trim().to_string())
                        .unwrap_or_else(|_| name.clone());

                    let source = if zone_type.contains("cpu")
                        || zone_type.contains("x86_pkg")
                        || zone_type.contains("acpitz")
                    {
                        TemperatureSource::Cpu
                    } else {
                        TemperatureSource::Other
                    };

                    temps.push(Temperature::new(
                        zone_type,
                        source,
                        temp_millidegrees as f64 / 1000.0,
                    ));
                }
            }
        }

        tracing::debug!("Found {} temperature sensors", temps.len());
        Ok(temps)
    }
}

// Need nix for statvfs
use nix;

#[cfg(test)]
mod tests {
    use super::{ProcfsConfig, ProcfsSystemSource};
    use crate::domain::TemperatureSource;
    use crate::ports::SystemSource;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    /// Throwaway fake /proc + /sys trees.
    struct FakeFs {
        root: PathBuf,
    }

    impl FakeFs {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("nanomon-sys-{}-{label}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let fake = Self { root };
            fake.write("proc/uptime", "12345.67 40000.00\n");
            fake.write("proc/loadavg", "0.52 0.78 1.21 2/456 12345\n");
            fake.write(
                "proc/meminfo",
                "MemTotal:       16384 kB\nMemAvailable:    4096 kB\nCached:  1024 kB\n\
                 Buffers:   512 kB\nSwapTotal:  2048 kB\nSwapFree:   1024 kB\n",
            );
            fake.write("proc/stat", "cpu  1000 100 500 10000 200 50 30 5\n");
            fake
        }

        fn write(&self, rel: &str, contents: &str) {
            let path = self.root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut f = fs::File::create(path).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
        }

        fn add_interface(&self, name: &str, up: bool, rx: u64, tx: u64) {
            let base = format!("sys/class/net/{name}");
            self.write(
                &format!("{base}/operstate"),
                if up { "up\n" } else { "down\n" },
            );
            self.write(&format!("{base}/statistics/rx_bytes"), &format!("{rx}\n"));
            self.write(&format!("{base}/statistics/tx_bytes"), &format!("{tx}\n"));
            self.write(&format!("{base}/statistics/rx_errors"), "1\n");
            self.write(&format!("{base}/statistics/tx_errors"), "2\n");
        }

        fn source(&self) -> ProcfsSystemSource {
            ProcfsSystemSource::new(ProcfsConfig::new(
                self.root.join("proc"),
                self.root.join("sys"),
            ))
        }
    }

    impl Drop for FakeFs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn host_info_reads_uptime() {
        let fake = FakeFs::new("hostinfo");
        let info = fake.source().get_host_info().await.unwrap();
        assert_eq!(info.uptime_seconds, 12345);
        // Hostname comes from the real /etc/hostname, falling back gracefully.
        assert!(!info.hostname.is_empty());
    }

    #[tokio::test]
    async fn an_explicit_hostname_wins_over_the_detected_one() {
        let fake = FakeFs::new("hostname");
        // A container cannot discover the host name: /etc/hostname is its own
        // and /proc/sys/kernel/hostname is namespaced, so it must be told.
        let source = ProcfsSystemSource::new(
            ProcfsConfig::new(fake.root.join("proc"), fake.root.join("sys"))
                .with_hostname(Some("mrznaz".to_string())),
        );

        let info = source.get_host_info().await.unwrap();

        assert_eq!(info.hostname, "mrznaz");
        // The rest of the reading is untouched.
        assert_eq!(info.uptime_seconds, 12345);
    }

    #[tokio::test]
    async fn host_info_errors_without_procfs() {
        let source =
            ProcfsSystemSource::new(ProcfsConfig::new("/nonexistent-nm", "/nonexistent-nm"));
        assert!(source.get_host_info().await.is_err());
    }

    #[tokio::test]
    async fn first_cpu_reading_is_zero_then_deltas_are_computed() {
        let fake = FakeFs::new("cpu");
        let source = fake.source();

        // No previous sample yet: the adapter cannot compute a delta.
        let first = source.get_cpu_metrics().await.unwrap();
        assert_eq!(first.usage_percent, 0.0);
        assert_eq!(first.iowait_percent, Some(0.0));

        // Advance the counters: busy +100 (user), idle +100 -> 50% usage.
        fake.write("proc/stat", "cpu  1100 100 500 10100 200 50 30 5\n");
        let second = source.get_cpu_metrics().await.unwrap();
        assert!((second.usage_percent - 50.0).abs() < 1e-6);
        assert!((second.user_percent - 50.0).abs() < 1e-6);
        assert_eq!(second.system_percent, 0.0);
    }

    #[tokio::test]
    async fn identical_cpu_samples_report_zero_usage() {
        let fake = FakeFs::new("cpu-idle");
        let source = fake.source();
        source.get_cpu_metrics().await.unwrap();
        // Same counters twice: no elapsed jiffies, so no usage.
        let second = source.get_cpu_metrics().await.unwrap();
        assert_eq!(second.usage_percent, 0.0);
    }

    #[tokio::test]
    async fn memory_metrics_derive_used_and_swap() {
        let fake = FakeFs::new("mem");
        let mem = fake.source().get_memory_metrics().await.unwrap();

        assert_eq!(mem.total_bytes, 16384 * 1024);
        assert_eq!(mem.available_bytes, 4096 * 1024);
        // used = total - available
        assert_eq!(mem.used_bytes, (16384 - 4096) * 1024);
        // cached = Cached + Buffers
        assert_eq!(mem.cached_bytes, Some((1024 + 512) * 1024));
        // swap used = SwapTotal - SwapFree
        assert_eq!(mem.swap_used_bytes, Some((2048 - 1024) * 1024));
    }

    #[tokio::test]
    async fn load_average_is_read_from_procfs() {
        let fake = FakeFs::new("load");
        let load = fake.source().get_load_average().await.unwrap();
        assert_eq!(load.one, 0.52);
        assert_eq!(load.five, 0.78);
        assert_eq!(load.fifteen, 1.21);
    }

    #[tokio::test]
    async fn network_interfaces_expose_counters_and_skip_loopback() {
        let fake = FakeFs::new("net");
        fake.add_interface("eth0", true, 1000, 2000);
        fake.add_interface("eth1", false, 5, 6);
        fake.add_interface("lo", true, 42, 42);

        let mut ifaces = fake.source().list_network_interfaces().await.unwrap();
        ifaces.sort_by(|a, b| a.name.cmp(&b.name));

        // Loopback carries no useful signal and is filtered out.
        assert_eq!(ifaces.len(), 2);
        assert_eq!(ifaces[0].name, "eth0");
        assert!(ifaces[0].is_up);
        assert_eq!(ifaces[0].metrics.rx_bytes, 1000);
        assert_eq!(ifaces[0].metrics.tx_bytes, 2000);
        assert_eq!(ifaces[0].metrics.rx_errors, 1);
        // Rates are only known once the collector has two samples.
        assert!(ifaces[0].metrics.rx_bytes_per_sec.is_none());
        assert!(!ifaces[1].is_up);
    }

    #[tokio::test]
    async fn interfaces_with_unreadable_statistics_are_skipped() {
        let fake = FakeFs::new("net-broken");
        fake.add_interface("eth0", true, 1, 2);
        // An interface directory without statistics (e.g. a virtual device).
        fs::create_dir_all(fake.root.join("sys/class/net/veth0")).unwrap();

        let ifaces = fake.source().list_network_interfaces().await.unwrap();
        assert_eq!(ifaces.len(), 1);
        assert_eq!(ifaces[0].name, "eth0");
    }

    #[tokio::test]
    async fn missing_net_class_is_an_error() {
        let fake = FakeFs::new("net-missing");
        assert!(fake.source().list_network_interfaces().await.is_err());
    }

    #[tokio::test]
    async fn temperatures_come_from_hwmon_with_labels_and_thresholds() {
        let fake = FakeFs::new("hwmon");
        fake.write("sys/class/hwmon/hwmon0/name", "coretemp\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_input", "54000\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_label", "Package id 0\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_max", "85000\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_crit", "100000\n");

        let temps = fake.source().get_temperatures().await.unwrap();
        assert_eq!(temps.len(), 1);
        assert_eq!(temps[0].label, "Package id 0");
        // Millidegrees are converted to degrees.
        assert_eq!(temps[0].current_celsius, 54.0);
        assert_eq!(temps[0].high_celsius, Some(85.0));
        assert_eq!(temps[0].critical_celsius, Some(100.0));
        assert!(matches!(temps[0].source, TemperatureSource::Cpu));
    }

    #[tokio::test]
    async fn hwmon_sensors_are_classified_by_device_and_label() {
        let fake = FakeFs::new("hwmon-class");
        fake.write("sys/class/hwmon/hwmon0/name", "nvme\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_input", "44000\n");
        fake.write("sys/class/hwmon/hwmon0/temp1_label", "Composite\n");
        fake.write("sys/class/hwmon/hwmon1/name", "acpitz\n");
        fake.write("sys/class/hwmon/hwmon1/temp1_input", "27800\n");

        let temps = fake.source().get_temperatures().await.unwrap();
        let disk = temps.iter().find(|t| t.label == "Composite").unwrap();
        assert!(matches!(disk.source, TemperatureSource::Disk));

        // No label: falls back to "<device> temp<n>", classified as other.
        let other = temps.iter().find(|t| t.label == "acpitz temp1").unwrap();
        assert!(matches!(other.source, TemperatureSource::Other));
        assert_eq!(other.current_celsius, 27.8);
        assert!(other.high_celsius.is_none());
    }

    #[tokio::test]
    async fn thermal_zones_are_used_when_no_hwmon_exists() {
        let fake = FakeFs::new("thermal");
        fake.write("sys/class/thermal/thermal_zone0/temp", "48000\n");
        fake.write("sys/class/thermal/thermal_zone0/type", "x86_pkg_temp\n");
        // Entries that are not thermal zones must be ignored.
        fake.write("sys/class/thermal/cooling_device0/type", "Processor\n");

        let temps = fake.source().get_temperatures().await.unwrap();
        assert_eq!(temps.len(), 1);
        assert_eq!(temps[0].label, "x86_pkg_temp");
        assert_eq!(temps[0].current_celsius, 48.0);
        assert!(matches!(temps[0].source, TemperatureSource::Cpu));
    }

    #[tokio::test]
    async fn absent_sensors_degrade_to_an_empty_list() {
        let fake = FakeFs::new("no-sensors");
        // Neither hwmon nor thermal exists: not an error, just nothing to show.
        assert!(fake.source().get_temperatures().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn disks_skip_virtual_filesystems() {
        let fake = FakeFs::new("disks");
        // "/" is a real mount point so statvfs succeeds; the virtual entries
        // must be filtered out by filesystem type.
        fake.write(
            "proc/mounts",
            "/dev/sda1 / ext4 rw 0 0\nproc /proc proc rw 0 0\ntmpfs /run tmpfs rw 0 0\n",
        );

        let disks = fake.source().list_disks().await.unwrap();
        assert_eq!(disks.len(), 1);
        assert_eq!(disks[0].mount_point, "/");
        assert_eq!(disks[0].filesystem, "ext4");
        assert!(disks[0].total_bytes > 0);
    }

    #[tokio::test]
    async fn unmountable_paths_are_skipped() {
        let fake = FakeFs::new("disks-missing");
        fake.write(
            "proc/mounts",
            "/dev/sdz /nonexistent-nanomon-mount ext4 rw 0 0\n",
        );
        assert!(fake.source().list_disks().await.unwrap().is_empty());
    }
}
