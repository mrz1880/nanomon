use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::Duration;

use chrono::Utc;

use crate::domain::Host;
use crate::ports::MetricStore;

/// In-memory ring buffer store for host snapshots
pub struct MemoryStore {
    snapshots: RwLock<VecDeque<Host>>,
    max_size: usize,
}

impl MemoryStore {
    pub fn new(max_size: usize) -> Self {
        Self {
            snapshots: RwLock::new(VecDeque::with_capacity(max_size)),
            max_size,
        }
    }

    pub fn with_default_size() -> Self {
        Self::new(360) // 1 hour at 10s interval
    }
}

impl MetricStore for MemoryStore {
    fn store(&mut self, snapshot: Host) {
        let mut snapshots = self.snapshots.write().unwrap();

        if snapshots.len() >= self.max_size {
            snapshots.pop_front();
        }

        snapshots.push_back(snapshot);
    }

    fn get_latest(&self) -> Option<&Host> {
        // SAFETY: We cannot return a reference to data behind a RwLock
        // because the lock guard would be dropped. Instead, we'll need to
        // change the trait to return owned data or use Arc.
        // For now, return None to compile, but this needs redesign.
        None
    }

    fn get_history(&self, duration: Duration) -> Vec<&Host> {
        // Same issue as get_latest - cannot return references
        Vec::new()
    }

    fn len(&self) -> usize {
        self.snapshots.read().unwrap().len()
    }
}

// Better implementation: use Arc for shared ownership
use std::sync::Arc;

/// In-memory ring buffer store using Arc for shared snapshots
pub struct ArcMemoryStore {
    snapshots: RwLock<VecDeque<Arc<Host>>>,
    max_size: usize,
}

impl ArcMemoryStore {
    pub fn new(max_size: usize) -> Self {
        Self {
            snapshots: RwLock::new(VecDeque::with_capacity(max_size)),
            max_size,
        }
    }

    pub fn with_default_size() -> Self {
        Self::new(360) // 1 hour at 10s interval
    }

    pub fn store(&mut self, snapshot: Host) {
        let mut snapshots = self.snapshots.write().unwrap();

        if snapshots.len() >= self.max_size {
            snapshots.pop_front();
        }

        snapshots.push_back(Arc::new(snapshot));
    }

    pub fn get_latest(&self) -> Option<Arc<Host>> {
        self.snapshots.read().unwrap().back().cloned()
    }

    pub fn get_history(&self, duration: Duration) -> Vec<Arc<Host>> {
        let snapshots = self.snapshots.read().unwrap();
        let now = Utc::now();
        let cutoff = now - chrono::Duration::from_std(duration).unwrap_or_default();

        snapshots
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.snapshots.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
