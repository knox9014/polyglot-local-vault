//! Go `ParserAdapter` — Tree-sitter backend (06 "파서 백엔드").
//! Same shape as `parser_python`: functions, methods (via receiver type),
//! structs, and interfaces become dotted qualnames (`Router.ServeHTTP`).

use tree_sitter::{Node as TsNode, Parser};

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct GoAdapter;

impl ParserAdapter for GoAdapter {
    fn extensions(&self) -> &[&str] {
        &["go"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_go::LANGUAGE.into()).expect("tree-sitter-go grammar");
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

/// `import "fmt"` and `import ( "fmt"; "github.com/x/y/pkg" )` both bottom
/// out at one or more `import_spec`s with a `path` string. Extracted as-is
/// (`"github.com/x/y/pkg"`) — resolving that to a vault file needs the
/// importing module's own path from its `go.mod`, which the resolver reads
/// (this single-file parser has no way to see a sibling file).
fn walk_imports(node: TsNode, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "import_spec"
            && let Some(path) = child.child_by_field_name("path")
            && let Some(text) = string_literal_value(path, src)
        {
            out.push(text);
        }
        walk_imports(child, src, out);
    }
}

/// An `interpreted_string_literal`'s content without its surrounding quotes.
fn string_literal_value(node: TsNode, src: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == "interpreted_string_literal_content")?.utf8_text(src).ok().map(str::to_string)
}

/// Go has no lexical nesting for methods — `func (r *Router) ServeHTTP(...)`
/// sits at the top level, and the receiver type is what makes it a method.
/// So unlike Python's recursive descent, this is a flat pass reading each
/// declaration's own field (`name`, or `receiver` + `name`).
fn walk(root: TsNode, src: &[u8], out: &mut Vec<Node>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => push_named(&child, src, "name", None, "function", out),
            "method_declaration" => {
                let receiver_type = child
                    .child_by_field_name("receiver")
                    .and_then(|params| receiver_type_name(params, src));
                push_named(&child, src, "name", receiver_type.as_deref(), "method", out);
            }
            "type_declaration" => {
                let mut tc = child.walk();
                for spec in child.children(&mut tc).filter(|c| c.kind() == "type_spec") {
                    let Some(name_node) = spec.child_by_field_name("name") else { continue };
                    let kind = spec
                        .child_by_field_name("type")
                        .map(|t| match t.kind() {
                            "struct_type" => "struct",
                            "interface_type" => "interface",
                            _ => "type",
                        })
                        .unwrap_or("type");
                    out.push(Node {
                        id: name_node.utf8_text(src).unwrap_or("").to_string(),
                        node_type: kind.to_string(),
                        name: name_node.utf8_text(src).unwrap_or("").to_string(),
                        source: child.utf8_text(src).unwrap_or("").to_string(),
                        range: child.byte_range(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// The receiver's declared type name, stripping a leading `*` for pointer
/// receivers (`(r *Router)` and `(r Router)` are the same qualname prefix).
fn receiver_type_name(params: TsNode, src: &[u8]) -> Option<String> {
    let mut cursor = params.walk();
    let param = params.children(&mut cursor).find(|c| c.kind() == "parameter_declaration")?;
    let type_node = param.child_by_field_name("type")?;
    let text = type_node.utf8_text(src).ok()?;
    Some(text.trim_start_matches('*').to_string())
}

fn push_named(node: &TsNode, src: &[u8], name_field: &str, qualname_prefix: Option<&str>, node_type: &str, out: &mut Vec<Node>) {
    let Some(name_node) = node.child_by_field_name(name_field) else { return };
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
        GoAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_function_struct_and_pointer_receiver_method() {
        let src = "package main\n\ntype Router struct {\n\tRoutes []string\n}\n\nfunc (r *Router) ServeHTTP() {\n}\n\nfunc main() {\n}\n";
        let out = parse(src);

        let ids: Vec<(&str, &str)> = out.nodes.iter().map(|n| (n.id.as_str(), n.node_type.as_str())).collect();
        assert!(ids.contains(&("Router", "struct")));
        assert!(ids.contains(&("Router.ServeHTTP", "method")));
        assert!(ids.contains(&("main", "function")));
        assert!(!out.partial);
    }

    #[test]
    fn symbol_id_round_trips_through_a_real_vault_address() {
        let out = parse("package main\n\ntype Router struct{}\n\nfunc (r *Router) ServeHTTP() {}\n");
        let method = out.nodes.iter().find(|n| n.id == "Router.ServeHTTP").unwrap();

        let fragment = method.id.split('.').map(addr::encode_symbol_name).collect::<Vec<_>>().join(".");
        let parsed = addr::parse(&format!("vault://server/router.go#{fragment}")).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["Router".into(), "ServeHTTP".into()])));
    }

    #[test]
    fn interface_declaration_is_extracted_too() {
        let out = parse("package main\n\ntype Handler interface {\n\tServe()\n}\n");
        assert!(out.nodes.iter().any(|n| n.id == "Handler" && n.node_type == "interface"));
    }

    /// Mid-edit state (06's own measurement basis: 99.9% Tree-sitter symbol
    /// survival vs. 0.0% for an AST parser on an unterminated function).
    #[test]
    fn unterminated_function_marks_partial_but_keeps_the_earlier_complete_symbol() {
        let out = parse("package main\n\ntype Router struct{}\n\nfunc serve(w Response, r *Request) {\n\tif r.Method ==");
        assert!(out.partial);
        assert!(out.nodes.iter().any(|n| n.id == "Router" && n.node_type == "struct"));
    }

    #[test]
    fn extracts_grouped_and_single_import_specs() {
        let src = "package main\n\nimport (\n\t\"fmt\"\n\t\"github.com/me/myrepo/internal/util\"\n)\n\nimport \"os\"\n";
        let out = parse(src);
        assert_eq!(out.imports, vec!["fmt", "github.com/me/myrepo/internal/util", "os"]);
    }
}
