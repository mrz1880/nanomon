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

#[cfg(test)]
mod tests {
    use super::MemoryStore;
    use crate::domain::Host;
    use crate::ports::MetricStore;
    use chrono::{Duration as ChronoDuration, Utc};
    use std::time::Duration;

    /// A snapshot named `hostname`, timestamped `age_secs` in the past.
    fn snapshot(hostname: &str, age_secs: i64) -> Host {
        Host::new(hostname.to_string())
            .with_timestamp(Utc::now() - ChronoDuration::seconds(age_secs))
    }

    #[test]
    fn starts_empty() {
        let store = MemoryStore::new(3);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert!(store.get_latest().is_none());
    }

    #[test]
    fn get_latest_returns_the_most_recent_store() {
        let store = MemoryStore::new(3);
        store.store(snapshot("first", 0));
        store.store(snapshot("second", 0));

        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
        assert_eq!(store.get_latest().unwrap().hostname, "second");
    }

    #[test]
    fn ring_buffer_evicts_oldest_beyond_capacity() {
        let store = MemoryStore::new(2);
        store.store(snapshot("a", 0));
        store.store(snapshot("b", 0));
        store.store(snapshot("c", 0));

        // Capacity is never exceeded, and the newest entry survives.
        assert_eq!(store.len(), 2);
        assert_eq!(store.get_latest().unwrap().hostname, "c");

        let all = store.get_history(Duration::from_secs(3600));
        let names: Vec<&str> = all.iter().map(|h| h.hostname.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]); // "a" was evicted
    }

    #[test]
    fn get_history_filters_out_snapshots_older_than_the_window() {
        let store = MemoryStore::new(10);
        store.store(snapshot("old", 600)); // 10 minutes ago
        store.store(snapshot("recent", 5)); // 5 seconds ago

        let window = store.get_history(Duration::from_secs(60));
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].hostname, "recent");

        // A wide enough window keeps both.
        assert_eq!(store.get_history(Duration::from_secs(3600)).len(), 2);
    }

    #[test]
    fn get_history_is_empty_when_nothing_is_recent_enough() {
        let store = MemoryStore::new(10);
        store.store(snapshot("old", 600));
        assert!(store.get_history(Duration::from_secs(60)).is_empty());
    }

    #[test]
    fn history_preserves_insertion_order() {
        let store = MemoryStore::new(10);
        for name in ["one", "two", "three"] {
            store.store(snapshot(name, 0));
        }

        let names: Vec<String> = store
            .get_history(Duration::from_secs(3600))
            .iter()
            .map(|h| h.hostname.clone())
            .collect();
        assert_eq!(names, vec!["one", "two", "three"]);
    }
}
