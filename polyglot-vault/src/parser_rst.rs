//! reStructuredText / plain-text `ParserAdapter` (06 "문서 (.md / .rst /
//! .txt)"). `.txt` gets the *same* adapter as `.rst`, not a separate one —
//! 06's own reasoning for including `.txt` at all is that django ships its
//! docs as RST-formatted `.txt` files, so the syntax to extract from a
//! `.txt` file is RST's, not some plain-text heading convention of its own.
//!
//! RST headings are underline-only detected here (a title line followed by
//! a line of one repeated punctuation character, `===`/`---`/`~~~`/...).
//! Overline+underline (used for the very top title) also satisfies this —
//! the overline is just an ordinary text line to this scanner and is
//! skipped, the following underline still marks the title. Sphinx-role
//! extraction is deferred for the same reason as `parser_markdown`'s links:
//! it needs vault-wide resolution, which belongs to Phase 3.

use crate::addr::{dedupe_slugs, slugify_heading};
use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct RstAdapter;

impl ParserAdapter for RstAdapter {
    fn extensions(&self) -> &[&str] {
        &["rst", "txt"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes);
        let headings = extract_headings(&text);

        let slugs = dedupe_slugs(headings.iter().map(|h| slugify_heading(&h.text)));
        let nodes: Vec<Node> = headings
            .into_iter()
            .zip(slugs)
            .map(|(h, slug)| Node {
                id: slug,
                node_type: "heading".to_string(),
                name: h.text,
                source: h.source,
                range: h.range,
            })
            .collect();

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput {
            nodes,
            edges: Vec::<Edge>::new(),
            searchable_text: text.into_owned(),
            sketches,
            partial: false,
            imports: Vec::new(),
        }
    }
}

struct Heading {
    text: String,
    source: String,
    range: std::ops::Range<usize>,
}

const UNDERLINE_CHARS: &[char] = &['=', '-', '~', '^', '"', '\'', '#', '*', '+', '.', ':', '_'];

fn is_underline(line: &str) -> bool {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    !trimmed.is_empty()
        && trimmed.chars().all(|c| UNDERLINE_CHARS.contains(&c))
        && trimmed.chars().all(|c| c == trimmed.chars().next().unwrap())
}

fn extract_headings(text: &str) -> Vec<Heading> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut headings = Vec::new();
    let mut offset = 0;
    let mut line_offsets = Vec::with_capacity(lines.len());
    for line in &lines {
        line_offsets.push(offset);
        offset += line.len();
    }

    for i in 0..lines.len() {
        let title = lines[i].trim_end_matches(['\n', '\r']);
        if title.trim().is_empty() {
            continue;
        }
        let Some(next) = lines.get(i + 1) else { continue };
        if !is_underline(next) {
            continue;
        }
        let underline_len = next.trim_end_matches(['\n', '\r']).chars().count();
        if underline_len < title.trim().chars().count() {
            continue; // RST requires the underline to be at least as long as the title
        }
        let start = line_offsets[i];
        let end = line_offsets[i + 1] + lines[i + 1].len();
        headings.push(Heading {
            text: title.trim().to_string(),
            source: format!("{title}\n{}", next.trim_end_matches(['\n', '\r'])),
            range: start..end,
        });
    }
    headings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        RstAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_underline_style_headings() {
        let src = "Teacher Router\n==============\n\nSome body text.\n\nSetup\n-----\n";
        let out = parse(src);

        let names: Vec<&str> = out.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Teacher Router", "Setup"]);
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["teacher-router", "setup"]);
    }

    #[test]
    fn overline_and_underline_title_still_detected_by_its_underline() {
        let src = "==============\nTeacher Router\n==============\n";
        let out = parse(src);
        assert_eq!(out.nodes.len(), 1);
        assert_eq!(out.nodes[0].name, "Teacher Router");
    }

    #[test]
    fn short_underline_is_not_a_heading() {
        // underline shorter than the title text — not valid RST, must not match
        let src = "A Much Longer Title\n---\n";
        let out = parse(src);
        assert!(out.nodes.is_empty());
    }

    #[test]
    fn applies_to_txt_extension_too() {
        assert!(RstAdapter.extensions().contains(&"txt"));
        assert!(RstAdapter.extensions().contains(&"rst"));
    }

    #[test]
    fn heading_id_round_trips_through_a_real_vault_address() {
        let out = parse("Teacher Router\n==============\n");
        let full = format!("vault://docs/architecture.rst#h:{}", addr::encode_segment(&out.nodes[0].id));
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Heading("teacher-router".into())));
    }
}
