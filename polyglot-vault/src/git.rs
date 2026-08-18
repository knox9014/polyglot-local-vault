//! Git rename-chain reader → `aliases.jsonl` records.
//! Spec: `docs/design/03_SYSTEM_ARCHITECTURE.md` ("Git reader"), `18_DATA_FORMATS.md` §4.2.
//!
//! Shells out to the `git` CLI rather than binding libgit2 — this environment
//! has no C toolchain to build a vendored libgit2, `git` is already a hard
//! requirement for a vault under version control, and `git log
//! --diff-filter=R -M` is exactly what `research/bench/resolve/resolve_bench1.py`
//! already validated the rename-detection output against.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AliasRecord {
    pub kind: &'static str, // "path" — the only kind this reader produces
    pub from: String,
    pub to: String,
    pub source: &'static str,     // "git"
    pub confidence: &'static str, // "high" (18 §4.2)
    pub ts: String,
    pub commit: String,
}

struct RenameEdge {
    to: String,
    commit: String,
    ts: String,
}

/// Collects the full rename history of `repo` and compresses chains
/// (`a -> b -> c` becomes one alias `a -> c`, carrying the commit/timestamp
/// of the last hop — the commit that put the file at its current path).
pub fn collect_rename_aliases(repo: &Path) -> io::Result<Vec<AliasRecord>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "--all", "--diff-filter=R", "--name-status", "-M", "--format=commit%x09%H%x09%cI"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    let text = String::from_utf8_lossy(&output.stdout);

    // git log lists newest commit first. Overwriting on each `old` occurrence
    // means the last write (= oldest occurrence in time) sticks, matching
    // resolve_bench1.py's validated behavior for the rare case a path name
    // is reused by an unrelated later rename.
    let mut direct: HashMap<String, RenameEdge> = HashMap::new();
    let (mut commit, mut ts) = (String::new(), String::new());
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("commit\t") {
            if let Some((sha, iso)) = rest.split_once('\t') {
                commit = sha.to_string();
                ts = iso.to_string();
            }
            continue;
        }
        let mut parts = line.split('\t');
        let status = parts.next().unwrap_or("");
        if !status.starts_with('R') {
            continue;
        }
        if let (Some(old), Some(new)) = (parts.next(), parts.next()) {
            direct.insert(old.to_string(), RenameEdge { to: new.to_string(), commit: commit.clone(), ts: ts.clone() });
        }
    }

    let mut aliases: Vec<AliasRecord> = direct
        .keys()
        .filter_map(|start| {
            let (to, commit, ts) = follow_chain(&direct, start)?;
            (to != *start).then_some(AliasRecord {
                kind: "path",
                from: start.clone(),
                to,
                source: "git",
                confidence: "high",
                ts,
                commit,
            })
        })
        .collect();
    aliases.sort_by(|a, b| a.from.cmp(&b.from));
    Ok(aliases)
}

/// Walks the rename chain starting at `start`, returning the final path and
/// the commit/timestamp of the last hop. `None` if `start` has no outgoing edge.
fn follow_chain(direct: &HashMap<String, RenameEdge>, start: &str) -> Option<(String, String, String)> {
    let mut current = start.to_string();
    let mut last: Option<(String, String, String)> = None;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..64 {
        if !seen.insert(current.clone()) {
            break; // cycle guard — shouldn't happen with real history, but don't loop forever
        }
        match direct.get(&current) {
            Some(edge) => {
                last = Some((edge.to.clone(), edge.commit.clone(), edge.ts.clone()));
                current = edge.to.clone();
            }
            None => break,
        }
    }
    last
}

/// Writes `aliases.jsonl` per 18 §4.2: a `_type`/`_v` header line, then one record per line.
pub fn write_aliases_jsonl(path: &Path, aliases: &[AliasRecord]) -> io::Result<()> {
    let mut out = String::from(r#"{"_type":"aliases","_v":1}"#);
    out.push('\n');
    for alias in aliases {
        out.push_str(&serde_json::to_string(alias)?);
        out.push('\n');
    }
    fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.email=test@test.invalid", "-c", "user.name=test"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    fn temp_repo(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-git-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        dir
    }

    #[test]
    fn compresses_a_two_hop_rename_chain() {
        let dir = temp_repo("chain");

        fs::write(dir.join("a.py"), "x = 1\n").unwrap();
        git(&dir, &["add", "a.py"]);
        git(&dir, &["commit", "-q", "-m", "add a.py"]);

        fs::rename(dir.join("a.py"), dir.join("b.py")).unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "rename a to b"]);

        fs::rename(dir.join("b.py"), dir.join("c.py")).unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "rename b to c"]);

        let last_commit = String::from_utf8(
            Command::new("git").arg("-C").arg(&dir).args(["rev-parse", "HEAD"]).output().unwrap().stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        let aliases = collect_rename_aliases(&dir).unwrap();
        assert_eq!(aliases.len(), 2, "one alias each for a->c and b->c: {aliases:?}");

        let a_alias = aliases.iter().find(|r| r.from == "a.py").unwrap();
        assert_eq!(a_alias.to, "c.py");
        assert_eq!(a_alias.commit, last_commit, "provenance should be the last hop's commit");

        let b_alias = aliases.iter().find(|r| r.from == "b.py").unwrap();
        assert_eq!(b_alias.to, "c.py");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_aliases_jsonl_matches_schema() {
        let dir = std::env::temp_dir().join(format!("vault-git-write-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aliases.jsonl");

        let aliases = vec![AliasRecord {
            kind: "path",
            from: "requests/api.py".into(),
            to: "src/requests/api.py".into(),
            source: "git",
            confidence: "high",
            ts: "2026-08-17T09:00:00+00:00".into(),
            commit: "a1b2c3d".into(),
        }];
        write_aliases_jsonl(&path, &aliases).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let mut lines = contents.lines();
        assert_eq!(lines.next().unwrap(), r#"{"_type":"aliases","_v":1}"#);
        let record: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(record["kind"], "path");
        assert_eq!(record["from"], "requests/api.py");
        assert_eq!(record["to"], "src/requests/api.py");
        assert_eq!(record["source"], "git");
        assert_eq!(record["confidence"], "high");
        assert_eq!(record["commit"], "a1b2c3d");
        assert!(lines.next().is_none());

        fs::remove_dir_all(&dir).unwrap();
    }
}
