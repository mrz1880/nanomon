use serde::{Deserialize, Serialize};

/// Type of monitored resource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceType {
    Host,
    Container,
    Process,
}

/// Common trait for all monitored resources
#[allow(dead_code)]
pub trait MonitoredResource {
    fn resource_type(&self) -> ResourceType;
    fn name(&self) -> &str;
    fn cpu_percent(&self) -> Option<f64>;
    fn memory_bytes(&self) -> Option<u64>;
    fn is_healthy(&self) -> bool;
}
