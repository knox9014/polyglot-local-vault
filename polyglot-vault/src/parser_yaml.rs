//! YAML `ParserAdapter` (06 "YAML": mapping/sequence/scalar/계층/경로),
//! same JSON-Pointer-style addressing as `parser_json`/`parser_toml`.

use serde_yaml::Value;

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct YamlAdapter;

impl ParserAdapter for YamlAdapter {
    fn extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes).into_owned();
        let mut nodes = Vec::new();
        let partial = match serde_yaml::from_str::<Value>(&text) {
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
        Value::Mapping(_) => "mapping",
        Value::Sequence(_) => "sequence",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Tagged(_) => "tagged",
    }
}

fn escape_pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// YAML permits non-string mapping keys (`42: foo`) — stringify whatever's
/// there rather than dropping the entry, since dropping it would silently
/// make that value unaddressable.
fn key_to_string(key: &Value) -> String {
    match key {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => serde_yaml::to_string(key).unwrap_or_default().trim().to_string(),
    }
}

fn walk(value: &Value, pointer: String, range: std::ops::Range<usize>, out: &mut Vec<Node>) {
    if !pointer.is_empty() {
        out.push(Node {
            id: pointer.clone(),
            node_type: kind_name(value).to_string(),
            name: pointer.rsplit('/').next().unwrap_or("").to_string(),
            source: serde_yaml::to_string(value).unwrap_or_default(),
            range: range.clone(),
        });
    }
    match value {
        Value::Mapping(map) => {
            for (k, v) in map {
                walk(v, format!("{pointer}/{}", escape_pointer_segment(&key_to_string(k))), range.clone(), out);
            }
        }
        Value::Sequence(items) => {
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
        YamlAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_nested_mapping_keys_as_pointers() {
        let out = parse("router:\n  threshold: 0.4\n  enabled: true\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"/router"));
        assert!(ids.contains(&"/router/threshold"));
        assert!(ids.contains(&"/router/enabled"));
        assert!(!out.partial);
    }

    #[test]
    fn sequence_indices_become_numeric_segments() {
        let out = parse("tags:\n  - a\n  - b\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"/tags/0"));
        assert!(ids.contains(&"/tags/1"));
    }

    #[test]
    fn invalid_yaml_is_marked_partial_with_no_nodes() {
        let out = parse("router: [unclosed\n  nested: :::\n");
        assert!(out.partial);
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn pointer_id_round_trips_through_a_real_vault_address() {
        let out = parse("router:\n  threshold: 0.4\n");
        let node = out.nodes.iter().find(|n| n.id == "/router/threshold").unwrap();
        let full = format!("vault://config/model.yaml#{}", node.id);
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Pointer("/router/threshold".into())));
    }
}
