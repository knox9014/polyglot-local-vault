//! TOML `ParserAdapter` (06 "TOML": table/array of tables/key-value +
//! JSON-Pointer-style path, same policy as `parser_json`). `.vault/vault.toml`
//! itself is TOML — this is what makes it addressable/searchable like any
//! other vault file, not special-cased.

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct TomlAdapter;

impl ParserAdapter for TomlAdapter {
    fn extensions(&self) -> &[&str] {
        &["toml"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes).into_owned();
        let mut nodes = Vec::new();
        // `str::parse` only accepts a bare value expression, not a full
        // document with `[table]` headers — `toml::from_str` (the serde
        // path) is what actually parses a whole file into a root Table.
        let partial = match toml::from_str::<toml::Value>(&text) {
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

fn kind_name(v: &toml::Value) -> &'static str {
    match v {
        toml::Value::Table(_) => "table",
        toml::Value::Array(_) => "array",
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "number",
        toml::Value::Float(_) => "number",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
    }
}

fn escape_pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn walk(value: &toml::Value, pointer: String, range: std::ops::Range<usize>, out: &mut Vec<Node>) {
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
        toml::Value::Table(map) => {
            for (k, v) in map {
                walk(v, format!("{pointer}/{}", escape_pointer_segment(k)), range.clone(), out);
            }
        }
        toml::Value::Array(items) => {
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
        TomlAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_nested_table_keys_as_pointers() {
        let out = parse("[router]\nthreshold = 0.4\nenabled = true\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"/router"));
        assert!(ids.contains(&"/router/threshold"));
        assert!(ids.contains(&"/router/enabled"));
        assert!(!out.partial);
    }

    #[test]
    fn invalid_toml_is_marked_partial_with_no_nodes() {
        let out = parse("this is not = = valid toml [[[");
        assert!(out.partial);
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn pointer_id_round_trips_through_a_real_vault_address() {
        let out = parse("[router]\nthreshold = 0.4\n");
        let node = out.nodes.iter().find(|n| n.id == "/router/threshold").unwrap();
        let full = format!("vault://.vault/vault.toml#{}", node.id);
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Pointer("/router/threshold".into())));
    }
}
