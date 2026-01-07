use std::time::Duration;

use crate::domain::Host;

/// Port for storing and retrieving host snapshots
#[allow(dead_code)]
pub trait MetricStore: Send + Sync {
    /// Store a new host snapshot
    fn store(&mut self, snapshot: Host);

    /// Get the most recent snapshot
    fn get_latest(&self) -> Option<&Host>;

    /// Get all snapshots within a time window
    fn get_history(&self, duration: Duration) -> Vec<&Host>;

    /// Get the number of stored snapshots
    fn len(&self) -> usize;

    /// Check if the store is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
