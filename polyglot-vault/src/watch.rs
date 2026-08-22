//! File watcher with debounce — a **low-latency hint only**.
//! Spec: `docs/design/03_SYSTEM_ARCHITECTURE.md` ("File Watcher + Reconciler").
//!
//! OS watch events are lost in practice (queue overflow, FSEvents directory
//! coalescing, event storms during `git checkout`). This module never tries
//! to make watching lossless — that's [`crate::reconcile`]'s job, run
//! periodically as a backstop. The watcher exists purely to make the common
//! case fast.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use notify_debouncer_mini::notify::{self, RecursiveMode};
use notify_debouncer_mini::{Debouncer, new_debouncer};

pub struct Watcher {
    // Kept alive only to hold the OS watch registered; dropping it stops
    // watching and is how a caller intentionally simulates total event loss.
    _debouncer: Debouncer<notify::RecommendedWatcher>,
    events: Receiver<notify_debouncer_mini::DebounceEventResult>,
    root: PathBuf,
}

/// `.vault/` and `.vault-ai/` are never real vault content (`scan.rs`
/// excludes them from every scan for the same reason) — but the raw OS
/// watch has no notion of that, so without filtering here, the app's own
/// writes to its bookkeeping files (R1/R2 refreshing their `.vault-ai/`
/// output, a decision landing in `.vault/decisions.jsonl`) get reported as
/// "the vault changed". A caller that reacts to that by writing to
/// `.vault-ai/` again (R1/R2's live refresh does exactly this) turns it into
/// a self-sustaining loop: write → watch event → re-run → write → ...
fn is_own_storage(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|rel| rel.components().next())
        .is_some_and(|first| matches!(first.as_os_str().to_str(), Some(".vault") | Some(".vault-ai")))
}

impl Watcher {
    /// Watches `root` recursively, coalescing raw OS events into batches
    /// after `debounce` of quiet (spec: 100-300ms).
    pub fn new(root: &Path, debounce: Duration) -> notify::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(debounce, tx)?;
        debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _debouncer: debouncer, events: rx, root: root.to_path_buf() })
    }

    /// Waits up to `timeout` for the next debounced batch of changed paths,
    /// with the vault's own `.vault`/`.vault-ai` bookkeeping filtered out.
    /// `None` covers timeout, a backend error, a disconnected channel, and
    /// a batch that turned out to be *entirely* our own writes alike —
    /// callers must not treat any of those as "nothing changed" vs. "nothing
    /// worth reacting to", which is exactly why this is folded into one type
    /// rather than exposed as a separate "was it real" flag.
    pub fn next_batch(&self, timeout: Duration) -> Option<Vec<PathBuf>> {
        match self.events.recv_timeout(timeout) {
            Ok(Ok(events)) => {
                let paths: Vec<PathBuf> =
                    events.into_iter().map(|e| e.path).filter(|p| !is_own_storage(&self.root, p)).collect();
                if paths.is_empty() { None } else { Some(paths) }
            }
            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconcile::{Change, diff, snapshot};
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-watch-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The core guarantee: even if the watcher delivers *zero* events (queue
    /// overflow, the process wasn't polling, whatever), the periodic
    /// reconciliation scan still catches every change on its own.
    #[test]
    fn reconcile_catches_changes_the_watcher_never_reported() {
        let dir = temp_dir("lost-events");
        fs::write(dir.join("a.txt"), "before").unwrap();
        let before = snapshot(&dir).unwrap();

        let watcher = Watcher::new(&dir, Duration::from_millis(100)).unwrap();

        std::thread::sleep(Duration::from_millis(10));
        fs::write(dir.join("a.txt"), "after-a-longer-write").unwrap();
        fs::write(dir.join("b.txt"), "new").unwrap();

        // Simulate total event loss: never drain the watcher's channel.
        drop(watcher);

        let after = snapshot(&dir).unwrap();
        let mut changes = diff(&before, &after);
        changes.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            changes,
            vec![
                (PathBuf::from("a.txt"), Change::Modified),
                (PathBuf::from("b.txt"), Change::Added),
            ]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Smoke test for the happy path: a real filesystem write does produce a
    /// debounced batch that names the changed file.
    #[test]
    fn watcher_reports_a_real_change() {
        let dir = temp_dir("happy-path");
        fs::write(dir.join("a.txt"), "before").unwrap();

        let watcher = Watcher::new(&dir, Duration::from_millis(100)).unwrap();
        std::thread::sleep(Duration::from_millis(200)); // let the watch registration settle
        fs::write(dir.join("a.txt"), "after").unwrap();

        let batch = watcher.next_batch(Duration::from_secs(5));
        assert!(batch.is_some(), "expected a debounced batch for a real filesystem write");
        assert!(
            batch.unwrap().iter().any(|p| p.ends_with("a.txt")),
            "expected the changed file to be named in the batch"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn is_own_storage_matches_vault_dirs_but_not_user_content() {
        let root = Path::new("/vault");
        assert!(is_own_storage(root, &root.join(".vault/links.jsonl")));
        assert!(is_own_storage(root, &root.join(".vault-ai/suggestions/r1.jsonl")));
        assert!(!is_own_storage(root, &root.join("notes/vault-planning.md")), "a real file merely named like the dirs must not match");
        assert!(!is_own_storage(root, &root.join("docs/README.md")));
    }

    /// The regression this exists for: R1/R2 write to `.vault-ai/` on their
    /// own live-refresh pass. Before filtering, the OS watcher reported that
    /// write as a real vault change, which a caller reacting to it (e.g. a
    /// visible graph re-rendering) would see as an endless reset loop — the
    /// write is itself a reaction to a "vault changed" signal.
    #[test]
    fn a_write_to_dot_vault_ai_is_not_reported_as_a_change() {
        let dir = temp_dir("own-storage-filtered");
        fs::create_dir_all(dir.join(".vault-ai/suggestions")).unwrap();

        let watcher = Watcher::new(&dir, Duration::from_millis(100)).unwrap();
        std::thread::sleep(Duration::from_millis(200)); // let the watch registration settle
        fs::write(dir.join(".vault-ai/suggestions/r1.jsonl"), "{}").unwrap();

        assert!(
            watcher.next_batch(Duration::from_millis(500)).is_none(),
            "a write under .vault-ai/ must not surface as a change"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
