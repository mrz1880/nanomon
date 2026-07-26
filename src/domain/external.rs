use serde::{Deserialize, Serialize};

/// A single metric ingested from an external scrape target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMetric {
    /// Logical source name (the scrape "job").
    pub job: String,
    /// Metric name as reported by the target.
    pub name: String,
    pub value: f64,
}
