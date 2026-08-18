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
}

impl Watcher {
    /// Watches `root` recursively, coalescing raw OS events into batches
    /// after `debounce` of quiet (spec: 100-300ms).
    pub fn new(root: &Path, debounce: Duration) -> notify::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(debounce, tx)?;
        debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
        Ok(Self { _debouncer: debouncer, events: rx })
    }

    /// Waits up to `timeout` for the next debounced batch of changed paths.
    /// `None` covers timeout, a backend error, and a disconnected channel
    /// alike — callers must not treat any of those as "nothing changed".
    pub fn next_batch(&self, timeout: Duration) -> Option<Vec<PathBuf>> {
        match self.events.recv_timeout(timeout) {
            Ok(Ok(events)) => Some(events.into_iter().map(|e| e.path).collect()),
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
}
