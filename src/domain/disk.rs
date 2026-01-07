use serde::{Deserialize, Serialize};

/// Disk entity (mount point with usage information)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disk {
    pub device: String,
    pub mount_point: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

impl Disk {
    pub fn new(
        device: String,
        mount_point: String,
        filesystem: String,
        total_bytes: u64,
        used_bytes: u64,
        available_bytes: u64,
    ) -> Self {
        Self {
            device,
            mount_point,
            filesystem,
            total_bytes,
            used_bytes,
            available_bytes,
        }
    }

    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}
