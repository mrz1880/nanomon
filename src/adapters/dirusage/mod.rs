use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{DirectoryEntry, DirectoryUsage};
use crate::ports::DirectoryUsageSource;

/// Max recursion depth when sizing a child directory.
const MAX_DEPTH: usize = 12;
/// Max filesystem entries visited per watched directory (cost cap).
const MAX_ENTRIES: usize = 100_000;

/// Adapter computing directory sizes with std::fs, bounded in depth and count.
pub struct FsDirectoryUsage {
    dirs: Vec<PathBuf>,
    max_depth: usize,
    max_entries: usize,
}

impl FsDirectoryUsage {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            max_depth: MAX_DEPTH,
            max_entries: MAX_ENTRIES,
        }
    }
}

/// Recursively size a path, decrementing `budget` per entry visited. Symlinks
/// are not followed; the walk stops when depth or the entry budget is exhausted.
fn entry_size(path: &Path, max_depth: usize, budget: &mut usize) -> u64 {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    let ft = meta.file_type();
    if ft.is_symlink() {
        return 0;
    }
    if ft.is_file() {
        return meta.len();
    }
    if !ft.is_dir() || max_depth == 0 {
        return 0;
    }

    let mut total = 0;
    if let Ok(read_dir) = fs::read_dir(path) {
        for entry in read_dir.flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            total += entry_size(&entry.path(), max_depth - 1, budget);
        }
    }
    total
}

/// Compute usage for one watched directory: its direct children and their
/// (bounded) recursive sizes, sorted largest first. Returns `None` if the path
/// is missing or unreadable (logged, never fatal).
fn usage_for(dir: &Path, max_depth: usize, max_entries: usize) -> Option<DirectoryUsage> {
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!("Cannot read watch dir {:?}: {}", dir, e);
            return None;
        }
    };

    let mut budget = max_entries;
    let mut entries = Vec::new();
    for child in read_dir.flatten() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let name = child.file_name().to_string_lossy().to_string();
        let bytes = entry_size(&child.path(), max_depth, &mut budget);
        entries.push(DirectoryEntry { name, bytes });
    }

    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Some(DirectoryUsage {
        path: dir.to_string_lossy().to_string(),
        entries,
    })
}

#[async_trait]
impl DirectoryUsageSource for FsDirectoryUsage {
    async fn directory_usage(&self) -> Vec<DirectoryUsage> {
        let dirs = self.dirs.clone();
        let max_depth = self.max_depth;
        let max_entries = self.max_entries;

        // Filesystem walks are blocking; keep them off the async runtime.
        tokio::task::spawn_blocking(move || {
            dirs.iter()
                .filter_map(|dir| usage_for(dir, max_depth, max_entries))
                .collect()
        })
        .await
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{usage_for, MAX_DEPTH, MAX_ENTRIES};
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nanomon-dirtest-{}-{}", std::process::id(), label));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &PathBuf, bytes: usize) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn sizes_direct_children_recursively() {
        let root = scratch("sizes");
        // root/a.bin (100), root/sub/{b.bin(50), c.bin(25)}
        write_file(&root.join("a.bin"), 100);
        fs::create_dir_all(root.join("sub")).unwrap();
        write_file(&root.join("sub").join("b.bin"), 50);
        write_file(&root.join("sub").join("c.bin"), 25);

        let usage = usage_for(&root, MAX_DEPTH, MAX_ENTRIES).unwrap();

        // Two direct children: "sub" (75) and "a.bin" (100), sorted largest first.
        assert_eq!(usage.entries.len(), 2);
        assert_eq!(usage.entries[0].name, "a.bin");
        assert_eq!(usage.entries[0].bytes, 100);
        assert_eq!(usage.entries[1].name, "sub");
        assert_eq!(usage.entries[1].bytes, 75);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_path_is_ignored() {
        let missing = std::env::temp_dir().join("nanomon-dirtest-does-not-exist-xyz");
        assert!(usage_for(&missing, MAX_DEPTH, MAX_ENTRIES).is_none());
    }
}
