//! R2 — config value ↔ existing path (`16_SUGGESTION_ENGINE.md` "R2").
//! A JSON/YAML/TOML string value that resolves to a real file in the vault
//! becomes a `references` link — auto-applied, not a suggestion queue entry
//! (16 "승인 정책": "경로 존재로 판정하므로 정밀도가 사실상 100%다" →
//! `origin: "extracted"`, written straight to the derived links store).
//! File-level on both ends, same as R1 — no fragment shown in 16's own
//! examples (`tsconfig.json "typings/globals.d.ts" → typings/globals.d.ts`).

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::links::{self, LinkRecord};

/// Rejects values that plainly aren't a vault-relative path before ever
/// touching the filesystem — a URL, an absolute path, or a Windows
/// drive-absolute path can't be "a path inside this vault".
fn is_plausible_relative_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 300
        && !s.contains("://")
        && !s.starts_with('/')
        && s.as_bytes().get(1) != Some(&b':')
        && !s.contains('\0')
}

fn json_string_leaves(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Object(map) => map.values().for_each(|v| json_string_leaves(v, out)),
        serde_json::Value::Array(items) => items.iter().for_each(|v| json_string_leaves(v, out)),
        _ => {}
    }
}

fn toml_string_leaves(value: &toml::Value, out: &mut Vec<String>) {
    match value {
        toml::Value::String(s) => out.push(s.clone()),
        toml::Value::Table(map) => map.values().for_each(|v| toml_string_leaves(v, out)),
        toml::Value::Array(items) => items.iter().for_each(|v| toml_string_leaves(v, out)),
        _ => {}
    }
}

fn yaml_string_leaves(value: &serde_yaml::Value, out: &mut Vec<String>) {
    match value {
        serde_yaml::Value::String(s) => out.push(s.clone()),
        serde_yaml::Value::Mapping(map) => map.values().for_each(|v| yaml_string_leaves(v, out)),
        serde_yaml::Value::Sequence(items) => items.iter().for_each(|v| yaml_string_leaves(v, out)),
        _ => {}
    }
}

fn string_values_in(root: &Path, config_path: &str) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join(config_path)) else { return Vec::new() };
    let mut out = Vec::new();
    match config_path.rsplit_once('.').map(|(_, e)| e) {
        Some("json") => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                json_string_leaves(&v, &mut out);
            }
        }
        Some("yaml") | Some("yml") => {
            if let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(&text) {
                yaml_string_leaves(&v, &mut out);
            }
        }
        Some("toml") => {
            if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                toml_string_leaves(&v, &mut out);
            }
        }
        _ => {}
    }
    out
}

/// Generates auto-applied `references` links for every `.json`/`.yaml`/
/// `.yml`/`.toml` file in `paths` whose string values resolve to another
/// file **in the index**. `ts` comes from the caller so tests (and any
/// future batch that wants one timestamp for a whole rescan) don't each get
/// a slightly different clock read.
///
/// Membership in `paths`, not `Path::is_file`, is what decides whether a
/// target counts. Checking the filesystem instead meant a config value could
/// resolve to a file the user had excluded via `.gitignore`/`[ignore]` —
/// measured on this repo, that produced 22 links pointing into
/// `node_modules/` and `target/debug/` that the index deliberately omits.
/// Those are worse than missing: `neighbors` hands the model a uri that
/// `read` then refuses, because `read`'s allowlist *is* the index.
pub fn generate(root: &Path, paths: impl IntoIterator<Item = impl AsRef<str>>, ts: &str) -> Vec<LinkRecord> {
    let paths: Vec<String> = paths.into_iter().map(|p| p.as_ref().to_string()).collect();
    let indexed: HashSet<&str> = paths.iter().map(String::as_str).collect();

    let mut records = Vec::new();
    for config_path in &paths {
        let config_path = config_path.as_str();
        let is_config = matches!(config_path.rsplit_once('.').map(|(_, e)| e), Some("json" | "yaml" | "yml" | "toml"));
        if !is_config {
            continue;
        }

        let mut seen_targets = HashSet::new();
        for raw in string_values_in(root, config_path) {
            if !is_plausible_relative_path(&raw) {
                continue;
            }
            let target = raw.replace('\\', "/");
            if target == config_path || !indexed.contains(target.as_str()) {
                continue;
            }
            if !seen_targets.insert(target.clone()) {
                continue;
            }
            records.push(LinkRecord {
                id: links::new_id(),
                op: "add".to_string(),
                from: format!("vault://{config_path}"),
                rel: "references".to_string(),
                to: format!("vault://{target}"),
                origin: "extracted".to_string(),
                confidence: "certain".to_string(),
                ts: ts.to_string(),
            });
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-suggest-r2-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn json_value_matching_a_real_path_becomes_a_link() {
        let dir = temp_dir("json");
        fs::create_dir_all(dir.join("typings")).unwrap();
        fs::write(dir.join("typings/globals.d.ts"), "declare const x: number;").unwrap();
        fs::write(dir.join("tsconfig.json"), r#"{"files": ["typings/globals.d.ts"]}"#).unwrap();

        // The target has to be listed too: `paths` is the index, and only
        // indexed files are linkable.
        let records = generate(&dir, ["tsconfig.json", "typings/globals.d.ts"], "2026-08-19T00:00:00Z");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].from, "vault://tsconfig.json");
        assert_eq!(records[0].to, "vault://typings/globals.d.ts");
        assert_eq!(records[0].rel, "references");
        assert_eq!(records[0].origin, "extracted");
        assert_eq!(records[0].confidence, "certain");
        assert!(records[0].id.starts_with("l_"));
    }

    #[test]
    fn toml_and_yaml_are_also_scanned() {
        let dir = temp_dir("toml-yaml");
        fs::write(dir.join("lint.py"), "pass").unwrap();
        fs::write(dir.join("pyproject.toml"), "[tool]\nscript = \"lint.py\"\n").unwrap();
        fs::write(dir.join("ci.yaml"), "steps:\n  - run: lint.py\n").unwrap();

        let records = generate(&dir, ["pyproject.toml", "ci.yaml", "lint.py"], "2026-08-19T00:00:00Z");
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|r| r.to == "vault://lint.py"));
    }

    #[test]
    fn a_target_that_exists_on_disk_but_is_not_indexed_is_not_linked() {
        let dir = temp_dir("excluded-target");
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("node_modules/pkg/index.js"), "").unwrap();
        fs::write(dir.join("conf.json"), r#"{"entry": "node_modules/pkg/index.js"}"#).unwrap();

        // `conf.json` is indexed; the target is deliberately not — exactly
        // what an `[ignore]` pattern produces. Linking to it anyway would
        // hand a model a uri that `mcp::read` then refuses.
        let records = generate(&dir, ["conf.json"], "2026-08-19T00:00:00Z");
        assert!(records.is_empty(), "an excluded file must not become a link target: {records:?}");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_value_that_is_not_a_real_path_is_ignored() {
        let dir = temp_dir("nonexistent");
        fs::write(dir.join("config.json"), r#"{"name": "does-not-exist.py", "version": "1.0.0"}"#).unwrap();

        let records = generate(&dir, ["config.json"], "2026-08-19T00:00:00Z");
        assert!(records.is_empty());
    }

    #[test]
    fn urls_and_absolute_paths_are_never_treated_as_vault_paths() {
        let dir = temp_dir("url-absolute");
        fs::write(dir.join("config.json"), r#"{"homepage": "https://example.com", "abs": "/etc/passwd"}"#).unwrap();

        let records = generate(&dir, ["config.json"], "2026-08-19T00:00:00Z");
        assert!(records.is_empty());
    }

    #[test]
    fn non_config_extensions_are_skipped() {
        let dir = temp_dir("skip-ext");
        fs::write(dir.join("target.py"), "pass").unwrap();
        fs::write(dir.join("readme.md"), "See `target.py`.").unwrap();

        let records = generate(&dir, ["readme.md"], "2026-08-19T00:00:00Z");
        assert!(records.is_empty());
    }
}
