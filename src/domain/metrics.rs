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
}

impl NetworkMetrics {
    pub fn new(rx_bytes: u64, tx_bytes: u64, rx_errors: u64, tx_errors: u64) -> Self {
        Self {
            rx_bytes,
            tx_bytes,
            rx_errors,
            tx_errors,
        }
    }

    pub fn zero() -> Self {
        Self {
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
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
