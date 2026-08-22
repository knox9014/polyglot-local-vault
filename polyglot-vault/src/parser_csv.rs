//! CSV `ParserAdapter` (06 "CSV": header/schema/column — "행은 노드로
//! 만들지 않는다"). Rows materialize on link via `#row:N`, always valid by
//! index alone (no extraction needed, same policy as `.ipynb` cells) — only
//! the header row becomes addressable nodes (`#col:name`).
//!
//! Header splitting is a bare `,` split, not full RFC 4180 (quoted commas,
//! embedded newlines). Headers containing a literal comma are rare enough
//! that pulling in a CSV crate for this one line isn't worth it yet — add
//! one if a real corpus hits it.

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct CsvAdapter;

impl ParserAdapter for CsvAdapter {
    fn extensions(&self) -> &[&str] {
        &["csv"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes).into_owned();
        // Excel writes a UTF-8 BOM at the start of exported CSVs — left in,
        // it glues onto the first header name and that column never matches.
        let header_line = text.strip_prefix('\u{feff}').unwrap_or(&text).lines().next().unwrap_or("");

        let nodes: Vec<Node> = header_line
            .split(',')
            .map(|col| col.trim().trim_matches('"').to_string())
            .filter(|col| !col.is_empty())
            .map(|name| Node {
                id: name.clone(),
                node_type: "column".to_string(),
                name: name.clone(),
                source: name,
                range: 0..header_line.len(),
            })
            .collect();

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput { nodes, edges: Vec::<Edge>::new(), searchable_text: text, sketches, partial: false, imports: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        CsvAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_header_columns_only_not_rows() {
        let out = parse("name,label,score\nalice,1,0.9\nbob,0,0.2\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["name", "label", "score"]);
        assert!(out.nodes.iter().all(|n| n.node_type == "column"));
    }

    #[test]
    fn utf8_bom_does_not_glue_onto_the_first_header() {
        let out = parse("\u{feff}name,label\nalice,1\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["name", "label"]);
    }

    #[test]
    fn column_id_round_trips_through_a_real_vault_address() {
        let out = parse("label,score\n1,0.9\n");
        let node = &out.nodes[0];
        let full = format!("vault://data/train.csv#col:{}", addr::encode_segment(&node.id));
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Column("label".into())));
    }
}
