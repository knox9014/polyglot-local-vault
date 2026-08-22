//! JSON `ParserAdapter` (06 "JSON": object/array/key/value + JSON Pointer).
//! Every key path becomes a node addressed by its RFC 6901 pointer
//! (`config.json#/router/threshold`) — `SymbolIndex::address` recognizes an
//! id starting with `/` as a pointer fragment and uses it as-is (`18` §1.5:
//! pointer fragments carry their own `/` separators, they don't go through
//! the `#`/`%`-escaping the qualname/heading fragments use).
//!
//! Byte ranges aren't tracked per-key (`serde_json::Value` doesn't carry
//! source spans) — `range` is the whole-file range for every node. Good
//! enough for "this key exists, here's its address"; upgrade to a
//! span-tracking JSON parser if per-key ranges turn out to matter.

use serde_json::Value;

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct JsonAdapter;

impl ParserAdapter for JsonAdapter {
    fn extensions(&self) -> &[&str] {
        &["json"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes).into_owned();
        let mut nodes = Vec::new();
        let partial = match serde_json::from_str::<Value>(&text) {
            Ok(value) => {
                walk(&value, String::new(), 0..text.len(), &mut nodes);
                false
            }
            Err(_) => true,
        };

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput { nodes, edges: Vec::<Edge>::new(), searchable_text: text, sketches, partial, imports: Vec::new() }
    }
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
}

/// `~` -> `~0`, `/` -> `~1`, per RFC 6901 — a key containing a literal `/`
/// (e.g. a path used as a JSON key) must not be read as another path segment.
fn escape_pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn walk(value: &Value, pointer: String, range: std::ops::Range<usize>, out: &mut Vec<Node>) {
    if !pointer.is_empty() {
        out.push(Node {
            id: pointer.clone(),
            node_type: kind_name(value).to_string(),
            name: pointer.rsplit('/').next().unwrap_or("").to_string(),
            source: value.to_string(),
            range: range.clone(),
        });
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                walk(v, format!("{pointer}/{}", escape_pointer_segment(k)), range.clone(), out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, format!("{pointer}/{i}"), range.clone(), out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        JsonAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_nested_key_paths_as_pointers() {
        let out = parse(r#"{"router": {"threshold": 0.4, "enabled": true}}"#);
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"/router"));
        assert!(ids.contains(&"/router/threshold"));
        assert!(ids.contains(&"/router/enabled"));
        assert!(!out.partial);
    }

    #[test]
    fn array_indices_become_numeric_segments() {
        let out = parse(r#"{"tags": ["a", "b"]}"#);
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"/tags/0"));
        assert!(ids.contains(&"/tags/1"));
    }

    #[test]
    fn key_containing_a_slash_is_rfc6901_escaped() {
        let out = parse(r#"{"a/b": 1}"#);
        assert!(out.nodes.iter().any(|n| n.id == "/a~1b"));
    }

    #[test]
    fn invalid_json_is_marked_partial_with_no_nodes() {
        let out = parse("{not valid json");
        assert!(out.partial);
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn pointer_id_round_trips_through_a_real_vault_address() {
        let out = parse(r#"{"router": {"threshold": 0.4}}"#);
        let node = out.nodes.iter().find(|n| n.id == "/router/threshold").unwrap();
        let full = format!("vault://config/model.json#{}", node.id);
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Pointer("/router/threshold".into())));
    }
}
