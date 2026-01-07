use async_trait::async_trait;

use crate::domain::Process;

/// Port for fetching process information
#[async_trait]
pub trait ProcessSource: Send + Sync {
    /// List all processes
    async fn list_processes(&self) -> Result<Vec<Process>, Box<dyn std::error::Error>>;

    /// Get top N processes sorted by CPU usage
    async fn get_top_by_cpu(&self, n: usize) -> Result<Vec<Process>, Box<dyn std::error::Error>>;

    /// Get top N processes sorted by memory usage
    async fn get_top_by_memory(&self, n: usize) -> Result<Vec<Process>, Box<dyn std::error::Error>>;
}
