use std::sync::{Arc, RwLock};

use crate::domain::DirectoryUsage;
use crate::ports::DirectoryUsageSource;

/// Caches directory-usage results so the (expensive) filesystem walk runs on a
/// schedule, never per HTTP request. Empty and inert when no source is set.
pub struct DirectoryUsageService {
    source: Option<Arc<dyn DirectoryUsageSource>>,
    cache: RwLock<Vec<DirectoryUsage>>,
}

impl DirectoryUsageService {
    pub fn new(source: Option<Arc<dyn DirectoryUsageSource>>) -> Self {
        Self {
            source,
            cache: RwLock::new(Vec::new()),
        }
    }

    /// Whether directory watching is enabled (a source is configured).
    pub fn enabled(&self) -> bool {
        self.source.is_some()
    }

    /// Recompute usage and replace the cache. No-op if disabled.
    pub async fn refresh(&self) {
        if let Some(source) = &self.source {
            let usage = source.directory_usage().await;
            *self.cache.write().unwrap() = usage;
        }
    }

    /// Current cached usage (clone).
    pub fn snapshot(&self) -> Vec<DirectoryUsage> {
        self.cache.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::DirectoryUsageService;
    use crate::domain::{DirectoryEntry, DirectoryUsage};
    use crate::ports::DirectoryUsageSource;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Counts scrapes and reports a growing size, so cache refreshes are visible.
    struct CountingSource {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl DirectoryUsageSource for CountingSource {
        async fn directory_usage(&self) -> Vec<DirectoryUsage> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            vec![DirectoryUsage {
                path: "/srv".to_string(),
                entries: vec![DirectoryEntry {
                    name: "data".to_string(),
                    bytes: n as u64 * 100,
                }],
            }]
        }
    }

    #[tokio::test]
    async fn disabled_without_a_source() {
        let service = DirectoryUsageService::new(None);
        assert!(!service.enabled());
        assert!(service.snapshot().is_empty());

        // Refreshing is a no-op rather than an error.
        service.refresh().await;
        assert!(service.snapshot().is_empty());
    }

    #[tokio::test]
    async fn snapshot_is_empty_until_the_first_refresh() {
        let service = DirectoryUsageService::new(Some(Arc::new(CountingSource {
            calls: AtomicUsize::new(0),
        })));

        assert!(service.enabled());
        // The walk only runs on refresh, never lazily on read.
        assert!(service.snapshot().is_empty());

        service.refresh().await;
        let snap = service.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].path, "/srv");
        assert_eq!(snap[0].entries[0].bytes, 100);
    }

    #[tokio::test]
    async fn refresh_replaces_the_cached_value() {
        let service = DirectoryUsageService::new(Some(Arc::new(CountingSource {
            calls: AtomicUsize::new(0),
        })));

        service.refresh().await;
        service.refresh().await;

        let snap = service.snapshot();
        // Second scrape's value replaces the first (no accumulation).
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].entries[0].bytes, 200);
    }
}
