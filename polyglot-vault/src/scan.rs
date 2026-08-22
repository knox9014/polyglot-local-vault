use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

/// Directories that are never user content: git's own storage and the vault's
/// two storage layers (03 "저장소 구조"). `.vault/` is committed to git so it
/// would never be caught by gitignore, and indexing our own metadata would put
/// `vault.toml` and `links.jsonl` into the user's search results and graph.
const NEVER_SCAN: [&str; 3] = [".git", ".vault", ".vault-ai"];

/// Walks `root`, respecting `.gitignore`/`.ignore` and always skipping `.git`,
/// and returns every non-directory file found.
pub fn scan_files(root: &Path) -> Vec<PathBuf> {
    scan_files_with(root, true, &[])
}

/// Same, but honoring `.vault/vault.toml`'s `[ignore]` section (18 §7):
/// `use_gitignore` toggles the git-derived rules, and `extra_patterns` are
/// additional excludes in gitignore syntax. Excluding vendored paths is an
/// accuracy decision, not a performance one (→ `06_POLYGLOT_PARSERS.md`).
pub fn scan_files_with(root: &Path, use_gitignore: bool, extra_patterns: &[String]) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false) // dotfiles like .editorconfig are real files, only .git is special-cased below
        .git_ignore(use_gitignore)
        .git_global(use_gitignore)
        .git_exclude(use_gitignore)
        .ignore(use_gitignore)
        .parents(use_gitignore)
        // `ignore`'s default is to honor `.gitignore` only inside an actual
        // git repo. A vault is routinely a plain folder holding many
        // projects, none of them `git init`ed — and there, that default
        // silently discards every `.gitignore` in the tree. Measured on this
        // repo: `ai-secretary/.gitignore` line 1 is `node_modules/`, yet
        // 80.9% of all generated links pointed inside `node_modules/`.
        // The user wrote the file to mean "exclude this"; whether they also
        // ran `git init` is beside the point.
        .require_git(false)
        .filter_entry(|entry| !NEVER_SCAN.iter().any(|d| entry.file_name() == OsStr::new(d)));

    if !extra_patterns.is_empty() {
        let mut overrides = OverrideBuilder::new(root);
        for pattern in extra_patterns {
            // `!` marks an exclude in OverrideBuilder; a bare glob would
            // whitelist instead and drop everything else.
            let _ = overrides.add(&format!("!{pattern}"));
        }
        if let Ok(built) = overrides.build() {
            builder.overrides(built);
        }
    }

    builder
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

    /// A vault is usually a plain folder of many projects, not itself a git
    /// repo — and `ignore`'s default drops every `.gitignore` in that case.
    /// Measured consequence before this was fixed: 80.9% of the links this
    /// repo generated pointed inside a `node_modules/` that a `.gitignore`
    /// had explicitly excluded.
    #[test]
    fn honors_gitignore_even_when_the_vault_is_not_a_git_repo() {
        let dir = std::env::temp_dir().join(format!("vault-scan-nogit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("project/node_modules/dep")).unwrap();
        // No `.git` anywhere — that is the whole point of this case.
        fs::write(dir.join("project/.gitignore"), "node_modules/\n").unwrap();
        fs::write(dir.join("project/node_modules/dep/index.js"), "").unwrap();
        fs::write(dir.join("project/app.js"), "").unwrap();

        let names: Vec<String> = scan_files(&dir)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();

        assert!(names.contains(&"project/app.js".to_string()));
        assert!(
            !names.iter().any(|n| n.contains("node_modules/")),
            "a .gitignore must apply even without a git repo: {names:?}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    /// The vendored excludes 06/16 specify are defaults, not opt-ins — a
    /// vault with no `.gitignore` at all must still skip `node_modules/`.
    #[test]
    fn default_config_excludes_vendored_dirs_without_any_gitignore() {
        let dir = std::env::temp_dir().join(format!("vault-scan-vendored-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::write(dir.join("node_modules/pkg/index.js"), "").unwrap();
        fs::write(dir.join("target/debug/build.log"), "").unwrap();
        fs::write(dir.join("main.py"), "").unwrap();

        let config = crate::config::VaultConfig::default();
        let names: Vec<String> = scan_files_with(&dir, config.ignore.use_gitignore, &config.ignore.patterns)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();

        assert_eq!(names, vec!["main.py"], "default patterns must exclude vendored/build dirs: {names:?}");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn never_scans_the_vaults_own_storage_dirs() {
        let dir = std::env::temp_dir().join(format!("vault-scan-own-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".vault")).unwrap();
        fs::create_dir_all(dir.join(".vault-ai/index")).unwrap();
        fs::write(dir.join(".vault/vault.toml"), "[vault]\nname = \"x\"\n").unwrap();
        fs::write(dir.join(".vault/links.jsonl"), "{}").unwrap();
        fs::write(dir.join(".vault-ai/index/names.idx"), "").unwrap();
        fs::write(dir.join("user.py"), "").unwrap();

        let names: Vec<String> = scan_files(&dir)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();

        assert_eq!(names, vec!["user.py"], "app metadata must never appear as user content: {names:?}");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn config_patterns_exclude_and_gitignore_can_be_turned_off() {
        let dir = std::env::temp_dir().join(format!("vault-scan-cfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".git")).unwrap(); // gitignore rules only apply inside a git repo
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::create_dir_all(dir.join("skipped")).unwrap();
        fs::write(dir.join(".gitignore"), "skipped/\n").unwrap();
        fs::write(dir.join("build/out.o"), "").unwrap();
        fs::write(dir.join("skipped/x.py"), "").unwrap();
        fs::write(dir.join("kept.py"), "").unwrap();

        let rel = |files: Vec<std::path::PathBuf>| -> Vec<String> {
            files
                .iter()
                .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
                .collect()
        };

        // extra pattern excludes on top of gitignore
        let names = rel(scan_files_with(&dir, true, &["build/".to_string()]));
        assert!(names.contains(&"kept.py".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("build/")), "config pattern must exclude: {names:?}");
        assert!(!names.iter().any(|n| n.starts_with("skipped/")), "gitignore still applies");

        // gitignore disabled -> its entries come back
        let names = rel(scan_files_with(&dir, false, &[]));
        assert!(names.iter().any(|n| n.starts_with("skipped/")), "use_gitignore=false must stop honoring it: {names:?}");

        fs::remove_dir_all(&dir).unwrap();
    }
}
