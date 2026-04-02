use serde::{Deserialize, Serialize};

/// A rule defining when an alert should fire
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub metric: AlertMetric,
    pub condition: AlertCondition,
    pub threshold: f64,
    pub webhook_url: String,
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: u64,
}

fn default_cooldown() -> u64 {
    300 // 5 minutes
}

/// Which metric to evaluate
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertMetric {
    CpuUsage,
    MemoryUsage,
    DiskUsage { mount_point: String },
    LoadAverage1m,
    Temperature { label: String },
}

/// Comparison condition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertCondition {
    Above,
    Below,
}

impl AlertCondition {
    pub fn evaluate(&self, current: f64, threshold: f64) -> bool {
        match self {
            Self::Above => current > threshold,
            Self::Below => current < threshold,
        }
    }
}

/// An alert event fired when a rule triggers
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub rule_name: String,
    pub metric: String,
    pub current_value: f64,
    pub threshold: f64,
    pub condition: String,
    pub hostname: String,
    pub timestamp: String,
}
