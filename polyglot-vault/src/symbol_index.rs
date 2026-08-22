//! Symbol index (2층 심볼, `05_FAST_LOCAL_SEARCH.md`) — parses files with a
//! registered `ParserAdapter` and makes their symbols addressable as a
//! full `vault://` address (18 §1~2) and searchable by substring on the id.
//!
//! Adding a format is: implement `ParserAdapter`, extend `adapter_for`,
//! nothing else here changes — including addressing, which dispatches on
//! the node's own `id` shape / `node_type`, not on which adapter made it.

use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::Path;

use crate::addr;
use crate::parser::{ParseInput, ParserAdapter, ShingleSketch};
use crate::parser_csv::CsvAdapter;
use crate::parser_go::GoAdapter;
use crate::parser_ipynb::IpynbAdapter;
use crate::parser_json::JsonAdapter;
use crate::parser_markdown::MarkdownAdapter;
use crate::parser_python::PythonAdapter;
use crate::parser_rst::RstAdapter;
use crate::parser_rust::RustAdapter;
use crate::parser_toml::TomlAdapter;
use crate::parser_ts::TypeScriptAdapter;
use crate::parser_yaml::YamlAdapter;

/// `pub(crate)` so `imports.rs` can reuse the same extension→adapter map
/// instead of re-listing the four code languages a second time.
pub(crate) fn adapter_for(ext: &str) -> Option<&'static dyn ParserAdapter> {
    match ext {
        "py" => Some(&PythonAdapter),
        "go" => Some(&GoAdapter),
        "rs" => Some(&RustAdapter),
        "ts" => Some(&TypeScriptAdapter),
        "md" => Some(&MarkdownAdapter),
        "rst" | "txt" => Some(&RstAdapter),
        "json" => Some(&JsonAdapter),
        "toml" => Some(&TomlAdapter),
        "yaml" | "yml" => Some(&YamlAdapter),
        "csv" => Some(&CsvAdapter),
        "ipynb" => Some(&IpynbAdapter),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub path: String, // vault-relative, forward slashes
    /// Dotted qualname for code symbols ("Config.load"), a heading slug, a
    /// column name, or an RFC 6901 JSON Pointer ("/router/threshold") — the
    /// shape depends on `node_type`, matched in `address()`.
    pub id: String,
    pub node_type: String,
    pub range: Range<usize>,
}

impl SymbolEntry {
    /// The full `vault://` address this entry resolves to (18 §1~2). A
    /// pointer's `id` already carries its own `/`-separated fragment syntax
    /// (RFC 6901), so it's used as-is instead of going through the
    /// `#`/`%`-escaping the qualname and heading fragments need.
    pub fn address(&self) -> String {
        let fragment = if self.id.starts_with('/') {
            self.id.clone()
        } else if self.node_type == "heading" {
            format!("h:{}", addr::encode_segment(&self.id))
        } else if self.node_type == "column" {
            format!("col:{}", addr::encode_segment(&self.id))
        } else {
            self.id.split('.').map(addr::encode_symbol_name).collect::<Vec<_>>().join(".")
        };
        format!("vault://{}#{}", self.path, fragment)
    }
}

#[derive(Default)]
pub struct SymbolIndex {
    entries: Vec<SymbolEntry>,
    /// Keyed by qualname (18 §4.4 sketches feed S4; the file-scoped id is
    /// enough at this stage since S1/S2 already require a matching qualname).
    sketches: HashMap<String, ShingleSketch>,
}

impl SymbolIndex {
    pub fn build(root: &Path, paths: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut index = SymbolIndex::default();
        for rel in paths {
            index.index_doc(root, rel.as_ref());
        }
        index
    }

    /// (Re)indexes one file: drops whatever symbols it had, then reparses if
    /// its extension has an adapter. Safe to call for an unsupported
    /// extension (silently indexes nothing) or a file that no longer exists.
    pub fn index_doc(&mut self, root: &Path, rel_path: &str) {
        self.remove_doc(rel_path);

        let Some(ext) = rel_path.rsplit_once('.').map(|(_, e)| e) else { return };
        let Some(adapter) = adapter_for(ext) else { return };
        let Ok(bytes) = fs::read(root.join(rel_path)) else { return };

        let output = adapter.parse(ParseInput { bytes: &bytes, previous: None });
        for node in output.nodes {
            if let Some(sketch) = output.sketches.get(&node.id) {
                self.sketches.insert(node.id.clone(), *sketch);
            }
            self.entries.push(SymbolEntry {
                path: rel_path.to_string(),
                id: node.id,
                node_type: node.node_type,
                range: node.range,
            });
        }
    }

    pub fn remove_doc(&mut self, rel_path: &str) {
        self.entries.retain(|e| e.path != rel_path);
    }

    /// Case-insensitive substring match on the qualname, returning `vault://`
    /// addresses — this is the type Phase 2's structural end condition
    /// checks for ("검색 결과가 vault:// 주소로 나온다").
    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        self.search_entries(query, limit).into_iter().map(SymbolEntry::address).collect()
    }

    /// Same match as `search`, but returns the entries themselves — callers
    /// that need to show more than the address (an icon by `node_type`, the
    /// bare symbol name, which file it's in) use this instead.
    pub fn search_entries(&self, query: &str, limit: usize) -> Vec<&SymbolEntry> {
        if query.is_empty() {
            return Vec::new();
        }
        let needle = query.to_lowercase();
        self.entries.iter().filter(|e| e.id.to_lowercase().contains(&needle)).take(limit).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All entries, for callers that scan the whole index rather than
    /// searching it (the suggestion engine's R1 needs every code symbol's
    /// bare name, not a query match).
    pub fn entries(&self) -> &[SymbolEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vault-symbol-index-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn search_returns_real_vault_addresses() {
        let dir = temp_dir("search");
        fs::write(dir.join("config.py"), "class Config:\n    def load(self):\n        pass\n").unwrap();

        let index = SymbolIndex::build(&dir, ["config.py"]);
        let hits = index.search("load", 10);

        assert_eq!(hits, vec!["vault://config.py#Config.load"]);
        // and it must parse back to exactly the symbol we found
        let parsed = addr::parse(&hits[0]).unwrap();
        assert_eq!(parsed.path, "config.py");
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["Config".into(), "load".into()])));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unsupported_extension_indexes_nothing_but_does_not_panic() {
        let dir = temp_dir("unsupported");
        fs::write(dir.join("image.png"), b"not actually a png, doesn't matter").unwrap();

        let index = SymbolIndex::build(&dir, ["image.png"]);
        assert!(index.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn re_indexing_a_path_replaces_rather_than_duplicates() {
        let dir = temp_dir("reindex");
        fs::write(dir.join("a.py"), "def one():\n    pass\n").unwrap();

        let mut index = SymbolIndex::build(&dir, ["a.py"]);
        assert_eq!(index.len(), 1);

        fs::write(dir.join("a.py"), "def two():\n    pass\n").unwrap();
        index.index_doc(&dir, "a.py");

        assert_eq!(index.len(), 1, "must not accumulate stale symbols from the old file content");
        assert_eq!(index.search("two", 10), vec!["vault://a.py#two"]);
        assert!(index.search("one", 10).is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_doc_drops_its_symbols() {
        let dir = temp_dir("remove");
        fs::write(dir.join("a.py"), "def gone():\n    pass\n").unwrap();

        let mut index = SymbolIndex::build(&dir, ["a.py"]);
        assert_eq!(index.len(), 1);
        index.remove_doc("a.py");
        assert!(index.is_empty());

        fs::remove_dir_all(&dir).unwrap();
    }
}
