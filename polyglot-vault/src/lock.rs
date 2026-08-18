//! Multi-instance lock and index schema version check for `.vault-ai/`.
//! Spec: `docs/design/11_SECURITY_PRIVACY_RELIABILITY.md` ("락 파일" / "스키마 버전").

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// Holds the OS-level lock on `.vault-ai/lock` for as long as it's alive.
/// Dropping it (or the process exiting/crashing) releases the lock — no
/// stale-PID-file cleanup needed, the OS does it.
pub struct InstanceLock {
    _file: File,
}

#[derive(Debug)]
pub struct AlreadyLockedError;

impl std::fmt::Display for AlreadyLockedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "vault is already open in another process")
    }
}

impl std::error::Error for AlreadyLockedError {}

/// Tries to acquire the single-instance lock for the vault rooted at `vault_ai_dir`
/// (i.e. `<vault>/.vault-ai/`). Returns `Ok(Err(AlreadyLockedError))`, not an `Err`,
/// when another process already holds it — that's an expected outcome, not an I/O failure.
pub fn acquire(vault_ai_dir: &Path) -> io::Result<Result<InstanceLock, AlreadyLockedError>> {
    fs::create_dir_all(vault_ai_dir)?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(vault_ai_dir.join("lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Ok(InstanceLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(Err(AlreadyLockedError)),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaCheck {
    /// No version was recorded yet (fresh `.vault-ai/`) — treated like a mismatch: index from scratch.
    FirstRun,
    Match,
    Mismatch { on_disk: u32 },
}

const SCHEMA_FILE: &str = "schema_version";

/// Compares `current` against the version recorded in `<vault_ai_dir>/state/schema_version`,
/// then writes `current` back so the next run sees a match.
pub fn check_and_write_schema_version(vault_ai_dir: &Path, current: u32) -> io::Result<SchemaCheck> {
    let state_dir = vault_ai_dir.join("state");
    fs::create_dir_all(&state_dir)?;
    let path = state_dir.join(SCHEMA_FILE);

    let check = match fs::read_to_string(&path) {
        Ok(contents) => match contents.trim().parse::<u32>() {
            Ok(on_disk) if on_disk == current => SchemaCheck::Match,
            Ok(on_disk) => SchemaCheck::Mismatch { on_disk },
            Err(_) => SchemaCheck::Mismatch { on_disk: 0 }, // corrupt file, reindex
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => SchemaCheck::FirstRun,
        Err(e) => return Err(e),
    };

    fs::write(&path, current.to_string())?;
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-lock-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_instance_is_rejected() {
        let dir = temp_dir("second-rejected");

        let first = acquire(&dir).unwrap().expect("first process should acquire the lock");
        let second = acquire(&dir).unwrap();
        assert!(second.is_err(), "second process must be rejected while the first holds the lock");

        drop(first);
        let third = acquire(&dir).unwrap();
        assert!(third.is_ok(), "lock must be released once the holder is dropped");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn schema_version_first_run_then_match_then_mismatch() {
        let dir = temp_dir("schema-version");

        assert_eq!(check_and_write_schema_version(&dir, 1).unwrap(), SchemaCheck::FirstRun);
        assert_eq!(check_and_write_schema_version(&dir, 1).unwrap(), SchemaCheck::Match);
        assert_eq!(
            check_and_write_schema_version(&dir, 2).unwrap(),
            SchemaCheck::Mismatch { on_disk: 1 }
        );
        assert_eq!(check_and_write_schema_version(&dir, 2).unwrap(), SchemaCheck::Match);

        fs::remove_dir_all(&dir).unwrap();
    }
}
