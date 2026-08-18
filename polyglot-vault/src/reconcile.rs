//! Reconciliation scan: catches changes the watcher missed.
//! Spec: `docs/design/03_SYSTEM_ARCHITECTURE.md` ("File Watcher + Reconciler").
//!
//! Watcher events can be lost (queue overflow, coalescing, event storms), so a
//! shallow `(path, mtime_ns, size)` scan runs periodically as a backstop and is
//! diffed against the last known snapshot. Deliberately cheap: no file content
//! is read, matching the measured cost of 281ms @ 100K files (single pass,
//! O(n) in file count).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{fs, io};

use crate::scan::scan_files;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileState {
    pub mtime_ns: u128,
    pub size: u64,
}

pub type Snapshot = HashMap<PathBuf, FileState>;

/// Scans `root` and records `(path, mtime_ns, size)` for every file, keyed by
/// path relative to `root`. Cheap on purpose — a `stat`, not a read.
pub fn snapshot(root: &Path) -> io::Result<Snapshot> {
    let mut result = Snapshot::new();
    for path in scan_files(root) {
        let metadata = fs::metadata(&path)?;
        let mtime_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        result.insert(rel, FileState { mtime_ns, size: metadata.len() });
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    /// `(path, mtime_ns, size)` changed — could be an edit, or a delete+create
    /// pair the watcher can't tell apart from an edit either (→ 03 "rename").
    Modified,
}

/// Diffs two snapshots. A single pass over each side's keys — linear in the
/// number of files, not the number of changes.
pub fn diff(previous: &Snapshot, current: &Snapshot) -> Vec<(PathBuf, Change)> {
    let mut changes = Vec::new();
    for (path, state) in current {
        match previous.get(path) {
            None => changes.push((path.clone(), Change::Added)),
            Some(prev_state) if prev_state != state => {
                changes.push((path.clone(), Change::Modified))
            }
            Some(_) => {}
        }
    }
    for path in previous.keys() {
        if !current.contains_key(path) {
            changes.push((path.clone(), Change::Removed));
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-reconcile-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn diff_detects_added_removed_modified_unchanged() {
        let dir = temp_dir("diff");
        fs::write(dir.join("stable.txt"), "same").unwrap();
        fs::write(dir.join("about_to_change.txt"), "before").unwrap();
        fs::write(dir.join("about_to_go.txt"), "bye").unwrap();

        let before = snapshot(&dir).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10)); // ensure mtime advances
        fs::write(dir.join("about_to_change.txt"), "after-longer-content").unwrap();
        fs::remove_file(dir.join("about_to_go.txt")).unwrap();
        fs::write(dir.join("new.txt"), "new").unwrap();

        let after = snapshot(&dir).unwrap();
        let mut changes = diff(&before, &after);
        changes.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            changes,
            vec![
                (PathBuf::from("about_to_change.txt"), Change::Modified),
                (PathBuf::from("about_to_go.txt"), Change::Removed),
                (PathBuf::from("new.txt"), Change::Added),
            ]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn identical_snapshots_produce_no_changes() {
        let dir = temp_dir("no-op");
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("b.txt"), "b").unwrap();

        let snap = snapshot(&dir).unwrap();
        assert!(diff(&snap, &snap).is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
