//! Python `ParserAdapter` — Tree-sitter backend (06 "파서 백엔드").
//! Extracts module/class/function/method symbols as a dotted qualname
//! (`Config.load`), the shape `addr::Fragment::Symbol` already expects.
//! Also extracts raw `import`/`from ... import` module specifiers
//! (`ParseOutput.imports`) — unresolved on purpose, see `parser.rs`.
//!
//! Only `class_definition` / `function_definition` are walked for symbols.
//! Decorators and variables are real extraction targets per 06 but aren't
//! needed for a symbol index to exist and address correctly — add them
//! when a caller actually needs them.

use tree_sitter::{Node as TsNode, Parser};

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct PythonAdapter;

impl ParserAdapter for PythonAdapter {
    fn extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_python::LANGUAGE.into()).expect("tree-sitter-python grammar");
        let Some(tree) = parser.parse(input.bytes, None) else {
            return ParseOutput { partial: true, ..ParseOutput::default() };
        };

        let mut nodes = Vec::new();
        walk(tree.root_node(), input.bytes, &[], &mut nodes);
        let mut imports = Vec::new();
        walk_imports(tree.root_node(), input.bytes, &mut imports);

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput {
            nodes,
            edges: Vec::<Edge>::new(), // call edges: not this slice (06 "call edge 정책" needs its own pass)
            searchable_text: String::from_utf8_lossy(input.bytes).into_owned(),
            sketches,
            partial: tree.root_node().has_error(),
            imports,
        }
    }
}

/// Recurses into every block (imports aren't always top-level — a
/// conditional or `try` import is common enough to bother with), collecting
/// each statement's raw module specifier as written:
///   `import os.path`        -> "os.path"
///   `import os.path as p`   -> "os.path"        (the alias isn't the target)
///   `from foo.bar import x` -> "foo.bar"
///   `from . import x`       -> "."              (relative_import's own text
///   `from ..pkg import y`   -> "..pkg"            already includes the dots)
fn walk_imports(node: TsNode, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                let mut names = child.walk();
                for name in child.children_by_field_name("name", &mut names) {
                    let target = if name.kind() == "aliased_import" { name.child_by_field_name("name") } else { Some(name) };
                    if let Some(t) = target
                        && let Ok(text) = t.utf8_text(src)
                    {
                        out.push(text.to_string());
                    }
                }
            }
            "import_from_statement" => {
                if let Some(module) = child.child_by_field_name("module_name")
                    && let Ok(text) = module.utf8_text(src)
                {
                    out.push(text.to_string());
                }
            }
            _ => {}
        }
        walk_imports(child, src, out);
    }
}

/// Recursively collects class/function symbols, threading `qualname_prefix`
/// so a method nested in a class gets `Class.method`, matching the fragment
/// grammar's dotted qualname (18 §1~2).
fn walk(node: TsNode, src: &[u8], qualname_prefix: &[String], out: &mut Vec<Node>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        if kind == "class_definition" || kind == "function_definition" {
            let Some(name_node) = child.child_by_field_name("name") else { continue };
            let name = name_node.utf8_text(src).unwrap_or("").to_string();

            let mut qualname_parts = qualname_prefix.to_vec();
            qualname_parts.push(name.clone());

            let node_type = if kind == "class_definition" {
                "class"
            } else if qualname_prefix.is_empty() {
                "function"
            } else {
                "method"
            };

            out.push(Node {
                id: qualname_parts.join("."),
                node_type: node_type.to_string(),
                name,
                source: child.utf8_text(src).unwrap_or("").to_string(),
                range: child.byte_range(),
            });

            // Descend so nested classes/methods are found too — a
            // `function_definition`'s body can itself contain one (a
            // closure), which is a legitimate nested symbol.
            walk(child, src, &qualname_parts, out);
        } else {
            walk(child, src, qualname_prefix, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        PythonAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_module_function_and_class_method_as_dotted_qualnames() {
        let src = "def route_request():\n    pass\n\nclass Config:\n    def load(self):\n        pass\n";
        let out = parse(src);

        let ids: Vec<(&str, &str)> = out.nodes.iter().map(|n| (n.id.as_str(), n.node_type.as_str())).collect();
        assert!(ids.contains(&("route_request", "function")));
        assert!(ids.contains(&("Config", "class")));
        assert!(ids.contains(&("Config.load", "method")));
        assert!(!out.partial);
    }

    #[test]
    fn symbol_id_round_trips_through_a_real_vault_address() {
        let out = parse("class Config:\n    def load(self):\n        pass\n");
        let method = out.nodes.iter().find(|n| n.id == "Config.load").unwrap();

        let fragment = method
            .id
            .split('.')
            .map(addr::encode_symbol_name)
            .collect::<Vec<_>>()
            .join(".");
        let full = format!("vault://src/config.py#{fragment}");

        let parsed = addr::parse(&full).unwrap();
        assert_eq!(parsed.path, "src/config.py");
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["Config".into(), "load".into()])));
    }

    #[test]
    fn syntax_error_marks_output_partial_but_keeps_recoverable_symbols() {
        // Unterminated function def, mid-typing — the exact case 06 measures
        // Tree-sitter against CPython `ast` on (99.9% vs 0.0% symbol survival).
        let out = parse("def process_");
        assert!(out.partial);
    }

    #[test]
    fn every_symbol_gets_a_shingle_sketch() {
        let out = parse("def f():\n    x = 1\n    return x\n");
        let f = out.nodes.iter().find(|n| n.id == "f").unwrap();
        assert!(out.sketches.contains_key(&f.id));
        assert_ne!(out.sketches[&f.id], [u32::MAX; 32], "a real body must produce at least one real hash");
    }

    #[test]
    fn extracts_absolute_aliased_and_relative_imports() {
        let src = "import os\nimport os.path as p\nfrom foo.bar import baz\nfrom . import sibling\nfrom .pkg import x\nfrom ..parent import y\n";
        let out = parse(src);
        assert_eq!(out.imports, vec!["os", "os.path", "foo.bar", ".", ".pkg", "..parent"]);
    }

    #[test]
    fn import_inside_a_conditional_block_is_still_found() {
        let out = parse("if True:\n    import os\n");
        assert_eq!(out.imports, vec!["os"]);
    }
}
