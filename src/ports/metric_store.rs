use std::sync::Arc;
use std::time::Duration;

use crate::domain::Host;

/// Port for storing and retrieving host snapshots.
/// Implementations must use interior mutability (e.g., RwLock).
pub trait MetricStore: Send + Sync {
    /// Store a new host snapshot
    fn store(&self, snapshot: Host);

    /// Get the most recent snapshot
    fn get_latest(&self) -> Option<Arc<Host>>;

    /// Get all snapshots within a time window
    fn get_history(&self, duration: Duration) -> Vec<Arc<Host>>;

    /// Get the number of stored snapshots
    #[allow(dead_code)]
    fn len(&self) -> usize;

    /// Check if the store is empty
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
