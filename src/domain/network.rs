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
