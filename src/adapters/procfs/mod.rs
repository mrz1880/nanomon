mod parser;
mod process;
mod system;

use std::path::PathBuf;

pub use process::ProcfsProcessSource;
pub use system::ProcfsSystemSource;

/// Configuration for procfs paths (useful for Docker mounts)
#[derive(Debug, Clone)]
pub struct ProcfsConfig {
    pub proc_path: PathBuf,
    pub sys_path: PathBuf,
    /// Host name to report instead of the one detected locally.
    ///
    /// A container cannot discover its host's name: `/etc/hostname` is its own
    /// and `/proc/sys/kernel/hostname` is namespaced even with `/proc` bind
    /// mounted, so the value has to be supplied.
    pub hostname: Option<String>,
}

impl ProcfsConfig {
    pub fn new(proc_path: impl Into<PathBuf>, sys_path: impl Into<PathBuf>) -> Self {
        Self {
            proc_path: proc_path.into(),
            sys_path: sys_path.into(),
            hostname: None,
        }
    }

    /// Report `hostname` instead of the locally detected one.
    pub fn with_hostname(mut self, hostname: Option<String>) -> Self {
        self.hostname = hostname;
        self
    }

    pub fn host() -> Self {
        Self {
            proc_path: PathBuf::from("/proc"),
            sys_path: PathBuf::from("/sys"),
            hostname: None,
        }
    }
}

impl Default for ProcfsConfig {
    fn default() -> Self {
        Self::host()
    }
}

/// Combined adapter for both system and process sources
#[derive(Debug, Clone)]
pub struct ProcfsAdapter {
    config: ProcfsConfig,
}

impl ProcfsAdapter {
    pub fn new(config: ProcfsConfig) -> Self {
        Self { config }
    }

    #[allow(dead_code)]
    pub fn with_default_paths() -> Self {
        Self::new(ProcfsConfig::default())
    }

    pub fn system_source(&self) -> ProcfsSystemSource {
        ProcfsSystemSource::new(self.config.clone())
    }

    pub fn process_source(&self) -> ProcfsProcessSource {
        ProcfsProcessSource::new(self.config.clone())
    }
}
