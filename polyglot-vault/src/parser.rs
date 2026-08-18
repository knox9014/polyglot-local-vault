//! Parser Adapter interface — the extension point Phase 2 fills with
//! per-language backends (Tree-sitter for Python/Go/TS/Rust, plus Markdown/
//! JSON/CSV/Notebook readers). Phase 0 only needs this to type-check so
//! `store`/`reconcile` can be written against a stable shape; no backend
//! is implemented here.
//! Spec: `docs/design/06_POLYGLOT_PARSERS.md` "Parser Adapter Interface".

use std::collections::HashMap;
use std::ops::Range;

pub struct ParseInput<'a> {
    pub bytes: &'a [u8],
    /// The previous parse output, for incremental reparsing. `None` on first parse.
    pub previous: Option<&'a ParseOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub node_type: String,
    pub name: String,
    pub source: String,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub rel: String,
    pub to: String,
    pub origin: String,
    pub confidence: String,
}

/// A 32-entry minhash sketch over 3-gram token shingles of a symbol's body
/// (crc32-based). Feeds address resolution stair S4 (18 §3.2). The parser
/// computes it because it already knows each symbol's source range — no
/// separate pass needed.
pub type ShingleSketch = [u32; 32];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseOutput {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub searchable_text: String,
    /// Keyed by `Node::id`, symbols only.
    pub sketches: HashMap<String, ShingleSketch>,
    /// True when a syntax error left the file only partially parsed — the
    /// indexer must merge, not replace, this file's existing symbols (06).
    pub partial: bool,
}

pub trait ParserAdapter {
    /// File extensions this adapter handles, e.g. `["py"]`.
    fn extensions(&self) -> &[&str];
    fn parse(&self, input: ParseInput<'_>) -> ParseOutput;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only check: the trait must actually be implementable and
    /// object-safe (`&dyn ParserAdapter`) — that's Phase 0's whole bar here.
    struct NoOpAdapter;

    impl ParserAdapter for NoOpAdapter {
        fn extensions(&self) -> &[&str] {
            &["noop"]
        }

        fn parse(&self, _input: ParseInput<'_>) -> ParseOutput {
            ParseOutput::default()
        }
    }

    #[test]
    fn adapter_is_object_safe_and_callable() {
        let adapter: &dyn ParserAdapter = &NoOpAdapter;
        assert_eq!(adapter.extensions(), &["noop"]);

        let output = adapter.parse(ParseInput { bytes: b"", previous: None });
        assert_eq!(output, ParseOutput::default());
    }
}
