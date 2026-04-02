use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::Utc;

use crate::domain::Host;
use crate::ports::MetricStore;

/// In-memory ring buffer store for host snapshots using Arc for shared ownership
pub struct MemoryStore {
    snapshots: RwLock<VecDeque<Arc<Host>>>,
    max_size: usize,
}

impl MemoryStore {
    pub fn new(max_size: usize) -> Self {
        Self {
            snapshots: RwLock::new(VecDeque::with_capacity(max_size)),
            max_size,
        }
    }
}

impl MetricStore for MemoryStore {
    fn store(&self, snapshot: Host) {
        let mut snapshots = self.snapshots.write().unwrap();

        if snapshots.len() >= self.max_size {
            snapshots.pop_front();
        }

        snapshots.push_back(Arc::new(snapshot));
    }

    fn get_latest(&self) -> Option<Arc<Host>> {
        self.snapshots.read().unwrap().back().cloned()
    }

    fn get_history(&self, duration: Duration) -> Vec<Arc<Host>> {
        let snapshots = self.snapshots.read().unwrap();
        let now = Utc::now();
        let cutoff = now - chrono::Duration::from_std(duration).unwrap_or_default();

        snapshots
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .cloned()
            .collect()
    }

    fn len(&self) -> usize {
        self.snapshots.read().unwrap().len()
    }
}
