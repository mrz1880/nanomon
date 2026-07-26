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
