//! Import/`use` edges — 06's "parser" origin (`defined_in`/`imports`/`calls`,
//! derived from static analysis, no guessing). Auto-applied like R2: no
//! approval queue, because these are read straight off the source, not
//! inferred. Deliberately scoped to only the import forms that resolve to
//! one unambiguous file from path text alone — nothing here searches the
//! vault for "a file that might match", the way that would risk connecting
//! two unrelated same-named modules in this vault's many separate projects:
//!
//! ```text
//! TypeScript/JS   relative specifiers only ("./x", "../x")
//! Python          relative imports with a trailing path ("from .pkg import x")
//! Rust            "crate::..." paths, and file-backed `mod x;`
//! ```
//!
//! Not resolved, on purpose:
//! - Python absolute imports (`import foo.bar`) — needs each project's own
//!   package root, which isn't tracked; guessing risks matching the wrong
//!   same-named package in an unrelated project elsewhere in the vault.
//! - Go entirely — a module path (`github.com/x/y/pkg`) only resolves
//!   against that repo's own `go.mod`-declared module name, which isn't
//!   parsed here.
//! - Rust `self::`/`super::` — these name the *current*/*parent* module,
//!   which isn't necessarily a different file at all (an inline `mod foo {
//!   .. }` block has no file of its own); resolving them needs a real
//!   module tree, not a path computation.
//!
//! All three are still extracted by their parsers (`ParseOutput.imports`)
//! for whatever future use needs the raw text — this module just doesn't
//! turn the unresolvable ones into edges.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::links::{self, LinkRecord};
use crate::parser::ParseInput;
use crate::symbol_index::adapter_for;

/// Generates `imports` links for every `.py`/`.rs`/`.ts` file in
/// `file_paths` whose import statements resolve to another real file in the
/// vault. `existing` is every current vault path — checking membership in a
/// set built once is a lot cheaper than a filesystem stat per candidate
/// across a large vault with many import statements.
pub fn generate(root: &Path, file_paths: impl IntoIterator<Item = impl AsRef<str>>, ts: &str) -> Vec<LinkRecord> {
    let file_paths: Vec<String> = file_paths.into_iter().map(|p| p.as_ref().to_string()).collect();
    let existing: HashSet<&str> = file_paths.iter().map(String::as_str).collect();

    let mut records = Vec::new();
    for path in &file_paths {
        let Some(ext) = path.rsplit_once('.').map(|(_, e)| e) else { continue };
        let Some(adapter) = adapter_for(ext) else { continue };
        if !matches!(ext, "py" | "rs" | "ts") {
            continue; // parsed for symbols/sketches too, but only these three have a resolver below
        }
        let Ok(bytes) = fs::read(root.join(path)) else { continue };
        let output = adapter.parse(ParseInput { bytes: &bytes, previous: None });

        let mut seen_targets = HashSet::new();
        for spec in &output.imports {
            let target = match ext {
                "ts" => resolve_ts_relative(path, spec, &existing),
                "py" => resolve_python_relative(path, spec, &existing),
                "rs" => resolve_rust_use(path, spec, &existing),
                _ => None,
            };
            let Some(target) = target else { continue };
            if &target == path || !seen_targets.insert(target.clone()) {
                continue;
            }
            records.push(LinkRecord {
                id: links::new_id(),
                op: "add".to_string(),
                from: format!("vault://{path}"),
                rel: "imports".to_string(),
                to: format!("vault://{target}"),
                origin: "parser".to_string(),
                confidence: "certain".to_string(),
                ts: ts.to_string(),
            });
        }
    }
    records
}

/// `"a/b/c.ts"` -> `"a/b"` — the directory a relative import is resolved
/// against. `""` for a top-level file (nothing to strip).
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// Joins `base_dir` with a `/`-separated relative spec, resolving `.` and
/// `..` segments — string-level, not `std::path`, so it stays in the same
/// forward-slash vault-relative shape every path in this codebase uses.
fn join_relative(base_dir: &str, spec: &str) -> String {
    let mut parts: Vec<&str> = if base_dir.is_empty() { Vec::new() } else { base_dir.split('/').collect() };
    for seg in spec.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn resolve_ts_relative(importing_file: &str, spec: &str, existing: &HashSet<&str>) -> Option<String> {
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return None; // a bare specifier ("lodash") is an npm package, not in the vault
    }
    let joined = join_relative(dir_of(importing_file), spec);
    ["", ".ts", ".tsx", ".js", ".jsx", "/index.ts", "/index.tsx", "/index.js"]
        .iter()
        .map(|suffix| format!("{joined}{suffix}"))
        .find(|candidate| existing.contains(candidate.as_str()))
}

/// Only `from .pkg import x` / `from ..parent.sub import y` style specifiers
/// — dots followed by a real path segment. A bare `from . import sibling`
/// (dots only, no trailing name) is skipped: which of that directory's
/// files it means depends on the *imported name*, which isn't in
/// `ParseOutput.imports` (only the module specifier is) — resolving it
/// would mean guessing, not computing.
fn resolve_python_relative(importing_file: &str, spec: &str, existing: &HashSet<&str>) -> Option<String> {
    let dot_count = spec.chars().take_while(|&c| c == '.').count();
    if dot_count == 0 {
        return None; // absolute import — see module doc for why this isn't resolved
    }
    let sub_path = &spec[dot_count..];
    if sub_path.is_empty() {
        return None;
    }
    let levels_up = dot_count - 1; // one dot = current package's own directory
    let mut base = dir_of(importing_file).to_string();
    for _ in 0..levels_up {
        base = dir_of(&base).to_string();
    }
    let joined = join_relative(&base, &sub_path.replace('.', "/"));
    [".py", "/__init__.py"]
        .iter()
        .map(|suffix| format!("{joined}{suffix}"))
        .find(|candidate| existing.contains(candidate.as_str()))
}

/// `crate::a::b` only — resolved against the nearest ancestor directory
/// that has a `Cargo.toml`, i.e. that crate's own root, so `crate::` paths
/// in one crate can't accidentally resolve into a different crate elsewhere
/// in the vault. `mod foo;` (no `crate::` prefix at all) is file-relative
/// instead: same directory as the file that declared it.
fn resolve_rust_use(importing_file: &str, spec: &str, existing: &HashSet<&str>) -> Option<String> {
    if let Some(name) = spec.strip_prefix("mod:") {
        let joined = join_relative(dir_of(importing_file), name);
        return [".rs", "/mod.rs"].iter().map(|suffix| format!("{joined}{suffix}")).find(|c| existing.contains(c.as_str()));
    }
    let sub_path = spec.strip_prefix("crate::")?;
    // Only the path portion before any `{...}` grouped-use list or trailing
    // leaf item matters for *which file* — `crate::a::b::{c, d}` and
    // `crate::a::b::c` both point somewhere under `a/b`. Stripping to the
    // last `::`-segment-that-could-be-a-file is still a guess between "b is
    // the file, c is an item in it" vs "b/c.rs is the file" — so this tries
    // both, preferring the deeper one.
    let segments: Vec<&str> = sub_path.split("::").map(|s| s.split('{').next().unwrap_or(s).trim()).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    let crate_src = find_crate_src_root(importing_file, existing)?;
    for depth in (1..=segments.len()).rev() {
        let joined = join_relative(&crate_src, &segments[..depth].join("/"));
        if let Some(found) = [".rs", "/mod.rs"].iter().map(|suffix| format!("{joined}{suffix}")).find(|c| existing.contains(c.as_str())) {
            return Some(found);
        }
    }
    None
}

/// Walks up from `importing_file`'s directory looking for a sibling
/// `Cargo.toml`; that directory's `src/` is where `crate::` paths start.
/// Deliberately checks `existing` (the vault's own file set) rather than
/// touching the filesystem — `Cargo.toml` itself isn't a code file the
/// symbol/import scan would have listed, so this checks for it explicitly.
fn find_crate_src_root(importing_file: &str, existing: &HashSet<&str>) -> Option<String> {
    let mut dir = dir_of(importing_file).to_string();
    loop {
        let cargo_toml = if dir.is_empty() { "Cargo.toml".to_string() } else { format!("{dir}/Cargo.toml") };
        if existing.contains(cargo_toml.as_str()) {
            return Some(if dir.is_empty() { "src".to_string() } else { format!("{dir}/src") });
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir_of(&dir).to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-imports-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_typescript_relative_import_with_extension_search() {
        let dir = temp_dir("ts");
        fs::create_dir_all(dir.join("src/lib")).unwrap();
        fs::write(dir.join("src/lib/utils.ts"), "export const x = 1;").unwrap();
        fs::write(dir.join("src/main.ts"), "import { x } from \"./lib/utils\";\n").unwrap();

        let records = generate(&dir, ["src/main.ts", "src/lib/utils.ts"], "2026-08-20T00:00:00Z");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].from, "vault://src/main.ts");
        assert_eq!(records[0].to, "vault://src/lib/utils.ts");
        assert_eq!(records[0].rel, "imports");
        assert_eq!(records[0].origin, "parser");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ts_bare_specifier_is_not_resolved() {
        let dir = temp_dir("ts-bare");
        fs::write(dir.join("main.ts"), "import _ from \"lodash\";\n").unwrap();
        let records = generate(&dir, ["main.ts"], "2026-08-20T00:00:00Z");
        assert!(records.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_python_relative_import_with_a_named_path() {
        let dir = temp_dir("py");
        fs::create_dir_all(dir.join("pkg")).unwrap();
        fs::write(dir.join("pkg/util.py"), "x = 1\n").unwrap();
        fs::write(dir.join("pkg/main.py"), "from .util import x\n").unwrap();

        let records = generate(&dir, ["pkg/main.py", "pkg/util.py"], "2026-08-20T00:00:00Z");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].to, "vault://pkg/util.py");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn python_bare_dot_import_is_not_resolved() {
        let dir = temp_dir("py-bare");
        fs::create_dir_all(dir.join("pkg")).unwrap();
        fs::write(dir.join("pkg/sibling.py"), "y = 1\n").unwrap();
        fs::write(dir.join("pkg/main.py"), "from . import sibling\n").unwrap();

        let records = generate(&dir, ["pkg/main.py", "pkg/sibling.py"], "2026-08-20T00:00:00Z");
        assert!(records.is_empty(), "a bare `from . import x` names the target only via the imported name, not the module spec — must not guess");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn python_absolute_import_is_not_resolved() {
        let dir = temp_dir("py-absolute");
        fs::create_dir_all(dir.join("foo")).unwrap();
        fs::write(dir.join("foo/bar.py"), "x = 1\n").unwrap();
        fs::write(dir.join("main.py"), "from foo.bar import x\n").unwrap();

        let records = generate(&dir, ["main.py", "foo/bar.py"], "2026-08-20T00:00:00Z");
        assert!(records.is_empty(), "absolute Python imports need a package root this pass doesn't track");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_rust_crate_path_scoped_to_its_own_cargo_toml() {
        let dir = temp_dir("rs-crate");
        fs::create_dir_all(dir.join("mycrate/src/config")).unwrap();
        fs::write(dir.join("mycrate/Cargo.toml"), "[package]\nname = \"mycrate\"\n").unwrap();
        fs::write(dir.join("mycrate/src/config/settings.rs"), "pub struct Settings;\n").unwrap();
        fs::write(dir.join("mycrate/src/main.rs"), "use crate::config::settings::Settings;\n").unwrap();

        let records = generate(
            &dir,
            ["mycrate/src/main.rs", "mycrate/src/config/settings.rs", "mycrate/Cargo.toml"],
            "2026-08-20T00:00:00Z",
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].to, "vault://mycrate/src/config/settings.rs");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_file_backed_mod_declaration() {
        let dir = temp_dir("rs-mod");
        fs::write(dir.join("submodule.rs"), "pub fn f() {}\n").unwrap();
        fs::write(dir.join("lib.rs"), "mod submodule;\n").unwrap();

        let records = generate(&dir, ["lib.rs", "submodule.rs"], "2026-08-20T00:00:00Z");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].to, "vault://submodule.rs");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rust_external_crate_and_self_super_are_not_resolved() {
        let dir = temp_dir("rs-external");
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "use std::fmt;\nuse self::helper;\nuse super::other;\n").unwrap();

        let records = generate(&dir, ["src/main.rs", "Cargo.toml"], "2026-08-20T00:00:00Z");
        assert!(records.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
