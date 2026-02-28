use super::enricher::{LanguageEnricher, node_text, strip_generic_args};
use cruxe_core::types::SymbolKind;

pub struct RustEnricher;

impl LanguageEnricher for RustEnricher {
    fn language(&self) -> &'static str {
        "rust"
    }

    fn separator(&self) -> &'static str {
        "::"
    }

    fn map_kind(
        &self,
        tag_kind: &str,
        has_parent: bool,
        node: Option<tree_sitter::Node>,
        _source: &str,
    ) -> Option<SymbolKind> {
        match tag_kind {
            "function" if has_parent => Some(SymbolKind::Method),
            "function" => Some(SymbolKind::Function),
            "method" => Some(SymbolKind::Method),
            "class" => {
                // Upstream tags.scm maps struct/enum/union/type all to @definition.class.
                // Disambiguate via the actual tree-sitter node kind.
                match node.map(|n| n.kind()) {
                    Some("enum_item") => Some(SymbolKind::Enum),
                    Some("type_item") => Some(SymbolKind::TypeAlias),
                    Some("union_item") => Some(SymbolKind::Struct),
                    _ => Some(SymbolKind::Struct), // struct_item or fallback
                }
            }
            "interface" => Some(SymbolKind::Trait),
            "module" => Some(SymbolKind::Module),
            "macro" => Some(SymbolKind::Function), // closest mapping
            "constant" => Some(SymbolKind::Constant),
            "variable" => Some(SymbolKind::Variable), // static items
            _ => None,
        }
    }

    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i)
                && child.kind() == "visibility_modifier"
            {
                return Some(node_text(child, source).to_string());
            }
        }
        Some("private".to_string())
    }

    fn find_parent_scope(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        let mut current = node.parent()?;
        loop {
            match current.kind() {
                "impl_item" => {
                    return current
                        .child_by_field_name("type")
                        .map(|n| normalize_rust_parent(node_text(n, source)));
                }
                "trait_item" | "struct_item" | "enum_item" | "mod_item" => {
                    return current
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source).to_string());
                }
                // declaration_list is the body of impl/trait; keep walking up.
                "declaration_list" => {
                    current = current.parent()?;
                }
                _ => return None,
            }
        }
    }
}

fn normalize_rust_parent(raw: &str) -> String {
    // impl receivers may include references/pointers and generic args:
    // `&Foo<T>` -> `Foo`, `crate::Foo<T>` -> `crate::Foo`.
    let no_refs = raw
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();
    strip_generic_args(no_refs, '<', '>')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn rust_visibility_defaults_to_private() {
        let source = "struct Foo; impl Foo { fn method(&self) {} }";
        let tree = parse_file(source, "rust").expect("parse rust");
        let method = find_named_node(tree.root_node(), source, "function_item", "method")
            .expect("method node");

        let enricher = RustEnricher;
        assert_eq!(
            enricher.extract_visibility(method, source),
            Some("private".to_string())
        );
    }

    #[test]
    fn rust_parent_scope_strips_generic_arguments() {
        let source = r#"
            struct Foo<T>(T);
            impl<T> Foo<T> {
                fn method(&self) {}
            }
        "#;
        let tree = parse_file(source, "rust").expect("parse rust");
        let method = find_named_node(tree.root_node(), source, "function_item", "method")
            .expect("method node");

        let enricher = RustEnricher;
        assert_eq!(
            enricher.find_parent_scope(method, source),
            Some("Foo".to_string())
        );
    }

    #[test]
    fn rust_variable_kind_maps_to_variable_for_static_items() {
        let enricher = RustEnricher;
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
