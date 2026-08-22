//! `.vault/` (source of truth, git-committed) vs `.vault-ai/` (pure derivative,
//! gitignored) layout, and the "open a vault" entry point that ties layout +
//! instance lock + schema version together.
//! Spec: `docs/design/03_SYSTEM_ARCHITECTURE.md` ("저장소 구조"), `18_DATA_FORMATS.md` §4.
//!
//! The one guarantee this module exists to hold: **deleting `.vault-ai/`
//! entirely and reindexing must fully recover.** Nothing that can't be
//! regenerated may ever be written under `.vault-ai/`.

use std::path::PathBuf;
use std::{fmt, fs, io};

use crate::lock::{self, InstanceLock, SchemaCheck};

pub const SCHEMA_VERSION: u32 = 1;

pub struct VaultLayout {
    root: PathBuf,
}

impl VaultLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn vault_dir(&self) -> PathBuf {
        self.root.join(".vault")
    }

    pub fn vault_ai_dir(&self) -> PathBuf {
        self.root.join(".vault-ai")
    }

    // .vault/ — SOURCE OF TRUTH, git-committed. Never delete these to "clean up" (18 §4).
    pub fn links_path(&self) -> PathBuf {
        self.vault_dir().join("links.jsonl")
    }
    pub fn aliases_path(&self) -> PathBuf {
        self.vault_dir().join("aliases.jsonl")
    }
    pub fn decisions_path(&self) -> PathBuf {
        self.vault_dir().join("decisions.jsonl")
    }
    pub fn sketches_path(&self) -> PathBuf {
        self.vault_dir().join("sketches.jsonl")
    }
    pub fn pending_path(&self) -> PathBuf {
        self.vault_dir().join("pending.jsonl")
    }
    pub fn config_path(&self) -> PathBuf {
        self.vault_dir().join("vault.toml")
    }

    // .vault-ai/ — pure derivative, gitignored. Every path here must be
    // reproducible from `.vault/` + the filesystem + git history alone.
    pub fn index_dir(&self) -> PathBuf {
        self.vault_ai_dir().join("index")
    }
    pub fn parsed_dir(&self) -> PathBuf {
        self.vault_ai_dir().join("parsed")
    }
    pub fn similarity_dir(&self) -> PathBuf {
        self.vault_ai_dir().join("similarity")
    }
    pub fn suggestions_dir(&self) -> PathBuf {
        self.vault_ai_dir().join("suggestions")
    }
    /// Derived-origin links (18 §4.6: `extracted`/`parser`/`git` — R2/R6 and
    /// future static-analysis edges). Distinct from `links_path()`, which is
    /// `.vault/` and holds only `manual`/`ai`-origin links (irreproducible).
    pub fn derived_links_path(&self) -> PathBuf {
        self.vault_ai_dir().join("links.jsonl")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.vault_ai_dir().join("state")
    }

    /// Creates every `.vault/` and `.vault-ai/` directory (idempotent).
    /// Called on first open and again after `.vault-ai/` has been wiped.
    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(self.vault_dir())?;
        for dir in [self.index_dir(), self.parsed_dir(), self.similarity_dir(), self.suggestions_dir(), self.state_dir()] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

pub struct Vault {
    pub layout: VaultLayout,
    pub schema: SchemaCheck,
    _lock: InstanceLock,
}

#[derive(Debug)]
pub enum OpenError {
    AlreadyOpenElsewhere,
    Io(io::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::AlreadyOpenElsewhere => write!(f, "vault is already open in another process"),
            OpenError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<io::Error> for OpenError {
    fn from(e: io::Error) -> Self {
        OpenError::Io(e)
    }
}

impl Vault {
    /// Opens (or creates) the vault rooted at `root`: builds the `.vault/` +
    /// `.vault-ai/` directory skeleton, takes the single-instance lock, and
    /// checks the index schema version. `schema` tells the caller whether a
    /// reindex is needed (`FirstRun` or `Mismatch`) — this function itself
    /// never reindexes, that's the caller's job once it holds a `Vault`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, OpenError> {
        let layout = VaultLayout::new(root);
        layout.ensure_dirs()?;
        let vault_ai_dir = layout.vault_ai_dir();
        let lock = lock::acquire(&vault_ai_dir)?.map_err(|_| OpenError::AlreadyOpenElsewhere)?;
        let schema = lock::check_and_write_schema_version(&vault_ai_dir, SCHEMA_VERSION)?;
        Ok(Self { layout, schema, _lock: lock })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-store-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The core guarantee (종료 조건 ③): wiping `.vault-ai/` entirely and
    /// reopening must leave `.vault/` (source of truth) untouched, and must
    /// rebuild a fresh, empty `.vault-ai/` skeleton — no data that only ever
    /// lived under `.vault-ai/` is required to come back.
    fn assert_wiping_vault_ai_fully_recovers(root: &Path) {
        let vault = Vault::open(root).unwrap();
        assert_eq!(vault.schema, SchemaCheck::FirstRun);

        // Source of truth: a real git-sourced alias, written the way git.rs would.
        let aliases = vec![crate::git::AliasRecord {
            kind: "path",
            from: "old/name.py".into(),
            to: "new/name.py".into(),
            source: "git",
            confidence: "high",
            ts: "2026-08-18T00:00:00+00:00".into(),
            commit: "deadbeef".into(),
        }];
        crate::git::write_aliases_jsonl(&vault.layout.aliases_path(), &aliases).unwrap();

        // Pure derivative: some index cache that only ever belongs under .vault-ai/.
        fs::write(vault.layout.index_dir().join("names.idx"), "pretend-index-bytes").unwrap();

        let aliases_before = fs::read_to_string(vault.layout.aliases_path()).unwrap();
        drop(vault); // release the instance lock before wiping / reopening

        fs::remove_dir_all(root.join(".vault-ai")).unwrap();

        let reopened = Vault::open(root).unwrap();
        assert_eq!(
            reopened.schema,
            SchemaCheck::FirstRun,
            "schema state resets after .vault-ai/ is wiped — that's expected, not a defect"
        );

        let aliases_after = fs::read_to_string(reopened.layout.aliases_path()).unwrap();
        assert_eq!(aliases_before, aliases_after, ".vault/ must survive a .vault-ai/ wipe untouched");

        assert!(reopened.layout.index_dir().is_dir(), "reindexing needs the directory skeleton back");
        assert!(
            !reopened.layout.index_dir().join("names.idx").exists(),
            "the old derived cache must NOT need to survive — it's regenerated by reindexing, not recovered"
        );
    }

    #[test]
    fn wiping_vault_ai_fully_recovers() {
        let dir = temp_dir("recover");
        assert_wiping_vault_ai_fully_recovers(&dir);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn second_open_is_rejected_while_first_is_held() {
        let dir = temp_dir("second-open");
        let first = Vault::open(&dir).unwrap();
        let second = Vault::open(&dir);
        assert!(matches!(second, Err(OpenError::AlreadyOpenElsewhere)));
        drop(first);
        fs::remove_dir_all(&dir).unwrap();
    }
}
