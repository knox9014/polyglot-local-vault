//! Jupyter Notebook `ParserAdapter` (06 "Jupyter Notebook": "Python code
//! cell은 Python 파서로 재분석"). Cells themselves aren't nodes — like CSV
//! rows, a cell materializes on link via `#cell:N` (always valid by index,
//! `04_VAULT_AND_DATA_MODEL.md`) — but a code cell's *symbols* are, so code
//! cells get concatenated and handed to `PythonAdapter`, markdown cells to
//! `MarkdownAdapter` for their headings.
//!
//! `range` on the resulting nodes is relative to the concatenated pseudo-source
//! this builds, not to byte offsets in the actual `.ipynb` JSON — there's no
//! single meaningful byte range in a notebook file for "this function", since
//! the source is scattered across a JSON array of strings. Good enough for
//! "this symbol exists, here's its address"; a cell-aware range needs the
//! notebook's own cell boundaries threaded through, not just Python's.

use serde_json::Value;

use crate::parser::{Node, ParseInput, ParseOutput, ParserAdapter};
use crate::parser_markdown::MarkdownAdapter;
use crate::parser_python::PythonAdapter;

pub struct IpynbAdapter;

impl ParserAdapter for IpynbAdapter {
    fn extensions(&self) -> &[&str] {
        &["ipynb"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let Ok(notebook) = serde_json::from_slice::<Value>(input.bytes) else {
            return ParseOutput { partial: true, ..ParseOutput::default() };
        };
        let cells = notebook.get("cells").and_then(Value::as_array).cloned().unwrap_or_default();

        let mut python_source = String::new();
        let mut markdown_source = String::new();
        for cell in &cells {
            let source = cell_source(cell);
            match cell.get("cell_type").and_then(Value::as_str) {
                Some("code") => {
                    python_source.push_str(&source);
                    python_source.push('\n');
                }
                Some("markdown") => {
                    markdown_source.push_str(&source);
                    markdown_source.push('\n');
                }
                _ => {}
            }
        }

        let py_out = PythonAdapter.parse(ParseInput { bytes: python_source.as_bytes(), previous: None });
        let md_out = MarkdownAdapter.parse(ParseInput { bytes: markdown_source.as_bytes(), previous: None });

        let mut nodes: Vec<Node> = py_out.nodes;
        nodes.extend(md_out.nodes);
        let mut sketches = py_out.sketches;
        sketches.extend(md_out.sketches);

        ParseOutput {
            nodes,
            edges: Vec::new(),
            searchable_text: String::from_utf8_lossy(input.bytes).into_owned(),
            sketches,
            partial: py_out.partial,
            imports: py_out.imports,
        }
    }
}

/// A cell's `source` is either one string or an array of lines (both are
/// valid per the notebook format — editors differ on which they write).
fn cell_source(cell: &Value) -> String {
    match cell.get("source") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(""),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn notebook(cells_json: &str) -> String {
        format!(r#"{{"cells": {cells_json}, "nbformat": 4}}"#)
    }

    #[test]
    fn extracts_symbols_from_code_cells() {
        let src = notebook(r##"[
            {"cell_type": "markdown", "source": ["# Intro\n"]},
            {"cell_type": "code", "source": "def load():\n    pass\n"}
        ]"##);
        let out = IpynbAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None });

        assert!(out.nodes.iter().any(|n| n.id == "load" && n.node_type == "function"));
        assert!(out.nodes.iter().any(|n| n.id == "intro" && n.node_type == "heading"));
        assert!(!out.partial);
    }

    #[test]
    fn source_as_array_of_lines_is_joined() {
        let src = notebook(r#"[{"cell_type": "code", "source": ["def f():\n", "    pass\n"]}]"#);
        let out = IpynbAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None });
        assert!(out.nodes.iter().any(|n| n.id == "f"));
    }

    #[test]
    fn invalid_notebook_json_is_marked_partial() {
        let out = IpynbAdapter.parse(ParseInput { bytes: b"not json", previous: None });
        assert!(out.partial);
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn symbol_id_round_trips_through_a_real_vault_address() {
        let src = notebook(r#"[{"cell_type": "code", "source": "def load():\n    pass\n"}]"#);
        let out = IpynbAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None });
        let node = out.nodes.iter().find(|n| n.id == "load").unwrap();

        let full = format!("vault://experiments/run.ipynb#{}", addr::encode_symbol_name(&node.id));
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["load".into()])));
    }
}
