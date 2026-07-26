use async_trait::async_trait;

use crate::domain::DirectoryUsage;

/// Port for computing disk usage of configured directories.
#[async_trait]
pub trait DirectoryUsageSource: Send + Sync {
    /// Compute usage for all watched directories (bounded, cache-friendly).
    async fn directory_usage(&self) -> Vec<DirectoryUsage>;
}
