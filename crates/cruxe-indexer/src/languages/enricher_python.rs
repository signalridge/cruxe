use super::enricher::{LanguageEnricher, node_text};
use cruxe_core::types::SymbolKind;

pub struct PythonEnricher;

impl LanguageEnricher for PythonEnricher {
    fn language(&self) -> &'static str {
        "python"
    }

    fn separator(&self) -> &'static str {
        "."
    }

    fn map_kind(
        &self,
        tag_kind: &str,
        has_parent: bool,
        _node: Option<tree_sitter::Node>,
        _source: &str,
    ) -> Option<SymbolKind> {
        match tag_kind {
            "function" if has_parent => Some(SymbolKind::Method),
            "function" => Some(SymbolKind::Function),
            "class" => Some(SymbolKind::Class),
            // Upstream Python tags.scm maps module-level assignments to @definition.constant.
            // We map them to Variable for consistency with the old extractor.
            "constant" => Some(SymbolKind::Variable),
            _ => None,
        }
    }

    fn extract_visibility(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        // Python visibility convention: underscore prefix = private.
        // We need the symbol name, not the node. The name is available
        // from the tag, but the enricher only gets the node. Walk to find
        // the name child.
        let name = find_name_text(node, source)?;
        // Dunder names are language-defined protocol methods (e.g., __init__, __str__)
        // and should not be treated as "private" API.
        if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
            return Some("public".into());
        }
        if name.starts_with('_') {
            Some("private".into())
        } else {
            Some("public".into())
        }
    }

    fn find_parent_scope(&self, node: tree_sitter::Node, source: &str) -> Option<String> {
        let mut current = node.parent()?;
        loop {
            match current.kind() {
                "class_definition" => {
                    return current
                        .child_by_field_name("name")
                        .map(|n| node_text(n, source).to_string());
                }
                // block is the body of class/function; decorated_definition wraps a
                // function/class. Keep walking up.
                "block" | "decorated_definition" => {
                    current = current.parent()?;
                }
                _ => return None,
            }
        }
    }
}

fn find_name_text<'a>(node: tree_sitter::Node, source: &'a str) -> Option<&'a str> {
    // For function_definition / class_definition, the name is a named field.
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(node_text(name_node, source));
    }
    // For expression_statement (assignment), drill into the assignment left side.
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "assignment"
            && let Some(left) = child.child_by_field_name("left")
            && left.kind() == "identifier"
        {
            return Some(node_text(left, source));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn python_dunder_method_is_public() {
        let source = r#"
class Service:
    def __init__(self):
        pass
"#;
        let tree = parse_file(source, "python").expect("parse python");
        let init_node =
            find_named_node(tree.root_node(), source, "function_definition", "__init__")
                .expect("__init__ node");

        let enricher = PythonEnricher;
        assert_eq!(
            enricher.extract_visibility(init_node, source),
            Some("public".to_string())
        );
    }

    #[test]
    fn python_single_underscore_method_is_private() {
        let source = r#"
class Service:
    def _helper(self):
        pass
"#;
        let tree = parse_file(source, "python").expect("parse python");
        let helper_node =
            find_named_node(tree.root_node(), source, "function_definition", "_helper")
                .expect("_helper node");

        let enricher = PythonEnricher;
        assert_eq!(
            enricher.extract_visibility(helper_node, source),
            Some("private".to_string())
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
