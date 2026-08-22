//! Markdown `ParserAdapter` (06 "문서 (.md / .rst / .txt)").
//!
//! Extracts ATX headings (`# Title`) as addressable `heading` nodes — the
//! doc-side counterpart to code symbols, feeding the heading resolution
//! ladder (18 §3.3). Links / inline-code tokens / Sphinx roles are also
//! listed in 06 as extraction targets, but they're *relationship* data (who
//! points at what), and resolving "what" requires vault-wide symbol lookup
//! this single-file parser doesn't have — that's the suggestion engine's
//! job (Phase 3, `16_SUGGESTION_ENGINE.md`), not this adapter's. Building
//! unresolved edges now would be guessing at a shape Phase 3 hasn't fixed.

use crate::addr::{dedupe_slugs, slugify_heading};
use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct MarkdownAdapter;

impl ParserAdapter for MarkdownAdapter {
    fn extensions(&self) -> &[&str] {
        &["md"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let text = String::from_utf8_lossy(input.bytes);
        let headings = extract_headings(&text);

        // A heading's range is its own line through everything up to the
        // next heading of the same or shallower level (or EOF) — the whole
        // section body, not just the title line. `mcp::read`'s
        // heading-fragment path slices this range directly to answer "just
        // this section"; a title-only range would hand back the heading
        // text and nothing else (found via a real query for one section of
        // a multi-heading note returning just its "# Setup" line).
        let ranges: Vec<_> =
            (0..headings.len()).map(|i| headings[i].start..section_end(&headings, i, text.len())).collect();

        let slugs = dedupe_slugs(headings.iter().map(|h| slugify_heading(&h.text)));
        let nodes: Vec<Node> = headings
            .iter()
            .zip(ranges)
            .zip(slugs)
            .map(|((h, range), slug)| Node {
                id: slug,
                node_type: "heading".to_string(),
                name: h.text.clone(),
                source: text[range.clone()].to_string(),
                range,
            })
            .collect();

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput {
            nodes,
            edges: Vec::<Edge>::new(),
            searchable_text: text.into_owned(),
            sketches,
            partial: false, // line-based extraction always completes; no syntax-error concept here
            imports: Vec::new(),
        }
    }
}

struct Heading {
    text: String,
    level: usize,
    /// Byte offset where this heading's line begins.
    start: usize,
}

/// `# Title` through `###### Title`, skipping anything inside a fenced code
/// block (a lone `#` in a shell snippet isn't a heading).
fn extract_headings(text: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let stripped = trimmed.trim_start();

        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            if let Some(rest) = stripped.strip_prefix('#') {
                let hashes = 1 + rest.chars().take_while(|&c| c == '#').count();
                let after = &stripped[hashes..];
                if hashes <= 6 && after.starts_with(char::is_whitespace) {
                    let heading_text = after.trim().trim_end_matches('#').trim().to_string();
                    if !heading_text.is_empty() {
                        headings.push(Heading { text: heading_text, level: hashes, start: offset });
                    }
                }
            }
        }
        offset += line.len();
    }
    headings
}

/// End of heading `i`'s section: the start of the next heading at the same
/// or a shallower level (`##` ends at the next `#`/`##`, not at a nested
/// `###`), or the end of the file if there is none.
fn section_end(headings: &[Heading], i: usize, text_len: usize) -> usize {
    let level = headings[i].level;
    headings[i + 1..].iter().find(|h| h.level <= level).map_or(text_len, |h| h.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        MarkdownAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_headings_with_deduped_slugs() {
        let src = "# Teacher Router\n\nSome text.\n\n## Setup\n\n## Setup\n";
        let out = parse(src);

        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["teacher-router", "setup", "setup-2"]);
        assert!(out.nodes.iter().all(|n| n.node_type == "heading"));
    }

    #[test]
    fn ignores_hash_inside_fenced_code_block() {
        let src = "# Real Heading\n\n```bash\n# not a heading, just a comment\necho hi\n```\n\n## Also Real\n";
        let out = parse(src);
        let names: Vec<&str> = out.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Real Heading", "Also Real"]);
    }

    /// A heading's range must cover its whole section, not just the title
    /// line — `mcp::read` slices this range directly to answer "just this
    /// section", and a title-only range made that return almost nothing.
    #[test]
    fn heading_range_covers_its_section_not_just_the_title_line() {
        let src = "# Overview\n\nIntro text.\n\n# Setup\n\nRun `main.py` first.\n\n# Done\n\nThat's it.\n";
        let out = parse(src);

        let setup = out.nodes.iter().find(|n| n.id == "setup").unwrap();
        assert!(setup.source.contains("Run `main.py` first"), "{:?}", setup.source);
        assert!(!setup.source.contains("Intro text"), "must not bleed into the previous section: {:?}", setup.source);
        assert!(!setup.source.contains("That's it"), "must stop before the next section: {:?}", setup.source);
    }

    /// A `##` subsection ends at the next `##` or `#`, not at a nested `###`
    /// — the parent section's range must include its subsections' content.
    #[test]
    fn a_subsection_does_not_end_the_parent_sections_range() {
        let src = "## Parent\n\nParent text.\n\n### Child\n\nChild text.\n\n## Next\n\nNext text.\n";
        let out = parse(src);

        let parent = out.nodes.iter().find(|n| n.id == "parent").unwrap();
        assert!(parent.source.contains("Parent text"));
        assert!(parent.source.contains("Child text"), "a subsection is part of its parent's range: {:?}", parent.source);
        assert!(!parent.source.contains("Next text"), "must stop at the next sibling: {:?}", parent.source);
    }

    #[test]
    fn heading_id_round_trips_through_a_real_vault_address() {
        let out = parse("# Teacher Router\n");
        let heading = &out.nodes[0];
        let full = format!("vault://docs/architecture.md#h:{}", addr::encode_segment(&heading.id));
        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Heading("teacher-router".into())));
    }
}
