use cruxe_core::types::SymbolKind;
use std::ops::Range;

pub fn map_tag_kind(
    tag_kind: &str,
    has_parent: bool,
    node_kind: Option<&str>,
) -> Option<SymbolKind> {
    match tag_kind {
        "function" if has_parent => Some(SymbolKind::Method),
        "function" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "class" => match node_kind {
            Some("enum_item" | "enum_declaration") => Some(SymbolKind::Enum),
            Some("type_item" | "type_alias_declaration") => Some(SymbolKind::TypeAlias),
            Some("trait_item") => Some(SymbolKind::Trait),
            Some("interface_declaration") => Some(SymbolKind::Interface),
            Some("union_item" | "struct_item") => Some(SymbolKind::Struct),
            Some("class_definition" | "class_declaration" | "abstract_class_declaration") => {
                Some(SymbolKind::Class)
            }
            Some("interface_type") => Some(SymbolKind::Interface),
            Some("struct_type") => Some(SymbolKind::Struct),
            _ => Some(SymbolKind::Struct),
        },
        "interface" => match node_kind {
            Some("trait_item") => Some(SymbolKind::Trait),
            _ => Some(SymbolKind::Interface),
        },
        "module" => Some(SymbolKind::Module),
        "macro" => Some(SymbolKind::Function),
        "constant" => match node_kind {
            Some("expression_statement" | "assignment") => Some(SymbolKind::Variable),
            _ => Some(SymbolKind::Constant),
        },
        "variable" => Some(SymbolKind::Variable),
        "type" => match node_kind {
            Some("struct_type" | "struct_item") => Some(SymbolKind::Struct),
            Some("interface_type") => Some(SymbolKind::Interface),
            _ => Some(SymbolKind::TypeAlias),
        },
        _ => None,
    }
}

pub fn find_parent_scope(node: tree_sitter::Node, source: &str) -> Option<String> {
    if node.kind() == "method_declaration"
        && let Some(receiver) = node.child_by_field_name("receiver")
        && let Some(receiver_ty) = extract_go_receiver(receiver, source)
    {
        return Some(strip_generic_args(
            receiver_ty.trim().trim_start_matches('*').trim(),
        ));
    }

    let mut current = node.parent()?;
    loop {
        if is_transparent_node(current.kind()) {
            current = current.parent()?;
            continue;
        }

        if current.kind() == "impl_item" {
            let raw = current
                .child_by_field_name("type")
                .map(|n| node_text(n, source))
                .unwrap_or_default();
            let normalized = strip_generic_args(raw.trim().trim_start_matches('&').trim());
            return (!normalized.is_empty()).then_some(normalized);
        }

        if current.kind() == "method_declaration"
            && let Some(receiver) = current.child_by_field_name("receiver")
            && let Some(receiver_ty) = extract_go_receiver(receiver, source)
        {
            return Some(strip_generic_args(
                receiver_ty.trim().trim_start_matches('*').trim(),
            ));
        }

        if is_scope_node(current.kind())
            && let Some(name_node) = current.child_by_field_name("name")
        {
            let raw = node_text(name_node, source);
            let normalized = strip_generic_args(raw);
            return (!normalized.is_empty()).then_some(normalized);
        }

        current = current.parent()?;
    }
}

pub fn strip_generic_args(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut angle_depth = 0usize;
    let mut bracket_depth = 0usize;

    for ch in name.chars() {
        match ch {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if angle_depth == 0 && bracket_depth == 0 => out.push(ch),
            _ => {}
        }
    }

    out.trim().to_string()
}

pub fn separator_for_language(language: &str) -> &'static str {
    match language {
        "rust" => "::",
        _ => ".",
    }
}

pub fn extract_signature(
    kind: SymbolKind,
    source: &str,
    line_range: Range<usize>,
) -> Option<String> {
    if !matches!(kind, SymbolKind::Function | SymbolKind::Method) {
        return None;
    }

    let raw = source.get(line_range)?;
    let first_line = raw.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}

fn is_scope_node(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "class_definition"
            | "trait_item"
            | "struct_item"
            | "enum_item"
            | "mod_item"
            | "internal_module"
            | "namespace_definition"
            | "function_item"
            | "function_definition"
            | "function_declaration"
    )
}

fn is_transparent_node(kind: &str) -> bool {
    matches!(
        kind,
        "declaration_list"
            | "class_body"
            | "block"
            | "statement_block"
            | "decorated_definition"
            | "object_type"
            | "program"
            | "source_file"
    )
}

fn extract_go_receiver(receiver: tree_sitter::Node, source: &str) -> Option<String> {
    for i in 0..receiver.child_count() {
        let child = receiver.child(i)?;
        if child.kind() == "parameter_declaration"
            && let Some(type_node) = child.child_by_field_name("type")
        {
            return Some(node_text(type_node, source).to_string());
        }
    }
    None
}

fn node_text<'a>(node: tree_sitter::Node, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn strip_generic_args_supports_rust_and_go_style_generics() {
        assert_eq!(strip_generic_args("Foo<T>"), "Foo");
        assert_eq!(strip_generic_args("pkg.Foo[T]"), "pkg.Foo");
    }

    #[test]
    fn find_parent_scope_reads_go_receiver_type() {
        let source = r#"
package demo
type Service[T any] struct{}
func (s *Service[T]) Handle() {}
"#;
        let tree = parse_file(source, "go").unwrap();
        let root = tree.root_node();
        let method = find_first_node_by_kind(root, "method_declaration").expect("method");
        assert_eq!(
            find_parent_scope(method, source),
            Some("Service".to_string())
        );
    }

    #[test]
    fn map_tag_kind_promotes_nested_function_to_method() {
        assert_eq!(
            map_tag_kind("function", true, Some("function_definition")),
            Some(SymbolKind::Method)
        );
    }

    fn find_first_node_by_kind<'a>(
        root: tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == kind {
                return Some(node);
            }
            for idx in (0..node.child_count()).rev() {
                if let Some(child) = node.child(idx) {
                    stack.push(child);
                }
            }
        }
        None
    }
}
