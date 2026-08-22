//! MCP tool logic (`08_MCP_AND_AI.md`) — the four tools that doc settles on
//! (`search` / `read` / `neighbors` / `link`), independent of any transport.
//! `src/bin/vault-mcp.rs` is the stdio JSON-RPC loop that calls into this;
//! everything testable lives here.
//!
//! Two rules from 08 shape every response:
//!   - **Address round-trip**: every hit carries its `vault://` uri, so the
//!     model can feed a `search` result straight into `neighbors`/`read`
//!     instead of falling back to reading whole files.
//!   - **`neighbors_hint`**: each hit says how many links it has, by
//!     direction and rel, so the model can tell a worthwhile next call from
//!     a dead end without making it.
//!
//! Security (08 "MCP 보안"): read-only by default — `link` is the only
//! writer and in approval mode it only appends to `.vault/pending.jsonl`,
//! never to `links.jsonl`. `read` resolves **only** paths that are in the
//! index, which is what keeps `.gitignore`d and `[ignore]`d files (`.env`,
//! credentials) unreadable: refusing by path pattern would be a blocklist,
//! and this is an allowlist of what was already indexed.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::links;
use crate::live_index::LiveIndex;
use crate::store::VaultLayout;
use crate::symbol_index::SymbolEntry;

/// Cap on any single tool's result count. The model supplies `limit`, so it
/// needs a ceiling; 08's whole point is keeping responses small enough that
/// reading whole files never looks easier.
const MAX_LIMIT: usize = 100;
const DEFAULT_LIMIT: usize = 20;
const PREVIEW_CHARS: usize = 200;

#[derive(Serialize, Debug, PartialEq)]
pub struct NeighborsHint {
    /// rel -> count, for links pointing *out* of this uri.
    pub out: HashMap<String, usize>,
    /// rel -> count, for links pointing *in* to it. 18 §5.1: only the
    /// forward direction is ever stored, so "in" is computed by reversing,
    /// never by storing a second record.
    #[serde(rename = "in")]
    pub inbound: HashMap<String, usize>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Hit {
    pub uri: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<[usize; 2]>,
    pub preview: String,
    pub confidence: String,
    pub neighbors_hint: NeighborsHint,
}

/// Adjacency built once from both link stores, so `neighbors_hint` on every
/// hit is a map lookup rather than a re-read of the jsonl files per result.
#[derive(Default)]
struct LinkGraph {
    out: HashMap<String, Vec<(String, String)>>, // uri -> [(rel, target_uri)]
    inbound: HashMap<String, Vec<(String, String)>>, // uri -> [(rel, source_uri)]
}

impl LinkGraph {
    fn load(layout: &VaultLayout) -> Self {
        let mut graph = LinkGraph::default();
        let records = links::read_links(&layout.derived_links_path())
            .unwrap_or_default()
            .into_iter()
            .chain(links::read_links(&layout.links_path()).unwrap_or_default());
        for r in records {
            if r.op != "add" {
                continue; // `del`/`retarget` tombstones aren't edges (18 §4.1)
            }
            graph.out.entry(r.from.clone()).or_default().push((r.rel.clone(), r.to.clone()));
            graph.inbound.entry(r.to).or_default().push((r.rel, r.from));
        }
        graph
    }

    fn hint(&self, uri: &str) -> NeighborsHint {
        let count = |side: &HashMap<String, Vec<(String, String)>>| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for (rel, _) in side.get(uri).into_iter().flatten() {
                *counts.entry(rel.clone()).or_default() += 1;
            }
            counts
        };
        NeighborsHint { out: count(&self.out), inbound: count(&self.inbound) }
    }
}

pub struct McpServer {
    live: LiveIndex,
    layout: VaultLayout,
    graph: LinkGraph,
}

impl McpServer {
    pub fn open(root: &Path) -> io::Result<Self> {
        let live = LiveIndex::build(root)?;
        let layout = VaultLayout::new(root);
        // Derived links are regenerated here rather than assumed: the MCP
        // server has to work for someone who has never opened the desktop
        // app, and without this it would serve an empty link graph while
        // reporting it as the answer. Best-effort — a read-only vault
        // should still be searchable, just without link data.
        let paths: Vec<String> = live.table.paths().map(str::to_string).collect();
        let _ = links::refresh_derived(root, &paths);
        let graph = LinkGraph::load(&layout);
        Ok(Self { live, layout, graph })
    }

    pub fn root(&self) -> &Path {
        self.live.root()
    }

    pub fn file_count(&self) -> usize {
        self.live.table.paths().count()
    }

    fn clamp(limit: Option<usize>) -> usize {
        limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }

    fn file_hit(&self, path: &str) -> Hit {
        let uri = format!("vault://{path}");
        let preview = std::fs::read_to_string(self.root().join(path))
            .map(|text| truncate(&text))
            .unwrap_or_default();
        Hit {
            neighbors_hint: self.graph.hint(&uri),
            uri,
            kind: "file".to_string(),
            range: None,
            preview,
            confidence: "certain".to_string(),
        }
    }

    fn symbol_hit(&self, entry: &SymbolEntry) -> Hit {
        let uri = entry.address();
        let preview = std::fs::read_to_string(self.root().join(&entry.path))
            .ok()
            .and_then(|text| text.get(entry.range.clone()).map(truncate))
            .unwrap_or_default();
        Hit {
            neighbors_hint: self.graph.hint(&uri),
            uri,
            kind: entry.node_type.clone(),
            range: Some([entry.range.start, entry.range.end]),
            preview,
            confidence: "certain".to_string(),
        }
    }

    /// `kind`: "file" | "symbol" | "text" | "auto" (default). "auto" runs
    /// all three and concatenates — 08 folded five v0.1 tools into this one
    /// specifically so the model never has to pick between near-identical
    /// tools, which means "auto" has to actually work without a hint.
    pub fn search(&self, query: &str, kind: &str, limit: Option<usize>) -> Vec<Hit> {
        if query.is_empty() {
            return Vec::new();
        }
        let limit = Self::clamp(limit);
        let mut hits = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if matches!(kind, "file" | "auto") {
            for (_, id) in self.live.table.search(query, limit) {
                let hit = self.file_hit(self.live.table.path(id as usize));
                if seen.insert(hit.uri.clone()) {
                    hits.push(hit);
                }
            }
        }
        if matches!(kind, "symbol" | "auto") {
            for entry in self.live.symbols.search_entries(query, limit) {
                let hit = self.symbol_hit(entry);
                if seen.insert(hit.uri.clone()) {
                    hits.push(hit);
                }
            }
        }
        if matches!(kind, "text" | "auto") {
            for (_, id) in self.live.content.search(query, limit) {
                let hit = self.file_hit(self.live.table.path(id as usize));
                if seen.insert(hit.uri.clone()) {
                    hits.push(hit);
                }
            }
        }
        hits.truncate(limit);
        hits
    }

    /// `mode`: "full" | "outline" | "range". "outline" returns the file's
    /// symbol list with no bodies — 08's minimum-privilege mode, and also
    /// the cheap way for a model to decide what to read next.
    pub fn read(&self, uri: &str, mode: &str, start_line: Option<usize>, end_line: Option<usize>) -> Result<String, String> {
        let addr = crate::addr::parse(uri).map_err(|e| e.to_string())?;
        // Allowlist, not blocklist: only what the index already holds. That
        // is what keeps ignored files (`.env`, credentials) out — see the
        // module doc.
        if self.live.table.path_to_id(&addr.path).is_none() {
            return Err(format!("not in this vault's index: {}", addr.path));
        }
        let text = std::fs::read_to_string(self.root().join(&addr.path)).map_err(|e| e.to_string())?;

        // Any fragment means "just that piece", regardless of mode — the uri
        // is already more specific than any mode could be. Not just
        // `Fragment::Symbol`: a markdown heading, a JSON/YAML/TOML pointer,
        // and a CSV column/row/cell are all addresses `search`/`outline`
        // hand out (`SymbolEntry::address()` builds all of them), so a
        // symbol-only check silently fell through to a full-file read for
        // the other four fragment kinds — found via a real heading uri
        // returning the entire file instead of the one section asked for.
        // Matching on the rendered address rather than re-deriving each
        // fragment shape here keeps this in sync with `address()` by
        // construction instead of by two places agreeing to match.
        if addr.fragment.is_some() {
            let entry = self
                .live
                .symbols
                .entries()
                .iter()
                .find(|e| e.path == addr.path && e.address() == uri)
                .ok_or_else(|| format!("no such address in {}: {uri}", addr.path))?;
            return Ok(text.get(entry.range.clone()).unwrap_or_default().to_string());
        }

        match mode {
            "outline" => {
                let mut lines: Vec<String> = self
                    .live
                    .symbols
                    .entries()
                    .iter()
                    .filter(|e| e.path == addr.path)
                    .map(|e| format!("{} {}  →  {}", e.node_type, e.id, e.address()))
                    .collect();
                if lines.is_empty() {
                    lines.push("(no symbols extracted from this file)".to_string());
                }
                Ok(lines.join("\n"))
            }
            "range" => {
                let all: Vec<&str> = text.lines().collect();
                let start = start_line.unwrap_or(1).saturating_sub(1).min(all.len());
                let end = end_line.unwrap_or(all.len()).min(all.len());
                Ok(all.get(start..end).unwrap_or_default().join("\n"))
            }
            _ => Ok(text),
        }
    }

    /// `direction`: "out" | "in" | "both" (default). `rel` empty means all.
    /// `depth` > 1 walks transitively, breadth-first, without revisiting.
    pub fn neighbors(&self, uri: &str, rel: &[String], depth: usize, direction: &str, limit: Option<usize>) -> Vec<Hit> {
        let limit = Self::clamp(limit);
        let depth = depth.clamp(1, 5);
        let mut seen = std::collections::HashSet::from([uri.to_string()]);
        let mut frontier = vec![uri.to_string()];
        let mut found: Vec<String> = Vec::new();

        for _ in 0..depth {
            let mut next = Vec::new();
            for current in &frontier {
                let sides = match direction {
                    "out" => vec![self.graph.out.get(current)],
                    "in" => vec![self.graph.inbound.get(current)],
                    _ => vec![self.graph.out.get(current), self.graph.inbound.get(current)],
                };
                for (edge_rel, other) in sides.into_iter().flatten().flatten() {
                    if !rel.is_empty() && !rel.contains(edge_rel) {
                        continue;
                    }
                    if seen.insert(other.clone()) {
                        found.push(other.clone());
                        next.push(other.clone());
                    }
                }
            }
            frontier = next;
            if found.len() >= limit || frontier.is_empty() {
                break;
            }
        }

        found.truncate(limit);
        found.iter().map(|u| self.hit_for_uri(u)).collect()
    }

    /// A neighbor uri may name a symbol or a file; resolve whichever it is
    /// so the returned hit carries a real type and preview rather than a
    /// bare string the model has to `read` just to identify.
    fn hit_for_uri(&self, uri: &str) -> Hit {
        if let Ok(addr) = crate::addr::parse(uri)
            && let Some(crate::addr::Fragment::Symbol(parts)) = &addr.fragment
        {
            let qualname = parts.join(".");
            if let Some(entry) = self.live.symbols.entries().iter().find(|e| e.path == addr.path && e.id == qualname) {
                return self.symbol_hit(entry);
            }
        }
        let path = uri.strip_prefix("vault://").unwrap_or(uri);
        self.file_hit(path)
    }

    /// Approval mode only (08's default): an AI-proposed link lands in
    /// `.vault/pending.jsonl` with an immutable id and waits for a human.
    /// It is deliberately **not** written to `links.jsonl` here — 08's
    /// "즉시 반영 모드" needs an explicit user opt-in that doesn't exist in
    /// the UI yet, and defaulting to it would let a model write the one
    /// store that a rescan can never rebuild.
    pub fn propose_link(&self, from: &str, to: &str, rel: &str, agent: &str, rationale: &str) -> Result<String, String> {
        for uri in [from, to] {
            crate::addr::parse(uri).map_err(|e| format!("{uri}: {e}"))?;
        }
        let record = PendingLink {
            id: format!("p_{}", ulid::Ulid::new()),
            rule: "mcp".to_string(),
            agent: agent.to_string(),
            from: from.to_string(),
            rel: rel.to_string(),
            to: to.to_string(),
            rationale: rationale.to_string(),
            ts: links::now_rfc3339(),
            status: "pending".to_string(),
        };
        append_pending(&self.layout.pending_path(), &record).map_err(|e| e.to_string())?;
        Ok(record.id)
    }
}

/// 18 §4.5. `id` is immutable: a model may propose several links before the
/// user gets to any of them, and approval has to name exactly one.
#[derive(Serialize, Debug, PartialEq)]
pub struct PendingLink {
    pub id: String,
    pub rule: String,
    pub agent: String,
    pub from: String,
    pub rel: String,
    pub to: String,
    pub rationale: String,
    pub ts: String,
    pub status: String,
}

fn append_pending(path: &Path, record: &PendingLink) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        writeln!(file, r#"{{"_type":"pending","_v":1}}"#)?;
    }
    writeln!(file, "{}", serde_json::to_string(record)?)
}

/// Char-boundary-safe truncation — a preview cut mid-UTF8 would panic on
/// slicing, and vault content is routinely non-ASCII.
fn truncate(text: &str) -> String {
    let trimmed = text.trim_start();
    match trimmed.char_indices().nth(PREVIEW_CHARS) {
        Some((idx, _)) => format!("{}…", &trimmed[..idx]),
        None => trimmed.to_string(),
    }
}

/// The tool list, exactly 08's four. Shared with the stdio binary so the
/// advertised schema and the dispatch below can't drift apart.
pub fn tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "vault_search",
            "description": "Search the vault by filename, code symbol, or file content. Returns vault:// URIs usable directly with vault_read and vault_neighbors, each with a neighbors_hint showing how many links it has.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search text."},
                    "kind": {"type": "string", "enum": ["auto", "file", "symbol", "text"], "description": "What to search. Default auto (all three)."},
                    "limit": {"type": "integer", "description": "Max results, default 20, max 100."}
                },
                "required": ["query"]
            }
        },
        {
            "name": "vault_read",
            "description": "Read a vault:// URI. A URI with a symbol fragment (vault://a.py#Class.method) returns just that symbol. mode=outline lists a file's symbols without any body text — the cheapest way to decide what to read.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "uri": {"type": "string", "description": "vault:// URI, from a search or neighbors result."},
                    "mode": {"type": "string", "enum": ["full", "outline", "range"], "description": "Default full."},
                    "start_line": {"type": "integer", "description": "1-based, for mode=range."},
                    "end_line": {"type": "integer", "description": "inclusive end, for mode=range."}
                },
                "required": ["uri"]
            }
        },
        {
            "name": "vault_neighbors",
            "description": "Follow links from a vault:// URI: imports/references between code, doc-to-code describes links, folder containment. Use direction=in for backlinks (what points AT this).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "uri": {"type": "string"},
                    "rel": {"type": "array", "items": {"type": "string"}, "description": "Filter to these relations, e.g. [\"imports\"]. Empty means all."},
                    "direction": {"type": "string", "enum": ["both", "out", "in"], "description": "Default both."},
                    "depth": {"type": "integer", "description": "Hops to follow, default 1, max 5."},
                    "limit": {"type": "integer"}
                },
                "required": ["uri"]
            }
        },
        {
            "name": "vault_propose_link",
            "description": "Propose a link between two vault:// URIs. Writes to a pending queue for the user to approve — it does NOT create the link directly.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {"type": "string"},
                    "to": {"type": "string"},
                    "rel": {"type": "string", "description": "Relation name, e.g. describes, references, configures."},
                    "rationale": {"type": "string", "description": "Why this link — shown to the user at approval time."}
                },
                "required": ["from", "to", "rel"]
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-mcp-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A vault with a documented, imported Python module — enough for every
    /// tool to have something real to return.
    fn fixture(name: &str) -> (PathBuf, McpServer) {
        let dir = temp_dir(name);
        fs::write(dir.join("router.py"), "class TeacherRouter:\n    def select(self):\n        pass\n").unwrap();
        fs::write(dir.join("main.py"), "from .router import TeacherRouter\n").unwrap();
        fs::write(
            dir.join("guide.md"),
            "# Overview\n\nSee `TeacherRouter` for routing.\n\n# Setup\n\nRun `main.py` first.\n",
        )
        .unwrap();
        fs::write(dir.join("conf.json"), r#"{"entry": "router.py"}"#).unwrap();

        // Derived links, the same way the app produces them on open.
        let ts = links::now_rfc3339();
        let paths: Vec<String> = vec!["router.py".into(), "main.py".into(), "guide.md".into(), "conf.json".into()];
        let mut records = crate::suggest_r2::generate(&dir, paths.clone(), &ts);
        records.extend(crate::imports::generate(&dir, paths, &ts));
        links::write_derived_links(&VaultLayout::new(&dir).derived_links_path(), &records).unwrap();

        let server = McpServer::open(&dir).unwrap();
        (dir, server)
    }

    #[test]
    fn search_returns_addressable_uris_with_hints() {
        let (dir, server) = fixture("search");

        let hits = server.search("TeacherRouter", "symbol", None);
        let hit = hits.iter().find(|h| h.uri == "vault://router.py#TeacherRouter").expect("symbol hit");
        assert_eq!(hit.kind, "class");
        assert!(hit.preview.contains("class TeacherRouter"));
        // The uri must feed straight back into read — that round-trip is
        // 08's whole reason for putting a uri on every hit.
        assert!(server.read(&hit.uri, "full", None, None).is_ok());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn neighbors_hint_counts_real_links_by_direction() {
        let (dir, server) = fixture("hint");

        let hits = server.search("router.py", "file", None);
        let hit = hits.iter().find(|h| h.uri == "vault://router.py").expect("file hit");
        // main.py imports it and conf.json references it — both point IN.
        assert_eq!(hit.neighbors_hint.inbound.get("imports"), Some(&1));
        assert_eq!(hit.neighbors_hint.inbound.get("references"), Some(&1));
        assert!(hit.neighbors_hint.out.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn neighbors_follows_direction_and_rel_filters() {
        let (dir, server) = fixture("neighbors");

        let inbound = server.neighbors("vault://router.py", &["imports".to_string()], 1, "in", None);
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].uri, "vault://main.py");

        let outbound = server.neighbors("vault://router.py", &["imports".to_string()], 1, "out", None);
        assert!(outbound.is_empty(), "router.py imports nothing — direction must actually filter");

        let unfiltered = server.neighbors("vault://router.py", &[], 1, "in", None);
        assert_eq!(unfiltered.len(), 2, "no rel filter must include the conf.json reference too");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_outline_lists_symbols_without_bodies() {
        let (dir, server) = fixture("outline");

        let outline = server.read("vault://router.py", "outline", None, None).unwrap();
        assert!(outline.contains("TeacherRouter"));
        assert!(outline.contains("vault://router.py#TeacherRouter"), "outline must stay addressable");
        assert!(!outline.contains("pass"), "outline must not include bodies");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_with_a_symbol_fragment_returns_only_that_symbol() {
        let (dir, server) = fixture("read-symbol");

        let body = server.read("vault://router.py#TeacherRouter.select", "full", None, None).unwrap();
        assert!(body.starts_with("def select"));
        assert!(!body.contains("class TeacherRouter"), "must be the method alone, not the whole file");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Found via a real Obsidian-style query: asking for one markdown
    /// section returned the whole file, because `read` only special-cased
    /// `Fragment::Symbol` — every other fragment kind (heading, JSON/YAML/
    /// TOML pointer, CSV column/row/cell) fell through to a full-file read
    /// despite `search`/`outline` handing out those exact addresses.
    #[test]
    fn read_with_a_heading_fragment_returns_only_that_section() {
        let (dir, server) = fixture("read-heading");

        let outline = server.read("vault://guide.md", "outline", None, None).unwrap();
        let setup_uri = outline.lines().find(|l| l.contains("#h:setup")).and_then(|l| l.split("→").nth(1)).unwrap().trim();

        let section = server.read(setup_uri, "full", None, None).unwrap();
        assert!(section.contains("Run `main.py` first"));
        assert!(!section.contains("Overview"), "must be the one section, not the whole file: {section:?}");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_refuses_a_path_that_is_not_indexed() {
        let (dir, server) = fixture("read-refuse");
        // Present on disk but excluded from the index — the allowlist is
        // what keeps secrets unreadable, so this must fail even though the
        // file plainly exists.
        fs::write(dir.join(".env"), "SECRET=hunter2\n").unwrap();

        let err = server.read("vault://.env", "full", None, None).unwrap_err();
        assert!(err.contains("not in this vault's index"), "got: {err}");
        assert!(server.read("vault://../outside.txt", "full", None, None).is_err());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn propose_link_writes_to_pending_not_to_links() {
        let (dir, server) = fixture("propose");
        let layout = VaultLayout::new(&dir);

        let id = server
            .propose_link("vault://guide.md", "vault://router.py", "describes", "test-agent", "guide explains it")
            .unwrap();
        assert!(id.starts_with("p_"));

        let pending = fs::read_to_string(layout.pending_path()).unwrap();
        assert!(pending.contains(r#"{"_type":"pending","_v":1}"#));
        assert!(pending.contains(&id));
        assert!(pending.contains("\"status\":\"pending\""));
        assert!(!layout.links_path().exists(), "approval-mode propose must never touch links.jsonl");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn truncate_never_splits_a_multibyte_character() {
        let long_korean = "가".repeat(PREVIEW_CHARS + 50);
        let out = truncate(&long_korean); // would panic on a byte-index slice
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), PREVIEW_CHARS + 1);
    }
}
