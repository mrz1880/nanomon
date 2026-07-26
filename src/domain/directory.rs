use serde::{Deserialize, Serialize};

/// A direct child of a watched directory with its (bounded) recursive size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub bytes: u64,
}

/// Per-directory usage report: a watched path and its direct children's sizes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryUsage {
    pub path: String,
    pub entries: Vec<DirectoryEntry>,
}
