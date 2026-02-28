use super::enricher::{LanguageEnricher, node_text};
use cruxe_core::types::SymbolKind;

pub struct TypeScriptEnricher;

impl LanguageEnricher for TypeScriptEnricher {
    fn language(&self) -> &'static str {
        "typescript"
    }

    fn separator(&self) -> &'static str {
        "."
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
                // Disambiguate enum_declaration vs class vs type_alias.
                match node.map(|n| n.kind()) {
                    Some("enum_declaration") => Some(SymbolKind::Enum),
                    Some("type_alias_declaration") => Some(SymbolKind::TypeAlias),
                    _ => Some(SymbolKind::Class),
                }
            }
            "interface" => Some(SymbolKind::Interface),
            "module" => Some(SymbolKind::Module),
            // Fix: upstream tags.scm captures lexical_declaration as @definition.variable.
            // Distinguish const vs let/var via the tree-sitter node.
            "variable" => {
                if let Some(n) = node
                    && is_const_declaration(n)
                {
                    return Some(SymbolKind::Constant);
                }
                Some(SymbolKind::Variable)
            }
            _ => None,
        }
    }

    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        // Check for accessibility_modifier (public/private/protected) on the node.
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i)
                && child.kind() == "accessibility_modifier"
            {
                return Some(node_text(child, source).to_string());
            }
        }
        // Check if wrapped in export_statement.
        if let Some(parent) = node.parent()
            && parent.kind() == "export_statement"
        {
            return Some("export".to_string());
        }
        Some("private".to_string())
    }

    fn find_parent_scope(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        let mut current = node.parent()?;
        loop {
            match current.kind() {
                "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "internal_module" => {
                    return current
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source).to_string());
                }
                // Body-like containers; keep walking up.
                "class_body" | "object_type" | "statement_block" => {
                    current = current.parent()?;
                }
                _ => return None,
            }
        }
    }
}

/// Check if a variable_declarator's enclosing declaration is `const`.
fn is_const_declaration(node: tree_sitter::Node) -> bool {
    // Walk up from variable_declarator to lexical_declaration to check the keyword.
    let mut current = Some(node);
    while let Some(n) = current {
        if n.kind() == "lexical_declaration" {
            // First child of lexical_declaration is the keyword (const/let/var).
            if let Some(first) = n.child(0) {
                return first.kind() == "const";
            }
        }
        current = n.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn typescript_visibility_defaults_to_private() {
        let source = "function internalFn() { return 1; }";
        let tree = parse_file(source, "typescript").expect("parse ts");
        let func = find_named_node(
            tree.root_node(),
            source,
            "function_declaration",
            "internalFn",
        )
        .expect("function node");

        let enricher = TypeScriptEnricher;
        assert_eq!(
            enricher.extract_visibility(func, source),
            Some("private".to_string())
        );
    }

    #[test]
    fn typescript_namespace_parent_scope_is_detected() {
        let source = r#"
namespace Api {
  function ping() { return 1; }
}
"#;
        let tree = parse_file(source, "typescript").expect("parse ts");
        let func = find_named_node(tree.root_node(), source, "function_declaration", "ping")
            .expect("function node");

        let enricher = TypeScriptEnricher;
        assert_eq!(
            enricher.find_parent_scope(func, source),
            Some("Api".to_string())
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
