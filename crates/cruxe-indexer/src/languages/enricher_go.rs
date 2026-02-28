use super::enricher::{LanguageEnricher, node_text, strip_generic_args};
use cruxe_core::types::SymbolKind;

pub struct GoEnricher;

impl LanguageEnricher for GoEnricher {
    fn language(&self) -> &'static str {
        "go"
    }

    fn separator(&self) -> &'static str {
        "."
    }

    fn map_kind(
        &self,
        tag_kind: &str,
        _has_parent: bool,
        node: Option<tree_sitter::Node>,
        _source: &str,
    ) -> Option<SymbolKind> {
        match tag_kind {
            "function" => Some(SymbolKind::Function),
            "method" => Some(SymbolKind::Method),
            "constant" => Some(SymbolKind::Constant),
            "variable" => Some(SymbolKind::Variable),
            "type" => {
                // Go tags.scm has separate captures for struct_type and interface_type,
                // but also a generic type_spec capture. Disambiguate via node.
                if let Some(n) = node {
                    // The node may be a type_spec or a type_declaration containing type_spec.
                    let type_spec = if n.kind() == "type_spec" {
                        Some(n)
                    } else {
                        find_child_kind(n, "type_spec")
                    };
                    if let Some(ts) = type_spec
                        && let Some(type_node) = ts.child_by_field_name("type")
                    {
                        return match type_node.kind() {
                            "struct_type" => Some(SymbolKind::Struct),
                            "interface_type" => Some(SymbolKind::Interface),
                            _ => Some(SymbolKind::TypeAlias),
                        };
                    }
                }
                Some(SymbolKind::TypeAlias)
            }
            _ => None,
        }
    }

    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        // Go convention: uppercase first letter = exported (public).
        let name = node
            .child_by_field_name("name")
            .map(|n| node_text(n, source))?;
        if name.chars().next().is_some_and(|c| c.is_uppercase()) {
            Some("public".into())
        } else {
            Some("private".into())
        }
    }

    fn find_parent_scope(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        // Go methods have a receiver parameter that identifies the parent type.
        if node.kind() == "method_declaration" {
            return node.child_by_field_name("receiver").and_then(|r| {
                for i in 0..r.child_count() {
                    if let Some(c) = r.child(i)
                        && c.kind() == "parameter_declaration"
                        && let Some(type_node) = c.child_by_field_name("type")
                    {
                        let text = node_text(type_node, source);
                        return Some(normalize_go_receiver(text));
                    }
                }
                None
            });
        }
        // Go has no nested type definitions; top-level functions/types have no parent.
        None
    }
}

fn find_child_kind<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == kind
        {
            return Some(child);
        }
    }
    None
}

fn normalize_go_receiver(raw: &str) -> String {
    // method receivers may include pointers and generic args:
    // `*Foo[T]` -> `Foo`, `pkg.Foo[T]` -> `pkg.Foo`.
    let no_ptr = raw.trim().trim_start_matches('*').trim();
    strip_generic_args(no_ptr, '[', ']')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn go_parent_scope_strips_generic_arguments() {
        let source = r#"
package demo

type Foo[T any] struct{}

func (s *Foo[T]) Handle() {}
"#;
        let tree = parse_file(source, "go").expect("parse go");
        let method = find_named_node(tree.root_node(), source, "method_declaration", "Handle")
            .expect("method node");

        let enricher = GoEnricher;
        assert_eq!(
            enricher.find_parent_scope(method, source),
            Some("Foo".to_string())
        );
    }

    #[test]
    fn go_map_kind_handles_constant_and_variable() {
        let enricher = GoEnricher;
        assert_eq!(
            enricher.map_kind("constant", false, None, ""),
            Some(SymbolKind::Constant)
        );
        assert_eq!(
            enricher.map_kind("variable", false, None, ""),
            Some(SymbolKind::Variable)
        );
    }

    fn find_named_node<'a>(
        root: tree_sitter::Node<'a>,
        source: &str,
        kind: &str,
        name: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == kind
                && let Some(name_node) = node.child_by_field_name("name")
                && node_text(name_node, source) == name
            {
                return Some(node);
            }
            for i in (0..node.child_count()).rev() {
                if let Some(child) = node.child(i) {
                    stack.push(child);
                }
            }
        }
        None
    }
}
