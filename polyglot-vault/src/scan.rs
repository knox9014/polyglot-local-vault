use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Walks `root`, respecting `.gitignore`/`.ignore` and always skipping `.git`,
/// and returns every non-directory file found.
pub fn scan_files(root: &Path) -> Vec<PathBuf> {
    WalkBuilder::new(root)
        .hidden(false) // dotfiles like .editorconfig are real files, only .git is special-cased below
        .filter_entry(|entry| entry.file_name() != OsStr::new(".git"))
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .map(|entry| entry.into_path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn respects_gitignore_and_skips_git_dir() {
        let dir = std::env::temp_dir().join(format!("vault-scan-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(dir.join("vendored")).unwrap();
        fs::write(dir.join(".gitignore"), "vendored/\n").unwrap();
        fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(dir.join("vendored/dep.py"), "").unwrap();
        fs::write(dir.join("kept.py"), "").unwrap();

        let files = scan_files(&dir);
        let names: Vec<_> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();

        assert!(names.contains(&"kept.py".to_string()));
        assert!(names.contains(&".gitignore".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("vendored/")));
        assert!(!names.iter().any(|n| n.starts_with(".git/")));

        fs::remove_dir_all(&dir).unwrap();
    }
}
