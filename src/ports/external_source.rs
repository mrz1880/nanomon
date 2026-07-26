use async_trait::async_trait;

use crate::domain::ExternalMetric;

/// Port for ingesting metrics from external endpoints (Prometheus text / JSON).
#[async_trait]
pub trait ExternalMetricSource: Send + Sync {
    /// Scrape all configured targets and return the collected metrics.
    async fn scrape(&self) -> Vec<ExternalMetric>;
}
