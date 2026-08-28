use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use base64::Engine;
use polyglot_vault::config::{self, VaultConfig};
use polyglot_vault::live_index::LiveIndex;
use polyglot_vault::reconcile::Change;
use polyglot_vault::store::VaultLayout;
use polyglot_vault::watch::Watcher;
use polyglot_vault::{addr, links, suggest_r1};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

struct AppState {
    /// `RwLock`, not `Mutex` — most commands (search, open_file, graph_data,
    /// R1/R2's background rescans, ...) only ever read this. R1/R2's first
    /// pass in particular can take a while on a doc-heavy vault; under a
    /// plain `Mutex` that held the lock the whole time every other command
    /// would freeze waiting for it, right after the window had just opened
    /// specifically to avoid a freeze (found in review — the fix that moved
    /// R1/R2 off the open-vault path traded a pre-window freeze for a
    /// post-window one). Only `apply_watch_batch`/`reconcile_now` (mutate in
    /// place) and a vault open/config-rebuild (replace it outright) need
    /// `.write()`.
    vault: RwLock<Option<LiveIndex>>,
    /// tag -> paths that contain it. Kept live by the watcher thread
    /// (`spawn_watcher`) alongside the index itself.
    tags: Mutex<HashMap<String, Vec<String>>>,
    /// Bumped every time a vault is (re)opened. The background watcher
    /// thread for a vault checks this before each iteration and exits once
    /// it no longer matches — the simplest way to stop the *previous*
    /// vault's thread without a cancellation channel per open.
    watch_generation: Arc<AtomicU64>,
}

/// Backstop interval: catches whatever the watcher itself lost (queue
/// overflow, coalesced events) — same role as `reconcile_now` elsewhere.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

/// Gates `refresh_derived_links` (R2 + import resolution) — covers R2's
/// config extensions and the three languages `imports::generate` actually
/// resolves (py/rs/ts). Go/markdown/etc. changing can't move either.
fn is_derived_links_extension(path: &std::path::Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("json" | "yaml" | "yml" | "toml" | "py" | "rs" | "ts"))
}

/// R1 rescans every `.md` file in the vault (`suggest_r1::generate` reads
/// each one from disk, same cost shape as R2's config scan) and matches
/// against code-symbol extensions — a change to anything else (an image, a
/// `.json` file, ...) can't move R1's output.
fn is_r1_relevant_extension(path: &std::path::Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "py" | "go" | "rs" | "ts" | "ipynb"))
}

/// The watcher hands back absolute OS paths; `reconcile_now`'s diffs are
/// already vault-relative (`reconcile::snapshot` stores them that way). This
/// normalizes either into the `"a/b.png"` shape the frontend's `allFiles`/
/// `currentOpenPath` use — `strip_prefix` simply fails (and falls through
/// unchanged) on a path that's already relative, so one function covers both.
fn to_vault_relative(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix(root).unwrap_or(path);
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Runs for the lifetime of one open vault: applies watcher batches to the
/// live index, backstops with a periodic full reconcile, and tells the
/// frontend to refresh whenever either one actually changes something.
/// Exits as soon as `generation` no longer matches `my_gen` (vault closed
/// or replaced by a newer `open_vault`/`open_vault_window` call).
fn spawn_watcher(app: tauri::AppHandle, root: PathBuf, generation: Arc<AtomicU64>, my_gen: u64) {
    std::thread::spawn(move || {
        // First R1/R2 pass, off the UI's critical path (see `do_open_vault`).
        {
            let state = app.state::<AppState>();
            let guard = state.vault.read().unwrap();
            if let Some(live) = guard.as_ref() {
                refresh_derived_links(live);
                refresh_suggestions(live);
            }
            drop(guard);
            let _ = app.emit("vault-changed", Vec::<String>::new());
        }

        let watcher = match Watcher::new(&root, WATCH_DEBOUNCE) {
            Ok(w) => w,
            Err(_) => return, // e.g. path removed between open and here; nothing to watch
        };
        let mut last_reconcile = Instant::now();

        loop {
            if generation.load(Ordering::SeqCst) != my_gen {
                return;
            }

            let mut changed = false;
            // Which vault-relative paths actually changed this cycle — sent
            // to the frontend so it only re-fetches the open viewer's file
            // (a re-fetch clears then resets an <img>/<audio> src, which
            // flickers) when that specific file was touched, not on every
            // unrelated change anywhere in the vault.
            let mut changed_paths: Vec<String> = Vec::new();
            // refresh_derived_links (R2 + imports) rescans every config and
            // py/rs/ts file in the vault — real cost on a vault with many of
            // them, so only pay it when something that could actually
            // change its output happened: one of those files, or a file
            // appearing/disappearing (existence is what both checks). A
            // content-only edit to some unrelated `.md` can't affect either.
            let mut r2_relevant = false;
            let mut r1_relevant = false;
            if let Some(paths) = watcher.next_batch(WATCH_DEBOUNCE) {
                // The watcher only gives us paths, not add/modify/remove
                // kind, so this is conservative in one direction only: it
                // can't miss a relevant edit, but a same-named file being
                // added/removed elsewhere (rare) waits for the reconcile
                // backstop below, which does have the change kind.
                r2_relevant = paths.iter().any(|p| is_derived_links_extension(p));
                r1_relevant = paths.iter().any(|p| is_r1_relevant_extension(p));
                changed_paths.extend(paths.iter().filter_map(|p| to_vault_relative(&root, p)));
                let state = app.state::<AppState>();
                let mut guard = state.vault.write().unwrap();
                if let Some(live) = guard.as_mut() {
                    live.apply_watch_batch(&paths);
                    changed = true;
                }
            }

            if last_reconcile.elapsed() >= RECONCILE_INTERVAL {
                let state = app.state::<AppState>();
                let mut guard = state.vault.write().unwrap();
                if let Some(live) = guard.as_mut() {
                    if let Ok(diffs) = live.reconcile_now() {
                        changed = changed || !diffs.is_empty();
                        r2_relevant = r2_relevant
                            || diffs.iter().any(|(p, c)| is_derived_links_extension(p) || !matches!(c, Change::Modified));
                        r1_relevant = r1_relevant
                            || diffs.iter().any(|(p, c)| is_r1_relevant_extension(p) || !matches!(c, Change::Modified));
                        changed_paths.extend(diffs.iter().filter_map(|(p, _)| to_vault_relative(&root, p)));
                    }
                }
                last_reconcile = Instant::now();
            }

            if changed {
                let state = app.state::<AppState>();
                let guard = state.vault.read().unwrap();
                if let Some(live) = guard.as_ref() {
                    *state.tags.lock().unwrap() = build_tag_index(live);
                    if r2_relevant {
                        refresh_derived_links(live);
                    }
                    if r1_relevant {
                        refresh_suggestions(live);
                    }
                }
                drop(guard);
                let _ = app.emit("vault-changed", changed_paths);
            }
        }
    });
}

#[derive(Serialize, Debug, PartialEq)]
struct FileView {
    content: String,
    /// `---`-fenced frontmatter at the top of the file, if any (key, value) in order.
    frontmatter: Vec<(String, String)>,
}

#[derive(Serialize, Debug, PartialEq)]
struct SearchHit {
    path: String,
    score: f64,
}

#[derive(Serialize, Debug, PartialEq)]
struct SymbolHit {
    address: String, // vault://path#Qualname — click target
    name: String,    // bare symbol/heading/column name, for display
    path: String,
    node_type: String, // "function" | "class" | "heading" | "column" | ... — frontend picks an icon
}

#[derive(Serialize, Debug, PartialEq)]
struct SearchResults {
    filename_hits: Vec<SearchHit>,
    symbol_hits: Vec<SymbolHit>,
    content_hits: Vec<SearchHit>,
}

#[derive(Serialize, Debug, PartialEq)]
struct SuggestionView {
    from: String,
    to: String,
    token: String,
    mention_count: usize,
}

#[derive(Serialize, Debug, PartialEq)]
struct GraphNode {
    id: String,
    label: String,
    kind: &'static str, // "folder" | "file"
    /// Lowercased file extension ("" for folders) — the frontend maps this to
    /// a per-language color. Kept as raw data here; color is presentation.
    ext: String,
    /// Creation time in epoch millis (falls back to modified time on file
    /// systems that don't record creation). Drives the timeline animation:
    /// nodes appear in the order the files were actually made.
    time: u64,
}

#[derive(Serialize, Debug, PartialEq)]
struct GraphEdge {
    source: String,
    target: String,
    rel: String, // "contains" (folder structure) or a real link's rel ("references", ...)
}

#[derive(Serialize, Debug)]
struct GraphData {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

// ---- pure logic, no tauri::State — this is what's actually tested ----

fn do_open_vault(path: &str) -> Result<LiveIndex, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("not a directory: {}", root.display()));
    }
    LiveIndex::build(&root).map_err(|e| e.to_string())
    // R1/R2 deliberately don't run here — they each re-read every doc/config
    // file, which on a slow disk doubles the wait before the window even
    // appears. `spawn_watcher` does the first pass on its own thread and
    // emits `vault-changed` when it lands, so the UI opens on the index
    // alone and links/suggestions fill in a moment later.
}

/// Runs R2 (config value ↔ existing path) and import/`use` resolution,
/// overwriting `.vault-ai/links.jsonl` with the combined result — both are
/// "parser"/"extracted"-origin, deterministic, no approval queue, so they
/// share one derived-links file same as they'd share one rescan trigger.
/// Best-effort: a vault the user can't write `.vault-ai/` into should still
/// open and search, just without this — so failures here are swallowed
/// rather than surfaced as an error.
fn refresh_derived_links(live: &LiveIndex) {
    let paths: Vec<String> = live.table.paths().map(str::to_string).collect();
    let _ = links::refresh_derived(live.root(), &paths);
}

/// Runs R1 (doc inline-code token ↔ symbol), drops anything the user's
/// already accepted/rejected (`decisions.jsonl`), and writes what's left to
/// `.vault-ai/suggestions/r1.jsonl` — also returned directly so
/// `list_suggestions` doesn't need a second pass over the same file.
fn refresh_suggestions(live: &LiveIndex) -> Vec<suggest_r1::Candidate> {
    let vault = VaultLayout::new(live.root());
    let decisions = suggest_r1::read_decisions(&vault.decisions_path()).unwrap_or_default();
    let candidates = suggest_r1::generate(live.root(), live.table.paths(), &live.symbols);
    let filtered = suggest_r1::filter_undecided(candidates, &decisions);
    let _ = suggest_r1::write_suggestions(&vault.suggestions_dir().join("r1.jsonl"), &filtered);
    filtered
}

const DEFAULT_SEARCH_LIMIT: usize = 20;
const MAX_SEARCH_LIMIT: usize = 200; // guard: the caller is UI-supplied

fn do_search(live: &LiveIndex, query: &str, limit: usize) -> SearchResults {
    if query.is_empty() {
        return SearchResults { filename_hits: vec![], symbol_hits: vec![], content_hits: vec![] };
    }
    let limit = limit.clamp(1, MAX_SEARCH_LIMIT);
    let filename_hits = live
        .table
        .search(query, limit)
        .into_iter()
        .map(|(score, id)| SearchHit { path: live.table.path(id as usize).to_string(), score: score as f64 })
        .collect();
    let symbol_hits = live
        .symbols
        .search_entries(query, limit)
        .into_iter()
        .map(|e| SymbolHit {
            address: e.address(),
            name: e.id.rsplit('.').next().unwrap_or(&e.id).rsplit('/').next().unwrap_or(&e.id).to_string(),
            path: e.path.clone(),
            node_type: e.node_type.clone(),
        })
        .collect();
    let content_hits = live
        .content
        .search(query, limit)
        .into_iter()
        .map(|(score, id)| SearchHit { path: live.table.path(id as usize).to_string(), score })
        .collect();
    SearchResults { filename_hits, symbol_hits, content_hits }
}

/// `addr::normalize_path` rejects absolute paths and `..` traversal — the
/// same check the address system already uses, reused here so a read or
/// write can't be pointed outside the vault by a crafted path (the UI only
/// ever sends paths it got back from `list_files`/search, but a Tauri
/// command is a process boundary, not something that trusts its caller).
fn do_open_file(live: &LiveIndex, path: &str) -> Result<FileView, String> {
    let normalized = addr::normalize_path(path).map_err(|e| e.to_string())?;
    let raw = std::fs::read_to_string(live.root().join(normalized)).map_err(|e| e.to_string())?;
    let frontmatter = extract_frontmatter(&raw).unwrap_or_default();
    Ok(FileView { content: raw, frontmatter })
}

fn do_save_file(live: &LiveIndex, path: &str, content: &str) -> Result<(), String> {
    let normalized = addr::normalize_path(path).map_err(|e| e.to_string())?;
    let full = live.root().join(normalized);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(full, content).map_err(|e| e.to_string())
}

/// Reads a binary file (image/audio) as base64 for the frontend to embed
/// directly as a `data:` URL — simpler than wiring up Tauri's `asset:`
/// protocol and its scope config for a vault root that changes at runtime,
/// at the cost of base64's ~33% size overhead. No size cap: the user asked
/// for it removed after the low-spec pass added one.
fn do_read_file_base64(live: &LiveIndex, path: &str) -> Result<String, String> {
    let normalized = addr::normalize_path(path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(live.root().join(normalized)).map_err(|e| e.to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn do_list_files(live: &LiveIndex) -> Vec<String> {
    let mut paths: Vec<String> = live.table.paths().map(str::to_string).collect();
    paths.sort();
    paths
}

/// Parses a leading `---\n ... \n---` frontmatter block. Only flat `key: value`
/// lines — no nested YAML, no lists. Good enough for Obsidian-style properties;
/// upgrade to a real YAML parser if a real gap shows up.
fn extract_frontmatter(text: &str) -> Option<Vec<(String, String)>> {
    let body = text.strip_prefix("---\r\n").or_else(|| text.strip_prefix("---\n"))?;
    let end = body.find("\n---")?;
    let block = &body[..end];
    Some(
        block
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect(),
    )
}

/// Inverts the index's per-doc tags into tag -> paths for the sidebar.
/// Pure in-memory: the tags were already extracted during the content index's
/// file read, so opening a vault reads each file once instead of twice.
fn build_tag_index(live: &LiveIndex) -> HashMap<String, Vec<String>> {
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    for (&doc_id, doc_tags) in live.content.tags_by_doc() {
        let path = live.table.path(doc_id as usize).to_string();
        for tag in doc_tags {
            tags.entry(tag.clone()).or_default().push(path.clone());
        }
    }
    for paths in tags.values_mut() {
        paths.sort();
    }
    tags
}

/// Creation time in epoch millis, falling back to modified time (some file
/// systems don't record creation) and finally to 0.
fn file_time_millis(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else { return 0 };
    meta.created()
        .or_else(|_| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Folder-containment graph — the one relation that's real today without
/// Phase 2 (parsers) or Phase 3 (suggestion engine). Real `links.jsonl`
/// relations slot in alongside this later without changing the frontend.
fn do_graph_data(live: &LiveIndex) -> GraphData {
    let mut nodes: HashMap<String, &'static str> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut seen_edges: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut times: HashMap<String, u64> = HashMap::new();

    nodes.insert(String::new(), "folder"); // vault root

    for path in live.table.paths() {
        let file_time = file_time_millis(&live.root().join(path));
        let parts: Vec<&str> = path.split('/').collect();
        let mut parent = String::new();
        for (i, part) in parts.iter().enumerate() {
            let is_file = i == parts.len() - 1;
            let current = if parent.is_empty() { part.to_string() } else { format!("{parent}/{part}") };
            nodes.entry(current.clone()).or_insert(if is_file { "file" } else { "folder" });

            // A folder's time is its earliest descendant's: in the timeline it
            // should appear exactly when its first file does, not at whatever
            // time the directory's own mtime happens to say.
            times
                .entry(current.clone())
                .and_modify(|t| *t = (*t).min(file_time))
                .or_insert(file_time);
            times
                .entry(parent.clone())
                .and_modify(|t| *t = (*t).min(file_time))
                .or_insert(file_time);

            let edge_key = (parent.clone(), current.clone());
            if seen_edges.insert(edge_key) {
                edges.push(GraphEdge { source: parent.clone(), target: current.clone(), rel: "contains".to_string() });
            }
            parent = current;
        }
    }

    // Real links — R2's auto-applied edges plus any accepted R1 suggestion
    // or manual link — alongside the folder structure above, these are the
    // graph's first non-`contains` edges. Two files because origin decides
    // location (18 §4.6): `.vault-ai/links.jsonl` is derived (R2, rebuilt
    // every rescan), `.vault/links.jsonl` is the irreproducible manual/
    // accepted-suggestion record.
    let vault = VaultLayout::new(live.root());
    let all_links = links::read_links(&vault.derived_links_path())
        .unwrap_or_default()
        .into_iter()
        .chain(links::read_links(&vault.links_path()).unwrap_or_default());
    for record in all_links {
        let (Ok(from_addr), Ok(to_addr)) = (addr::parse(&record.from), addr::parse(&record.to)) else { continue };
        if nodes.contains_key(&from_addr.path) && nodes.contains_key(&to_addr.path) {
            edges.push(GraphEdge { source: from_addr.path, target: to_addr.path, rel: record.rel });
        }
    }

    let node_list = nodes
        .into_iter()
        .map(|(id, kind)| {
            let label = id.rsplit('/').next().unwrap_or("(vault)").to_string();
            let ext = if kind == "file" {
                label.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
            } else {
                String::new()
            };
            let time = times.get(&id).copied().unwrap_or(0);
            GraphNode { id, label, kind, ext, time }
        })
        .collect();

    GraphData { nodes: node_list, edges }
}

// ---- thin tauri::command wrappers ----

#[tauri::command]
fn open_vault(app: tauri::AppHandle, state: State<AppState>, path: String) -> Result<usize, String> {
    let live = do_open_vault(&path)?;
    let count = live.table.paths().count();
    let tag_index = build_tag_index(&live);
    let root = live.root().to_path_buf();
    *state.vault.write().unwrap() = Some(live);
    *state.tags.lock().unwrap() = tag_index;
    let my_gen = state.watch_generation.fetch_add(1, Ordering::SeqCst) + 1;
    spawn_watcher(app, root, state.watch_generation.clone(), my_gen);
    Ok(count)
}

#[tauri::command]
fn search(state: State<AppState>, query: String, limit: Option<usize>) -> Result<SearchResults, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    Ok(do_search(live, &query, limit.unwrap_or(DEFAULT_SEARCH_LIMIT)))
}

fn do_list_suggestions(live: &LiveIndex) -> Vec<SuggestionView> {
    refresh_suggestions(live)
        .into_iter()
        .map(|c| SuggestionView { from: c.from, to: c.to, token: c.token, mention_count: c.mention_count })
        .collect()
}

#[tauri::command]
fn list_suggestions(state: State<AppState>) -> Result<Vec<SuggestionView>, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    Ok(do_list_suggestions(live))
}

/// Records the user's verdict (`decisions.jsonl`, so a rejected candidate
/// doesn't come back) and, on accept, writes the real link to
/// `.vault/links.jsonl` with `origin: "manual"` — 18 §4.6: an approved
/// suggestion counts as manual because the final judgment was the user's,
/// not the rule's guess.
fn do_decide_suggestion(live: &LiveIndex, from: &str, to: &str, verdict: &str) -> Result<(), String> {
    if verdict != "accept" && verdict != "reject" {
        return Err(format!("unknown verdict: {verdict}"));
    }
    let vault = VaultLayout::new(live.root());

    let decision = suggest_r1::Decision {
        key: suggest_r1::decision_key("R1", from, to),
        verdict: verdict.to_string(),
        rule: "R1".to_string(),
        from: from.to_string(),
        to: to.to_string(),
        ts: links::now_rfc3339(),
    };
    suggest_r1::append_decision(&vault.decisions_path(), &decision).map_err(|e| e.to_string())?;

    if verdict == "accept" {
        let record = links::LinkRecord {
            id: links::new_id(),
            op: "add".to_string(),
            from: from.to_string(),
            rel: "describes".to_string(),
            to: to.to_string(),
            origin: "manual".to_string(),
            confidence: "certain".to_string(),
            ts: links::now_rfc3339(),
        };
        links::append_manual_link(&vault.links_path(), &record).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn decide_suggestion(state: State<AppState>, from: String, to: String, verdict: String) -> Result<(), String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    do_decide_suggestion(live, &from, &to, &verdict)
}

#[tauri::command]
fn open_file(state: State<AppState>, path: String) -> Result<FileView, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    do_open_file(live, &path)
}

#[tauri::command]
fn read_file_base64(state: State<AppState>, path: String) -> Result<String, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    do_read_file_base64(live, &path)
}

#[tauri::command]
fn save_file(state: State<AppState>, path: String, content: String) -> Result<(), String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    do_save_file(live, &path, &content)
}

#[tauri::command]
fn list_files(state: State<AppState>) -> Result<Vec<String>, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    Ok(do_list_files(live))
}

#[tauri::command]
fn list_tags(state: State<AppState>) -> HashMap<String, Vec<String>> {
    state.tags.lock().unwrap().clone()
}

#[derive(Serialize, Debug)]
struct VaultStats {
    path: String,
    file_count: usize,
    indexed_docs: usize,
    tag_count: usize,
    path_table_bytes: usize,
    index_bytes: usize,
}

#[tauri::command]
fn vault_stats(state: State<AppState>) -> Result<VaultStats, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    Ok(VaultStats {
        path: live.root().display().to_string(),
        file_count: live.table.paths().count(),
        indexed_docs: live.content.indexed_docs(),
        tag_count: state.tags.lock().unwrap().len(),
        path_table_bytes: live.table.resident_bytes(),
        index_bytes: live.content.resident_bytes(),
    })
}

/// Reads `.vault/vault.toml` for the open vault (18 §7).
#[tauri::command]
fn get_vault_config(state: State<AppState>) -> Result<VaultConfig, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    config::read(live.root())
}

/// Writes `.vault/vault.toml` and rebuilds the index, because `[ignore]` and
/// `[limits]` change *what gets indexed* — saving without rebuilding would
/// leave the UI showing results the new settings exclude.
#[tauri::command]
fn save_vault_config(state: State<AppState>, config: VaultConfig) -> Result<usize, String> {
    let mut guard = state.vault.write().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    let root = live.root().to_path_buf();

    config::write(&root, &config)?;
    let rebuilt = LiveIndex::build_with_config(&root, &config).map_err(|e| e.to_string())?;
    let count = rebuilt.table.paths().count();
    let tag_index = build_tag_index(&rebuilt);
    refresh_derived_links(&rebuilt);
    *guard = Some(rebuilt);
    *state.tags.lock().unwrap() = tag_index;
    Ok(count)
}

#[tauri::command]
fn graph_data(state: State<AppState>) -> Result<GraphData, String> {
    let guard = state.vault.read().unwrap();
    let live = guard.as_ref().ok_or("no vault open")?;
    Ok(do_graph_data(live))
}

// ---- window management: launcher <-> main (Obsidian opens a vault in its
// own window and closes the launcher; same model here). Done in Rust so the
// vault index is loaded before the main window exists and can't race it. ----

const LAUNCHER_SIZE: (f64, f64) = (860.0, 560.0);
const MAIN_SIZE: (f64, f64) = (1200.0, 800.0);

#[tauri::command]
async fn open_vault_window(app: tauri::AppHandle, state: State<'_, AppState>, path: String) -> Result<(), String> {
    let live = do_open_vault(&path)?;
    let tag_index = build_tag_index(&live);
    let root = live.root().to_path_buf();
    *state.vault.write().unwrap() = Some(live);
    *state.tags.lock().unwrap() = tag_index;
    let my_gen = state.watch_generation.fetch_add(1, Ordering::SeqCst) + 1;
    spawn_watcher(app.clone(), root, state.watch_generation.clone(), my_gen);

    if let Some(existing) = app.get_webview_window("main") {
        // The backend now holds a different vault, but this window's UI (tree,
        // tags, graph cache, open file) is all built from the old one. Reload
        // it so every bit of that per-vault state is rebuilt — focusing alone
        // would leave the user looking at the previous vault's contents.
        existing.eval("window.location.reload()").map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
    } else {
        tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("Polyglot")
            .inner_size(MAIN_SIZE.0, MAIN_SIZE.1)
            .build()
            .map_err(|e| e.to_string())?;
    }
    if let Some(launcher) = app.get_webview_window("launcher") {
        launcher.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn show_launcher(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("launcher") {
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, "launcher", tauri::WebviewUrl::App("launcher.html".into()))
        .title("Polyglot")
        .inner_size(LAUNCHER_SIZE.0, LAUNCHER_SIZE.1)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// Baked in at build time (build.rs) from `git rev-parse HEAD` — compared
// against GitHub's latest master commit on startup so the in-app popup can
// tell the user a newer build exists.
const BUILD_GIT_SHA: &str = env!("BUILD_GIT_SHA");
const UPDATE_REPO: &str = "knox9014/polyglot-local-vault";
// The app self-updates by rebuilding from source, so the repo root is just
// the crate dir two levels up (`.../desktop/src-tauri` -> repo root). Works
// for any checkout without hardcoding a machine-specific path.
const UPDATE_REPO_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..");

#[derive(Serialize, Clone)]
struct UpdateInfo {
    available: bool,
}

/// Shells out to `gh` (already authenticated on this machine) rather than
/// calling the GitHub REST API directly, since the repo is private and this
/// reuses the user's existing `gh auth login` instead of embedding a token.
#[tauri::command]
fn check_for_update() -> UpdateInfo {
    let remote_sha = std::process::Command::new("gh")
        .args([
            "api",
            &format!("repos/{UPDATE_REPO}/commits/master"),
            "--jq",
            ".sha",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    UpdateInfo {
        available: !remote_sha.is_empty() && remote_sha != BUILD_GIT_SHA,
    }
}

/// Spawns the update script detached, then exits — the running exe has to
/// release its file lock before the script can overwrite it, so this
/// process cannot wait around for the rebuild to finish.
#[tauri::command]
fn apply_update(app: tauri::AppHandle) -> Result<(), String> {
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-File",
            &format!(r"{UPDATE_REPO_DIR}\desktop\check-and-update.ps1"),
        ])
        .current_dir(UPDATE_REPO_DIR)
        .spawn()
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            vault: RwLock::new(None),
            tags: Mutex::new(HashMap::new()),
            watch_generation: Arc::new(AtomicU64::new(0)),
        })
        .invoke_handler(tauri::generate_handler![
            open_vault,
            search,
            list_suggestions,
            decide_suggestion,
            open_file,
            save_file,
            read_file_base64,
            list_files,
            list_tags,
            graph_data,
            vault_stats,
            get_vault_config,
            save_vault_config,
            open_vault_window,
            show_launcher,
            check_for_update,
            apply_update
        ])
        .setup(|app| {
            // Fire-and-forget: a stale/offline `gh` call just means no popup,
            // never a startup delay or failure.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let info = check_for_update();
                if info.available {
                    let _ = handle.emit("update-available", ());
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("desktop-lib-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_vault_rejects_non_directory() {
        match do_open_vault("C:/definitely/not/a/real/path/xyz") {
            Err(msg) => assert!(msg.contains("not a directory")),
            Ok(_) => panic!("expected an error for a nonexistent path"),
        }
    }

    #[test]
    fn search_returns_both_kinds_of_hit() {
        let dir = temp_dir("search");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/router.py"), "def route_request(): pass").unwrap();
        fs::write(dir.join("readme.md"), "unrelated").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let results = do_search(&live, "router", 20);
        assert!(
            results.filename_hits.iter().any(|h| h.path == "src/router.py"),
            "expected a filename hit for router.py: {results:?}"
        );

        let content_results = do_search(&live, "route_request", 20);
        assert!(
            content_results.content_hits.iter().any(|h| h.path == "src/router.py"),
            "expected a content hit for route_request: {content_results:?}"
        );

        let symbol_results = do_search(&live, "route_request", 20);
        assert!(
            symbol_results.symbol_hits.iter().any(|h| {
                h.address == "vault://src/router.py#route_request" && h.name == "route_request" && h.node_type == "function"
            }),
            "expected a symbol hit for the route_request function: {symbol_results:?}"
        );

        let empty = do_search(&live, "", 20);
        assert!(empty.filename_hits.is_empty() && empty.symbol_hits.is_empty() && empty.content_hits.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_file_reads_relative_to_vault_root() {
        let dir = temp_dir("open-file");
        fs::write(dir.join("a.txt"), "hello vault").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let view = do_open_file(&live, "a.txt").unwrap();
        assert_eq!(view.content, "hello vault");
        assert!(view.frontmatter.is_empty());
        assert!(do_open_file(&live, "missing.txt").is_err());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_open_file_rejects_a_path_that_escapes_the_vault() {
        let dir = temp_dir("open-file-escape");
        fs::write(dir.parent().unwrap().join("secret-outside.txt"), "leak me").unwrap();
        fs::write(dir.join("a.txt"), "hello vault").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        assert!(do_open_file(&live, "../secret-outside.txt").is_err());
        assert!(do_open_file(&live, "/etc/passwd").is_err());

        let _ = fs::remove_file(dir.parent().unwrap().join("secret-outside.txt"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_save_file_writes_and_overwrites_a_vault_file() {
        let dir = temp_dir("save-file");
        fs::write(dir.join("a.txt"), "before").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        do_save_file(&live, "a.txt", "after").unwrap();
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "after");

        // a brand new file is fine too — the editor's "new file" path
        do_save_file(&live, "new/nested.md", "# hi").unwrap();
        assert_eq!(fs::read_to_string(dir.join("new/nested.md")).unwrap(), "# hi");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_read_file_base64_round_trips_binary_content() {
        let dir = temp_dir("read-base64");
        // PNG magic bytes — not a real image, just needs to be non-UTF8
        // binary so this proves the path doesn't go through read_to_string.
        let bytes: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00, 0xAB];
        fs::write(dir.join("pic.png"), &bytes).unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let encoded = do_read_file_base64(&live, "pic.png").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(decoded, bytes);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_read_file_base64_rejects_a_path_that_escapes_the_vault() {
        let dir = temp_dir("read-base64-escape");
        fs::write(dir.join("a.png"), [1, 2, 3]).unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        assert!(do_read_file_base64(&live, "../outside.png").is_err());
        assert!(do_read_file_base64(&live, "/etc/passwd").is_err());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_save_file_rejects_a_path_that_escapes_the_vault() {
        let dir = temp_dir("save-file-escape");
        fs::write(dir.join("a.txt"), "x").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        assert!(do_save_file(&live, "../outside.txt", "pwned").is_err());
        assert!(do_save_file(&live, "/etc/passwd", "pwned").is_err());
        assert!(!dir.parent().unwrap().join("outside.txt").exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_file_parses_frontmatter_and_keeps_full_content() {
        let dir = temp_dir("frontmatter");
        fs::write(dir.join("note.md"), "---\ntitle: Hello World\nstatus: draft\n---\nbody text here").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let view = do_open_file(&live, "note.md").unwrap();
        assert_eq!(
            view.frontmatter,
            vec![("title".to_string(), "Hello World".to_string()), ("status".to_string(), "draft".to_string())]
        );
        assert!(view.content.contains("body text here"), "content must still include the raw file, not just the parsed part");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tag_index_maps_tags_to_the_files_that_contain_them() {
        let dir = temp_dir("tag-index");
        fs::write(dir.join("a.md"), "about #project").unwrap();
        fs::write(dir.join("b.md"), "also #project and #urgent").unwrap();
        fs::write(dir.join("c.md"), "no tags here").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let index = build_tag_index(&live);
        let mut project_files = index.get("project").cloned().unwrap_or_default();
        project_files.sort();
        assert_eq!(project_files, vec!["a.md", "b.md"]);
        assert_eq!(index.get("urgent").cloned(), Some(vec!["b.md".to_string()]));
        assert!(!index.contains_key("notatag"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn graph_data_builds_containment_edges_from_paths() {
        let dir = temp_dir("graph");
        fs::create_dir_all(dir.join("src/nested")).unwrap();
        fs::write(dir.join("src/nested/deep.py"), "").unwrap();
        fs::write(dir.join("top.md"), "").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let graph = do_graph_data(&live);

        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&""), "vault root node must exist");
        assert!(ids.contains(&"src"));
        assert!(ids.contains(&"src/nested"));
        assert!(ids.contains(&"src/nested/deep.py"));
        assert!(ids.contains(&"top.md"));

        let has_edge = |s: &str, t: &str| graph.edges.iter().any(|e| e.source == s && e.target == t && e.rel == "contains");
        assert!(has_edge("", "src"));
        assert!(has_edge("src", "src/nested"));
        assert!(has_edge("src/nested", "src/nested/deep.py"));
        assert!(has_edge("", "top.md"));

        let deep_node = graph.nodes.iter().find(|n| n.id == "src/nested/deep.py").unwrap();
        assert_eq!(deep_node.kind, "file");
        assert_eq!(deep_node.label, "deep.py");
        assert_eq!(deep_node.ext, "py", "frontend colors nodes by this");
        let folder_node = graph.nodes.iter().find(|n| n.id == "src").unwrap();
        assert_eq!(folder_node.kind, "folder");
        assert_eq!(folder_node.ext, "", "folders have no language color");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn graph_data_includes_r2_links_alongside_containment() {
        let dir = temp_dir("graph-r2");
        fs::write(dir.join("lint.py"), "pass").unwrap();
        fs::write(dir.join("pyproject.toml"), "[tool]\nscript = \"lint.py\"\n").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        // R2 runs on the watcher thread now, not inside do_open_vault (it's
        // off the window-open critical path) — so drive it explicitly here.
        refresh_derived_links(&live);
        let graph = do_graph_data(&live);

        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.source == "pyproject.toml" && e.target == "lint.py" && e.rel == "references"),
            "R2's auto-applied link must show up as a real graph edge: {:?}",
            graph.edges
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn graph_data_includes_resolved_import_edges() {
        let dir = temp_dir("graph-imports");
        fs::write(dir.join("utils.py"), "x = 1\n").unwrap();
        fs::write(dir.join("main.py"), "from .utils import x\n").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        refresh_derived_links(&live); // see the R2 test above for why this is explicit here
        let graph = do_graph_data(&live);

        assert!(
            graph.edges.iter().any(|e| e.source == "main.py" && e.target == "utils.py" && e.rel == "imports"),
            "a resolved Python import must show up as a real graph edge: {:?}",
            graph.edges
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn do_list_suggestions_surfaces_an_r1_candidate() {
        let dir = temp_dir("suggestions-list");
        fs::write(dir.join("router.py"), "class TeacherRouter:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "See `TeacherRouter` for details.\n").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        let suggestions = do_list_suggestions(&live);

        assert_eq!(
            suggestions,
            vec![SuggestionView {
                from: "vault://docs.md".into(),
                to: "vault://router.py".into(),
                token: "TeacherRouter".into(),
                mention_count: 1,
            }]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn accepting_a_suggestion_writes_a_manual_link_and_it_shows_in_the_graph() {
        let dir = temp_dir("suggestions-accept");
        fs::write(dir.join("router.py"), "class TeacherRouter:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "See `TeacherRouter` for details.\n").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        let suggestion = &do_list_suggestions(&live)[0];

        do_decide_suggestion(&live, &suggestion.from, &suggestion.to, "accept").unwrap();

        // the accepted candidate must not be offered again
        assert!(do_list_suggestions(&live).is_empty());

        // and it must now be a real edge in the graph
        let graph = do_graph_data(&live);
        assert!(
            graph.edges.iter().any(|e| e.source == "docs.md" && e.target == "router.py" && e.rel == "describes"),
            "accepted suggestion must appear as a graph edge: {:?}",
            graph.edges
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejecting_a_suggestion_removes_it_and_does_not_create_a_link() {
        let dir = temp_dir("suggestions-reject");
        fs::write(dir.join("router.py"), "class TeacherRouter:\n    pass\n").unwrap();
        fs::write(dir.join("docs.md"), "See `TeacherRouter` for details.\n").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        let suggestion = &do_list_suggestions(&live)[0];

        do_decide_suggestion(&live, &suggestion.from, &suggestion.to, "reject").unwrap();

        assert!(do_list_suggestions(&live).is_empty(), "a rejected candidate must not reappear");
        let graph = do_graph_data(&live);
        assert!(!graph.edges.iter().any(|e| e.rel == "describes"), "rejecting must not create a link");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn decide_suggestion_rejects_an_unknown_verdict() {
        let dir = temp_dir("suggestions-bad-verdict");
        fs::write(dir.join("a.py"), "pass").unwrap();
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        let err = do_decide_suggestion(&live, "vault://docs.md", "vault://a.py", "maybe").unwrap_err();
        assert!(err.contains("maybe"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_limit_is_honored_and_clamped() {
        let dir = temp_dir("search-limit");
        for i in 0..40 {
            fs::write(dir.join(format!("match{i}.txt")), "shared body word").unwrap();
        }
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();

        assert_eq!(do_search(&live, "match", 5).filename_hits.len(), 5);
        assert_eq!(
            do_search(&live, "match", 30).filename_hits.len(),
            30,
            "raising the limit must actually return more — it used to be pinned at the backend default"
        );
        // absurd values are clamped, not honored, since the value comes from the UI
        assert!(do_search(&live, "match", usize::MAX).filename_hits.len() <= MAX_SEARCH_LIMIT);
        assert_eq!(do_search(&live, "match", 0).filename_hits.len(), 1, "0 clamps up to 1");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn graph_nodes_carry_times_and_folders_take_their_earliest_child() {
        let dir = temp_dir("graph-time");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/first.py"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        fs::write(dir.join("src/second.py"), "").unwrap();

        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        let graph = do_graph_data(&live);
        let time_of = |id: &str| graph.nodes.iter().find(|n| n.id == id).unwrap().time;

        let first = time_of("src/first.py");
        let second = time_of("src/second.py");
        assert!(first > 0 && second > 0, "files must carry a real timestamp for the timeline");
        assert_eq!(
            time_of("src"),
            first.min(second),
            "a folder must appear when its earliest file does"
        );
        assert_eq!(time_of(""), first.min(second), "vault root too");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vault_config_ignore_patterns_actually_change_what_is_indexed() {
        let dir = temp_dir("config-applies");
        // Deliberately *not* `build/`, `node_modules/`, etc. — those are in
        // the default excludes now, which would make step one below pass for
        // the wrong reason. This test is about a user-supplied pattern
        // taking effect; `scan.rs` covers the defaults separately.
        fs::create_dir_all(dir.join("scratch")).unwrap();
        fs::write(dir.join("scratch/generated.py"), "scratchartifact").unwrap();
        fs::write(dir.join("real.py"), "realcode").unwrap();

        // default config: both files are indexed
        let live = do_open_vault(dir.to_str().unwrap()).unwrap();
        assert_eq!(live.table.paths().count(), 2);
        assert_eq!(do_search(&live, "scratchartifact", 20).content_hits.len(), 1);

        // exclude scratch/ via vault.toml, rebuild the way save_vault_config does
        let mut cfg = config::VaultConfig::default();
        cfg.ignore.patterns = vec!["scratch/".to_string()];
        config::write(&dir, &cfg).unwrap();

        let live = LiveIndex::build(&dir).unwrap(); // re-reads vault.toml from disk
        let paths: Vec<String> = live.table.paths().map(str::to_string).collect();
        assert_eq!(paths, vec!["real.py"], "excluded path must be gone: {paths:?}");
        assert!(
            do_search(&live, "scratchartifact", 20).content_hits.is_empty(),
            "content from an excluded file must not remain searchable"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn vault_config_content_limit_actually_skips_large_files() {
        let dir = temp_dir("config-limit");
        fs::write(dir.join("small.txt"), "tinyword").unwrap();
        fs::write(dir.join("big.txt"), format!("bigword {}", "x".repeat(2000))).unwrap();

        let mut cfg = config::VaultConfig::default();
        cfg.limits.content_bytes = 500; // below big.txt, above small.txt
        config::write(&dir, &cfg).unwrap();

        let live = LiveIndex::build(&dir).unwrap();
        assert_eq!(do_search(&live, "tinyword", 20).content_hits.len(), 1);
        assert!(
            do_search(&live, "bigword", 20).content_hits.is_empty(),
            "a file over [limits] content_bytes must not be content-indexed"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
