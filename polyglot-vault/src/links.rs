//! `links.jsonl` record shape (`18_DATA_FORMATS.md` §4.1), shared by every
//! writer regardless of which rule or user action produced the record.
//! Storage *location* splits by `origin` (18 §4.6) — this module only
//! covers the derived side (`.vault-ai/links.jsonl`: `extracted`/`parser`/
//! `git`, rebuilt whole on every rescan). `.vault/links.jsonl` (`manual`/
//! `ai`, append-only, irreproducible) is a different writer when something
//! actually needs to append to it — R1's future "accept" button, a manual
//! link the user draws in the UI. Don't reuse `write_derived_links` for
//! that; overwriting `.vault/links.jsonl` wholesale would destroy history
//! a rescan can never rebuild.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;

/// Current time as RFC 3339 — every `.vault/`/`.vault-ai/` JSONL record's
/// `ts` field uses this format (18, throughout §4).
pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkRecord {
    pub id: String,
    pub op: String, // "add" — this module never writes "del"/"retarget"
    pub from: String,
    pub rel: String,
    pub to: String,
    pub origin: String,
    pub confidence: String,
    pub ts: String,
}

pub fn new_id() -> String {
    format!("l_{}", Ulid::new())
}

/// Overwrites `.vault-ai/links.jsonl` wholesale. Safe only for derived-origin
/// links: they regenerate identically from the same source on the next
/// rescan, so there's nothing to lose by replacing the whole file instead of
/// diffing against what was there before.
pub fn write_derived_links(path: &Path, records: &[LinkRecord]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::from(r#"{"_type":"links","_v":1}"#);
    out.push('\n');
    for r in records {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    fs::write(path, out)
}

/// Reads `.vault-ai/links.jsonl` (or `.vault/links.jsonl` — same record
/// shape), skipping the `_type`/`_v` header line. A missing file just means
/// no links yet, not an error — the common case before R2 has ever run.
/// Regenerates every derived-origin link (R2 config→path, plus resolved
/// imports) and overwrites `.vault-ai/links.jsonl`. Deterministic: the same
/// vault contents always produce the same set, which is what makes
/// overwriting safe and lets both the desktop app and the MCP server call
/// this without coordinating.
///
/// Lives here rather than in either caller because both need it and neither
/// owns it: the desktop app runs it on open and on file changes, and the MCP
/// server runs it at startup so it still works for someone who has never
/// opened the app at all.
pub fn refresh_derived(root: &Path, paths: &[String]) -> io::Result<Vec<LinkRecord>> {
    let ts = now_rfc3339();
    let mut records = crate::suggest_r2::generate(root, paths, &ts);
    records.extend(crate::imports::generate(root, paths, &ts));
    write_derived_links(&crate::store::VaultLayout::new(root).derived_links_path(), &records)?;
    Ok(records)
}

pub fn read_links(path: &Path) -> io::Result<Vec<LinkRecord>> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    text.lines()
        .filter(|line| !line.contains("\"_type\""))
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect()
}

/// Appends one record to `.vault/links.jsonl` — manual/ai-origin links,
/// irreproducible, so never overwritten wholesale the way
/// `write_derived_links` overwrites `.vault-ai/links.jsonl`. This is what an
/// approved R1 suggestion (and any future manual link the user draws) ends
/// up calling.
pub fn append_manual_link(path: &Path, record: &LinkRecord) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        writeln!(file, r#"{{"_type":"links","_v":1}}"#)?;
    }
    writeln!(file, "{}", serde_json::to_string(record)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_has_the_l_prefix_and_is_unique() {
        let a = new_id();
        let b = new_id();
        assert!(a.starts_with("l_"));
        assert!(b.starts_with("l_"));
        assert_ne!(a, b);
    }

    #[test]
    fn write_derived_links_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("vault-links-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("links.jsonl");

        let record = LinkRecord {
            id: new_id(),
            op: "add".into(),
            from: "vault://tsconfig.json".into(),
            rel: "references".into(),
            to: "vault://typings/globals.d.ts".into(),
            origin: "extracted".into(),
            confidence: "certain".into(),
            ts: "2026-08-19T00:00:00Z".into(),
        };
        write_derived_links(&path, &[record.clone()]).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(r#"{"_type":"links","_v":1}"#));
        assert!(text.contains("typings/globals.d.ts"));

        assert_eq!(read_links(&path).unwrap(), vec![record]);
        assert_eq!(read_links(&dir.join("missing.jsonl")).unwrap(), Vec::new(), "a missing file must read as no links, not an error");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn append_manual_link_accumulates_instead_of_overwriting() {
        let dir = std::env::temp_dir().join(format!("vault-links-append-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("links.jsonl");

        let make = |from: &str| LinkRecord {
            id: new_id(),
            op: "add".into(),
            from: from.into(),
            rel: "describes".into(),
            to: "vault://src/router.py".into(),
            origin: "manual".into(),
            confidence: "certain".into(),
            ts: "2026-08-19T00:00:00Z".into(),
        };
        append_manual_link(&path, &make("vault://docs.md")).unwrap();
        append_manual_link(&path, &make("vault://readme.md")).unwrap();

        let records = read_links(&path).unwrap();
        assert_eq!(records.len(), 2, "a second append must add to the file, not replace it");
        assert!(records.iter().any(|r| r.from == "vault://docs.md"));
        assert!(records.iter().any(|r| r.from == "vault://readme.md"));

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().filter(|l| l.contains("\"_type\"")).count(), 1, "the header must only be written once");

        fs::remove_dir_all(&dir).unwrap();
    }
}
