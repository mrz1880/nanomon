use serde::{Deserialize, Serialize};

use super::NetworkMetrics;

/// Network interface entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub is_up: bool,
    pub metrics: NetworkMetrics,
}

impl NetworkInterface {
    pub fn new(name: String, is_up: bool, metrics: NetworkMetrics) -> Self {
        Self {
            name,
            is_up,
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NetworkInterface, NetworkMetrics};

    #[test]
    fn new_keeps_name_state_and_metrics() {
        let iface = NetworkInterface::new(
            "eth0".to_string(),
            true,
            NetworkMetrics::new(100, 200, 1, 2),
        );

        assert_eq!(iface.name, "eth0");
        assert!(iface.is_up);
        assert_eq!(iface.metrics.rx_bytes, 100);
        assert_eq!(iface.metrics.tx_bytes, 200);
        assert_eq!(iface.metrics.rx_errors, 1);
        assert_eq!(iface.metrics.tx_errors, 2);
    }

    #[test]
    fn down_interface_keeps_is_up_false() {
        let iface = NetworkInterface::new("eth1".to_string(), false, NetworkMetrics::zero());
        assert!(!iface.is_up);
        assert_eq!(iface.metrics.rx_bytes, 0);
    }
}
