//! TypeScript `ParserAdapter` — Tree-sitter backend (06 "파서 백엔드").
//! `.ts` only (`LANGUAGE_TYPESCRIPT`) — `.tsx` needs the sibling
//! `LANGUAGE_TSX` grammar and isn't in 06's v0.1 format list, so it's not
//! wired in here; add it as a second adapter if that gap ever matters.

use tree_sitter::{Node as TsNode, Parser};

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct TypeScriptAdapter;

impl ParserAdapter for TypeScriptAdapter {
    fn extensions(&self) -> &[&str] {
        &["ts"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("tree-sitter-typescript grammar");
        let Some(tree) = parser.parse(input.bytes, None) else {
            return ParseOutput { partial: true, ..ParseOutput::default() };
        };

        let mut nodes = Vec::new();
        walk(tree.root_node(), input.bytes, &mut nodes);
        let mut imports = Vec::new();
        walk_imports(tree.root_node(), input.bytes, &mut imports);

        let sketches = nodes.iter().map(|n| (n.id.clone(), shingle_sketch(&n.source))).collect();

        ParseOutput {
            nodes,
            edges: Vec::<Edge>::new(),
            searchable_text: String::from_utf8_lossy(input.bytes).into_owned(),
            sketches,
            partial: tree.root_node().has_error(),
            imports,
        }
    }
}

/// `import ... from "X"`, `export ... from "X"` (a re-export, still a real
/// dependency edge), and CommonJS `require("X")` — the three ways a `.ts`
/// file names another module. `X` is kept exactly as written: `"./utils"`
/// (resolvable, relative) and `"lodash"` (a bare specifier — an npm
/// package, not a vault file) look the same to this pass; telling them
/// apart is the resolver's job, not the parser's.
fn walk_imports(node: TsNode, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" | "export_statement" => {
                if let Some(source) = child.child_by_field_name("source")
                    && let Some(text) = string_literal_value(source, src)
                {
                    out.push(text);
                }
            }
            "call_expression" => {
                let is_require = child.child_by_field_name("function").and_then(|f| f.utf8_text(src).ok()) == Some("require");
                if is_require
                    && let Some(args) = child.child_by_field_name("arguments")
                    && let Some(first) = args.named_child(0)
                    && let Some(text) = string_literal_value(first, src)
                {
                    out.push(text);
                }
            }
            _ => {}
        }
        walk_imports(child, src, out);
    }
}

/// A `string` node's content without its surrounding quotes — tree-sitter
/// wraps it in a `string_fragment` child rather than including the quotes
/// in that child's own text.
fn string_literal_value(node: TsNode, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == "string_fragment")?.utf8_text(src).ok().map(str::to_string)
}

fn walk(root: TsNode, src: &[u8], out: &mut Vec<Node>) {
    let mut cursor = root.walk();
    for raw_child in root.children(&mut cursor) {
        // `export class Foo {}` / `export default class Foo {}` wrap the
        // declaration inside an `export_statement` — real-world TS/module
        // code is almost entirely `export`ed, so without unwrapping this,
        // top-level symbols would go nearly all-missing rather than just
        // occasionally missing. `export { foo }` re-exports have no
        // `declaration` field and are correctly skipped (nothing new here).
        let child = if raw_child.kind() == "export_statement" {
            match raw_child.child_by_field_name("declaration") {
                Some(decl) => decl,
                None => continue,
            }
        } else {
            raw_child
        };
        match child.kind() {
            "function_declaration" => push(&child, src, None, "function", out),
            "class_declaration" => {
                let Some(name_node) = child.child_by_field_name("name") else { continue };
                let class_name = name_node.utf8_text(src).unwrap_or("").to_string();
                push(&child, src, None, "class", out);
                if let Some(body) = child.child_by_field_name("body") {
                    push_members(body, src, &class_name, "method_definition", "method", out);
                }
            }
            "interface_declaration" => {
                let Some(name_node) = child.child_by_field_name("name") else { continue };
                let iface_name = name_node.utf8_text(src).unwrap_or("").to_string();
                push(&child, src, None, "interface", out);
                if let Some(body) = child.child_by_field_name("body") {
                    push_members(body, src, &iface_name, "method_signature", "method", out);
                }
            }
            _ => {}
        }
    }
}

fn push_members(body: TsNode, src: &[u8], qualname_prefix: &str, member_kind: &str, node_type: &str, out: &mut Vec<Node>) {
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        if member.kind() == member_kind {
            push(&member, src, Some(qualname_prefix), node_type, out);
        }
    }
}

fn push(node: &TsNode, src: &[u8], qualname_prefix: Option<&str>, node_type: &str, out: &mut Vec<Node>) {
    let Some(name_node) = node.child_by_field_name("name") else { return };
    let name = name_node.utf8_text(src).unwrap_or("").to_string();
    let id = match qualname_prefix {
        Some(prefix) => format!("{prefix}.{name}"),
        None => name.clone(),
    };
    out.push(Node {
        id,
        node_type: node_type.to_string(),
        name,
        source: node.utf8_text(src).unwrap_or("").to_string(),
        range: node.byte_range(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr;

    fn parse(src: &str) -> ParseOutput {
        TypeScriptAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_class_method_function_and_interface() {
        let src = "class Router {\n  select(x: number): number {\n    return x;\n  }\n}\n\nfunction main() {}\n\ninterface Handler {\n  serve(): void;\n}\n";
        let out = parse(src);

        let ids: Vec<(&str, &str)> = out.nodes.iter().map(|n| (n.id.as_str(), n.node_type.as_str())).collect();
        assert!(ids.contains(&("Router", "class")));
        assert!(ids.contains(&("Router.select", "method")));
        assert!(ids.contains(&("main", "function")));
        assert!(ids.contains(&("Handler", "interface")));
        assert!(ids.contains(&("Handler.serve", "method")));
        assert!(!out.partial);
    }

    #[test]
    fn symbol_id_round_trips_through_a_real_vault_address() {
        let out = parse("class Router {\n  select() {}\n}\n");
        let method = out.nodes.iter().find(|n| n.id == "Router.select").unwrap();

        let fragment = method.id.split('.').map(addr::encode_symbol_name).collect::<Vec<_>>().join(".");
        let parsed = addr::parse(&format!("vault://src/router.ts#{fragment}")).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["Router".into(), "select".into()])));
    }

    #[test]
    fn exported_declarations_are_still_extracted() {
        // Almost all real-world TS is `export`ed — this was missing entirely
        // before `export_statement` unwrapping was added (found in review).
        let src = "export class Router {\n  select() {}\n}\n\nexport function main() {}\n\nexport interface Handler {\n  serve(): void;\n}\n\nexport default class Foo {}\n";
        let out = parse(src);

        let ids: Vec<(&str, &str)> = out.nodes.iter().map(|n| (n.id.as_str(), n.node_type.as_str())).collect();
        assert!(ids.contains(&("Router", "class")));
        assert!(ids.contains(&("Router.select", "method")));
        assert!(ids.contains(&("main", "function")));
        assert!(ids.contains(&("Handler", "interface")));
        assert!(ids.contains(&("Foo", "class")));
    }

    /// Mid-edit state (06's own measurement basis: 99.9% Tree-sitter symbol
    /// survival vs. 0.0% for an AST parser on an unterminated function).
    #[test]
    fn unterminated_function_marks_partial_but_keeps_the_earlier_complete_symbol() {
        let out = parse("class Router {}\n\nfunction handle(request, context");
        assert!(out.partial);
        assert!(out.nodes.iter().any(|n| n.id == "Router" && n.node_type == "class"));
    }

    #[test]
    fn extracts_import_reexport_and_require_specifiers() {
        let src = "import { foo } from \"./utils\";\nimport bar from \"../lib/bar\";\nimport \"lodash\";\nexport { x } from \"./reexport\";\nconst m = require(\"./legacy\");\n";
        let out = parse(src);
        assert_eq!(out.imports, vec!["./utils", "../lib/bar", "lodash", "./reexport", "./legacy"]);
    }
}
