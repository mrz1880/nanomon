use serde::{Deserialize, Serialize};

/// CPU metrics for a host or container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f64,
    pub user_percent: f64,
    pub system_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iowait_percent: Option<f64>, // host only
}

impl CpuMetrics {
    pub fn new(usage_percent: f64, user_percent: f64, system_percent: f64) -> Self {
        Self {
            usage_percent,
            user_percent,
            system_percent,
            iowait_percent: None,
        }
    }

    pub fn with_iowait(mut self, iowait_percent: f64) -> Self {
        self.iowait_percent = Some(iowait_percent);
        self
    }
}

/// Memory metrics for a host or container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_bytes: Option<u64>, // host only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_used_bytes: Option<u64>, // host only
}

impl MemoryMetrics {
    pub fn new(used_bytes: u64, total_bytes: u64, available_bytes: u64) -> Self {
        Self {
            used_bytes,
            total_bytes,
            available_bytes,
            cached_bytes: None,
            swap_used_bytes: None,
        }
    }

    pub fn with_cache(mut self, cached_bytes: u64) -> Self {
        self.cached_bytes = Some(cached_bytes);
        self
    }

    pub fn with_swap(mut self, swap_used_bytes: u64) -> Self {
        self.swap_used_bytes = Some(swap_used_bytes);
        self
    }

    #[allow(dead_code)]
    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}

/// I/O metrics (disk or block device)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoMetrics {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

impl IoMetrics {
    pub fn new(read_bytes: u64, write_bytes: u64) -> Self {
        Self {
            read_bytes,
            write_bytes,
        }
    }

    pub fn zero() -> Self {
        Self {
            read_bytes: 0,
            write_bytes: 0,
        }
    }
}

/// Network metrics (interface or container)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    /// Receive throughput in bytes/second (None until a delta is available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_bytes_per_sec: Option<f64>,
    /// Transmit throughput in bytes/second (None until a delta is available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_bytes_per_sec: Option<f64>,
}

impl NetworkMetrics {
    pub fn new(rx_bytes: u64, tx_bytes: u64, rx_errors: u64, tx_errors: u64) -> Self {
        Self {
            rx_bytes,
            tx_bytes,
            rx_errors,
            tx_errors,
            rx_bytes_per_sec: None,
            tx_bytes_per_sec: None,
        }
    }

    pub fn zero() -> Self {
        Self {
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_bytes_per_sec: None,
            tx_bytes_per_sec: None,
        }
    }
}

/// System load average (1, 5, 15 minutes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

impl LoadAverage {
    pub fn new(one: f64, five: f64, fifteen: f64) -> Self {
        Self { one, five, fifteen }
    }

    pub fn zero() -> Self {
        Self {
            one: 0.0,
            five: 0.0,
            fifteen: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuMetrics, IoMetrics, LoadAverage, MemoryMetrics, NetworkMetrics};

    #[test]
    fn cpu_metrics_iowait_is_host_only() {
        let cpu = CpuMetrics::new(30.0, 20.0, 10.0);
        assert_eq!(cpu.usage_percent, 30.0);
        assert_eq!(cpu.user_percent, 20.0);
        assert_eq!(cpu.system_percent, 10.0);
        assert!(cpu.iowait_percent.is_none());

        let cpu = cpu.with_iowait(2.5);
        assert_eq!(cpu.iowait_percent, Some(2.5));
    }

    #[test]
    fn memory_usage_percent_is_used_over_total() {
        assert_eq!(MemoryMetrics::new(512, 1024, 512).usage_percent(), 50.0);
        assert_eq!(MemoryMetrics::new(0, 1024, 1024).usage_percent(), 0.0);
    }

    #[test]
    fn memory_usage_percent_guards_zero_total() {
        let percent = MemoryMetrics::new(0, 0, 0).usage_percent();
        assert_eq!(percent, 0.0);
        assert!(percent.is_finite());
    }

    #[test]
    fn memory_optional_fields_are_host_only() {
        let mem = MemoryMetrics::new(1, 2, 1);
        assert!(mem.cached_bytes.is_none());
        assert!(mem.swap_used_bytes.is_none());

        let mem = mem.with_cache(64).with_swap(32);
        assert_eq!(mem.cached_bytes, Some(64));
        assert_eq!(mem.swap_used_bytes, Some(32));
    }

    #[test]
    fn zero_constructors_start_at_zero() {
        let io = IoMetrics::zero();
        assert_eq!(io.read_bytes, 0);
        assert_eq!(io.write_bytes, 0);

        let io = IoMetrics::new(10, 20);
        assert_eq!(io.read_bytes, 10);
        assert_eq!(io.write_bytes, 20);

        let load = LoadAverage::zero();
        assert_eq!(load.one, 0.0);
        assert_eq!(load.five, 0.0);
        assert_eq!(load.fifteen, 0.0);

        let net = NetworkMetrics::zero();
        assert_eq!(net.rx_bytes, 0);
        assert_eq!(net.tx_bytes, 0);
        assert_eq!(net.rx_errors, 0);
        assert_eq!(net.tx_errors, 0);
    }

    #[test]
    fn load_average_keeps_each_window() {
        let load = LoadAverage::new(1.5, 2.5, 3.5);
        assert_eq!(load.one, 1.5);
        assert_eq!(load.five, 2.5);
        assert_eq!(load.fifteen, 3.5);
    }

    #[test]
    fn network_rates_default_to_none_and_are_omitted_from_json() {
        let metrics = NetworkMetrics::new(10, 20, 0, 0);
        assert!(metrics.rx_bytes_per_sec.is_none());
        assert!(metrics.tx_bytes_per_sec.is_none());

        // Rates are skipped in JSON until a delta is available.
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(!json.contains("rx_bytes_per_sec"));
        assert!(!json.contains("tx_bytes_per_sec"));
    }
}
