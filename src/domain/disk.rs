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

    #[allow(dead_code)]
    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::Disk;

    fn disk(used: u64, total: u64) -> Disk {
        Disk::new(
            "/dev/sda1".to_string(),
            "/".to_string(),
            "ext4".to_string(),
            total,
            used,
            total.saturating_sub(used),
        )
    }

    #[test]
    fn new_keeps_every_field() {
        let d = disk(250, 1000);
        assert_eq!(d.device, "/dev/sda1");
        assert_eq!(d.mount_point, "/");
        assert_eq!(d.filesystem, "ext4");
        assert_eq!(d.total_bytes, 1000);
        assert_eq!(d.used_bytes, 250);
        assert_eq!(d.available_bytes, 750);
    }

    #[test]
    fn usage_percent_is_used_over_total() {
        assert_eq!(disk(250, 1000).usage_percent(), 25.0);
        assert_eq!(disk(1000, 1000).usage_percent(), 100.0);
        assert_eq!(disk(0, 1000).usage_percent(), 0.0);
    }

    #[test]
    fn zero_capacity_reports_zero_not_nan() {
        // Virtual filesystems report a zero total; must not divide by zero.
        let percent = disk(0, 0).usage_percent();
        assert_eq!(percent, 0.0);
        assert!(percent.is_finite());
    }
}
