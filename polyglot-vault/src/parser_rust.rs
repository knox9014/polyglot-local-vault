//! Rust `ParserAdapter` — Tree-sitter backend (06 "파서 백엔드").
//! `struct`/`enum`/`trait` become top-level symbols; methods inside an
//! `impl Type { .. }` or `trait Name { .. }` block get `Type.method` /
//! `Name.method` qualnames, same shape as Python's class nesting.

use tree_sitter::{Node as TsNode, Parser};

use crate::parser::{Edge, Node, ParseInput, ParseOutput, ParserAdapter, shingle_sketch};

pub struct RustAdapter;

impl ParserAdapter for RustAdapter {
    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse(&self, input: ParseInput<'_>) -> ParseOutput {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).expect("tree-sitter-rust grammar");
        let Some(tree) = parser.parse(input.bytes, None) else {
            return ParseOutput { partial: true, ..ParseOutput::default() };
        };

        let mut nodes = Vec::new();
        walk(tree.root_node(), input.bytes, "", &mut nodes);
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

/// `use crate::a::b;` / `use self::x;` / `use super::y;` / `use std::fmt;`
/// keep their raw path text (`"crate::a::b"`, including a `{...}` grouped
/// list verbatim if there is one — the resolver splits that, not this pass).
/// `mod foo;` is a different resolution shape (file-relative, not
/// crate-root-relative), so it's tagged `"mod:foo"` rather than sharing the
/// bare-string shape `use` gets; the resolver dispatches on that prefix.
fn walk_imports(node: TsNode, src: &[u8], out: &mut Vec<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "use_declaration" => {
                if let Some(arg) = child.child_by_field_name("argument")
                    && let Ok(text) = arg.utf8_text(src)
                {
                    out.push(text.to_string());
                }
            }
            "mod_item" => {
                // `mod foo;` (declares a file) vs `mod foo { .. }` (inline
                // body, no file to resolve) — only the former has no `body`.
                if child.child_by_field_name("body").is_none()
                    && let Some(name) = child.child_by_field_name("name")
                    && let Ok(text) = name.utf8_text(src)
                {
                    out.push(format!("mod:{text}"));
                }
            }
            _ => {}
        }
        walk_imports(child, src, out);
    }
}

/// `prefix` is the enclosing module path, so an item inside
/// `mod tests { .. }` becomes `tests.foo` — the same dotted shape Python's
/// `Class.method` uses, and still substring-matchable by the bare name.
fn walk(root: TsNode, src: &[u8], prefix: &str, out: &mut Vec<Node>) {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_item" => push(&child, src, "name", prefix, "function", out),
            "struct_item" => push(&child, src, "name", prefix, "struct", out),
            "enum_item" => push(&child, src, "name", prefix, "enum", out),
            "trait_item" => {
                let Some(name_node) = child.child_by_field_name("name") else { continue };
                let trait_name = name_node.utf8_text(src).unwrap_or("").to_string();
                push(&child, src, "name", prefix, "trait", out);
                if let Some(body) = child.child_by_field_name("body") {
                    push_methods(body, src, &qualname(prefix, &trait_name), out);
                }
            }
            // `impl Config { .. }` and `impl Greet for Config { .. }` both put
            // the implementing type in the `type` field; the trait being
            // implemented (if any) is in `trait`, which we don't need for
            // the qualname — a method resolves through the type either way.
            "impl_item" => {
                let Some(type_node) = child.child_by_field_name("type") else { continue };
                let type_name = base_type_name(type_node, src);
                if let Some(body) = child.child_by_field_name("body") {
                    push_methods(body, src, &qualname(prefix, &type_name), out);
                }
            }
            // An inline `mod name { .. }` is a real scope holding real
            // symbols. Skipping it made every `#[cfg(test)] mod tests { .. }`
            // function invisible to the symbol index — which is most of the
            // test suite in an idiomatic Rust file, this crate's included.
            // (`mod name;` has no body: that's a file reference, handled as
            // an import instead — see `walk_imports`.)
            "mod_item" => {
                if let Some(body) = child.child_by_field_name("body") {
                    let mod_name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(src).ok())
                        .unwrap_or("");
                    walk(body, src, &qualname(prefix, mod_name), out);
                }
            }
            _ => {}
        }
    }
}

fn qualname(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") }
}

/// `impl<T> Container<T> { .. }` puts the implementing type in a
/// `generic_type` node whose own text is `Container<T>` — used as a
/// qualname prefix that would make every method's id `Container<T>.get`
/// instead of `Container.get`. Strip to the bare name (its `type` field)
/// the same way regardless of how many generic layers are nested.
fn base_type_name(type_node: TsNode, src: &[u8]) -> String {
    if type_node.kind() == "generic_type"
        && let Some(inner) = type_node.child_by_field_name("type")
    {
        return base_type_name(inner, src);
    }
    type_node.utf8_text(src).unwrap_or("").to_string()
}

/// Methods inside an `impl`/`trait` body — `function_item` when it has a
/// body, `function_signature_item` for a trait method with no default body.
fn push_methods(body: TsNode, src: &[u8], qualname_prefix: &str, out: &mut Vec<Node>) {
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        if matches!(member.kind(), "function_item" | "function_signature_item") {
            push(&member, src, "name", qualname_prefix, "method", out);
        }
    }
}

fn push(node: &TsNode, src: &[u8], name_field: &str, qualname_prefix: &str, node_type: &str, out: &mut Vec<Node>) {
    let Some(name_node) = node.child_by_field_name(name_field) else { return };
    let name = name_node.utf8_text(src).unwrap_or("").to_string();
    let id = qualname(qualname_prefix, &name);
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
        RustAdapter.parse(ParseInput { bytes: src.as_bytes(), previous: None })
    }

    #[test]
    fn extracts_struct_impl_method_and_free_function() {
        let src = "struct Config {\n    x: i32,\n}\n\nimpl Config {\n    fn load(&self) -> i32 {\n        self.x\n    }\n}\n\nfn main() {}\n";
        let out = parse(src);

        let ids: Vec<(&str, &str)> = out.nodes.iter().map(|n| (n.id.as_str(), n.node_type.as_str())).collect();
        assert!(ids.contains(&("Config", "struct")));
        assert!(ids.contains(&("Config.load", "method")));
        assert!(ids.contains(&("main", "function")));
        assert!(!out.partial);
    }

    #[test]
    fn trait_default_and_signature_methods_both_get_qualnames() {
        let src = "trait Greet {\n    fn hello(&self);\n    fn bye(&self) {}\n}\n";
        let out = parse(src);

        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(ids.contains(&"Greet"));
        assert!(ids.contains(&"Greet.hello"));
        assert!(ids.contains(&"Greet.bye"));
    }

    #[test]
    fn symbol_id_round_trips_through_a_real_vault_address() {
        let out = parse("struct Config;\n\nimpl Config {\n    fn load(&self) {}\n}\n");
        let method = out.nodes.iter().find(|n| n.id == "Config.load").unwrap();

        let fragment = method.id.split('.').map(addr::encode_symbol_name).collect::<Vec<_>>().join(".");
        let parsed = addr::parse(&format!("vault://src/config.rs#{fragment}")).unwrap();
        assert_eq!(parsed.fragment, Some(addr::Fragment::Symbol(vec!["Config".into(), "load".into()])));
    }

    #[test]
    fn generic_impl_block_strips_type_arguments_from_the_qualname() {
        // Without stripping, this would be "Container<T>.get" — a qualname
        // that can never match `S1` (same-file same-qualname) against a
        // caller's plain `Container.get` reference (found in review).
        let out = parse("struct Container<T> {\n    items: Vec<T>,\n}\n\nimpl<T> Container<T> {\n    fn get(&self) -> &T {\n        &self.items[0]\n    }\n}\n");
        assert!(out.nodes.iter().any(|n| n.id == "Container.get" && n.node_type == "method"));
    }

    /// Mid-edit state (06's own measurement basis: 99.9% Tree-sitter symbol
    /// survival vs. 0.0% for an AST parser on an unterminated function).
    #[test]
    fn unterminated_function_marks_partial_but_keeps_the_earlier_complete_symbol() {
        let out = parse("struct Config;\n\nfn process_");
        assert!(out.partial);
        assert!(out.nodes.iter().any(|n| n.id == "Config" && n.node_type == "struct"));
    }

    /// Found by the P4 natural-language measurement: a query for a real
    /// test function returned nothing, because `walk` never descended into
    /// `mod tests { .. }`. In idiomatic Rust that hides most of a file's
    /// test suite — 158 of this crate's own tests were unsearchable.
    #[test]
    fn symbols_inside_an_inline_mod_block_are_extracted() {
        let src = "fn top() {}\n\n#[cfg(test)]\nmod tests {\n    struct Fixture;\n    fn helper() {}\n    #[test]\n    fn settings_without_accent_still_loads() {}\n    mod deeper {\n        fn nested() {}\n    }\n}\n";
        let out = parse(src);
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();

        assert!(ids.contains(&"top"), "top-level items must be unaffected: {ids:?}");
        assert!(ids.contains(&"tests.settings_without_accent_still_loads"), "{ids:?}");
        assert!(ids.contains(&"tests.helper"));
        assert!(ids.contains(&"tests.Fixture"));
        assert!(ids.contains(&"tests.deeper.nested"), "nesting must compose: {ids:?}");
    }

    /// `mod name;` (no body) stays an import, not a symbol — the two forms
    /// share a keyword but mean different things.
    #[test]
    fn a_file_backed_mod_declaration_is_not_treated_as_a_scope() {
        let out = parse("mod submodule;\nfn only_real_symbol() {}\n");
        let ids: Vec<&str> = out.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["only_real_symbol"]);
        assert_eq!(out.imports, vec!["mod:submodule"]);
    }

    #[test]
    fn extracts_use_paths_and_file_backed_mod_declarations() {
        let src = "use crate::config::Settings;\nuse self::util;\nuse super::helper;\nuse std::collections::HashMap;\nmod submodule;\nmod inline_body { }\n";
        let out = parse(src);
        assert_eq!(
            out.imports,
            vec!["crate::config::Settings", "self::util", "super::helper", "std::collections::HashMap", "mod:submodule"]
        );
    }
}
