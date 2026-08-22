//! Ties `PathTable` + `InvertedIndex` to the watcher/reconciler so file
//! changes patch the live index instead of triggering a full rebuild.
//! Spec: `docs/design/05_FAST_LOCAL_SEARCH.md` "인덱싱" — "변경 파일만
//! 재파싱 → 관련 인덱스만 갱신".
//!
//! Two independent entry points feed the same `apply`, matching the
//! watcher/reconciler split (03 "File Watcher + Reconciler"):
//!   - `apply_watch_batch`: watcher hints, low-latency but can be wrong/lost.
//!     We don't trust its event *kind*, only that the path might have
//!     changed — the actual add/modify/remove call is made by checking disk.
//!   - `reconcile_now`: the authoritative backstop, diffs a full rescan
//!     against the last known snapshot (`reconcile::diff`), so it also
//!     catches whatever the watcher missed.

use std::io;
use std::path::{Path, PathBuf};

use crate::index::InvertedIndex;
use crate::reconcile::{self, Change, Snapshot};
use crate::search::PathTable;
use crate::symbol_index::SymbolIndex;

pub struct LiveIndex {
    root: PathBuf,
    pub table: PathTable,
    pub content: InvertedIndex,
    pub symbols: SymbolIndex,
    last_snapshot: Snapshot,
}

impl LiveIndex {
    /// Builds using `.vault/vault.toml` if present, documented defaults if not.
    /// A malformed config is *not* silently ignored — `config::read`'s error
    /// surfaces so a broken settings file can't quietly change what gets indexed.
    pub fn build(root: &Path) -> io::Result<Self> {
        let config = crate::config::read(root).map_err(io::Error::other)?;
        Self::build_with_config(root, &config)
    }

    pub fn build_with_config(root: &Path, config: &crate::config::VaultConfig) -> io::Result<Self> {
        let table = PathTable::build_with(root, config.ignore.use_gitignore, &config.ignore.patterns);
        let content = InvertedIndex::build_with(root, &table, config.limits.content_bytes);
        let symbols = SymbolIndex::build(root, table.paths());
        let last_snapshot = reconcile::snapshot(root)?;
        Ok(LiveIndex { root: root.to_path_buf(), table, content, symbols, last_snapshot })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn to_rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }

    /// Applies one change to the live table + content index. Public so
    /// callers with their own change source (tests, a future MCP write
    /// path) don't have to go through watch/reconcile to use it.
    pub fn apply_one(&mut self, rel_path: &str, change: &Change) {
        match change {
            Change::Removed => {
                if let Some(id) = self.table.remove(rel_path) {
                    self.content.remove_doc(id);
                }
                self.symbols.remove_doc(rel_path);
            }
            Change::Added => {
                let id = self.table.add(rel_path);
                self.content.index_doc(&self.root, id, rel_path);
                self.symbols.index_doc(&self.root, rel_path);
            }
            Change::Modified => {
                let id = self.table.add(rel_path); // no-op if already present, per PathTable::add
                self.content.remove_doc(id);
                self.content.index_doc(&self.root, id, rel_path);
                self.symbols.index_doc(&self.root, rel_path); // index_doc already drops stale entries first
            }
        }
    }

    /// Watcher path: for each changed path, infer Added/Modified/Removed by
    /// checking whether it currently exists on disk and whether we already
    /// know about it — the watcher's own event kind isn't trusted (03: a
    /// rename can arrive split into delete+create, or as one lossy hint).
    pub fn apply_watch_batch(&mut self, paths: &[PathBuf]) {
        for path in paths {
            let rel = self.to_rel(path);
            let exists = self.root.join(&rel).is_file();
            let known = self.table.path_to_id(&rel).is_some();
            let change = match (exists, known) {
                (true, true) => Change::Modified,
                (true, false) => Change::Added,
                (false, true) => Change::Removed,
                (false, false) => continue, // never existed, nothing to do
            };
            self.apply_one(&rel, &change);
        }
    }

    /// Reconciler path: full rescan, diffed against the last snapshot this
    /// `LiveIndex` took. Authoritative — catches whatever the watcher lost.
    pub fn reconcile_now(&mut self) -> io::Result<Vec<(PathBuf, Change)>> {
        let current = reconcile::snapshot(&self.root)?;
        let changes = reconcile::diff(&self.last_snapshot, &current);
        for (path, change) in &changes {
            let rel = self.to_rel(path);
            self.apply_one(&rel, change);
        }
        self.last_snapshot = current;
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-live-index-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reconcile_now_picks_up_add_modify_remove_without_full_rebuild() {
        let dir = temp_dir("reconcile");
        fs::write(dir.join("a.py"), "keepterm").unwrap();
        fs::write(dir.join("b.py"), "willberemoved").unwrap();
        let mut live = LiveIndex::build(&dir).unwrap();
        assert_eq!(live.table.len(), 2);
        assert_eq!(live.content.search("willberemoved", 10).len(), 1);

        fs::remove_file(dir.join("b.py")).unwrap();
        fs::write(dir.join("a.py"), "changedterm").unwrap();
        fs::write(dir.join("c.py"), "newterm").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10)); // ensure mtime advances past a.py's original write

        let changes = live.reconcile_now().unwrap();
        assert_eq!(changes.len(), 3, "expected exactly a.py modified, b.py removed, c.py added: {changes:?}");

        assert!(live.content.search("willberemoved", 10).is_empty(), "b.py's term must be gone");
        assert!(live.content.search("keepterm", 10).is_empty(), "a.py's old content must be gone");
        assert_eq!(live.content.search("changedterm", 10).len(), 1, "a.py's new content must be indexed");
        assert_eq!(live.content.search("newterm", 10).len(), 1, "c.py must be indexed");
        assert!(live.table.path_to_id("b.py").is_none());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn watch_batch_infers_change_kind_from_disk_state() {
        let dir = temp_dir("watch");
        fs::write(dir.join("a.py"), "original").unwrap();
        let mut live = LiveIndex::build(&dir).unwrap();

        fs::write(dir.join("a.py"), "updated").unwrap();
        fs::write(dir.join("b.py"), "brandnew").unwrap();
        live.apply_watch_batch(&[dir.join("a.py"), dir.join("b.py")]);

        assert!(live.content.search("original", 10).is_empty());
        assert_eq!(live.content.search("updated", 10).len(), 1);
        assert_eq!(live.content.search("brandnew", 10).len(), 1);
        assert_eq!(live.table.len(), 2);

        fs::remove_file(dir.join("a.py")).unwrap();
        live.apply_watch_batch(&[dir.join("a.py")]);
        assert!(live.table.path_to_id("a.py").is_none());
        assert!(live.content.search("updated", 10).is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
