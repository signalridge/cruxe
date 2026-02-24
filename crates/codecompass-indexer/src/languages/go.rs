use super::ExtractedSymbol;
use crate::import_extract::RawImport;
use codecompass_core::types::SymbolKind;

pub fn extract(tree: &tree_sitter::Tree, source: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let root = tree.root_node();
    extract_from_node(root, source, &mut symbols);
    symbols
}

fn extract_from_node(node: tree_sitter::Node, source: &str, symbols: &mut Vec<ExtractedSymbol>) {
    match node.kind() {
        "function_declaration" => {
            if let Some(sym) = extract_function(node, source) {
                symbols.push(sym);
            }
        }
        "method_declaration" => {
            if let Some(sym) = extract_method(node, source) {
                symbols.push(sym);
            }
        }
        "type_declaration" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() != "type_spec" {
                        continue;
                    }
                    if let Some(sym) = extract_type_spec(child, source) {
                        symbols.push(sym);
                    }
                }
            }
        }
        "const_declaration" | "var_declaration" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() != "const_spec" && child.kind() != "var_spec" {
                        continue;
                    }
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = node_text(name_node, source);
                        let kind = if node.kind() == "const_declaration" {
                            SymbolKind::Constant
                        } else {
                            SymbolKind::Variable
                        };
                        let vis = if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                            Some("public".into())
                        } else {
                            Some("private".into())
                        };
                        symbols.push(ExtractedSymbol {
                            name: name.clone(),
                            qualified_name: name,
                            kind,
                            language: "go".into(),
                            signature: None,
                            line_start: child.start_position().row as u32 + 1,
                            line_end: child.end_position().row as u32 + 1,
                            visibility: vis,
                            parent_name: None,
                            body: Some(node_text(child, source)),
                        });
                    }
                }
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_from_node(child, source, symbols);
        }
    }
}

fn extract_function(node: tree_sitter::Node, source: &str) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);
    let sig = node_text(node, source).lines().next()?.trim().to_string();
    let vis = if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Some("public".into())
    } else {
        Some("private".into())
    };

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: name,
        kind: SymbolKind::Function,
        language: "go".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: vis,
        parent_name: None,
        body: Some(node_text(node, source)),
    })
}

fn extract_method(node: tree_sitter::Node, source: &str) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);
    let sig = node_text(node, source).lines().next()?.trim().to_string();

    // Extract receiver type
    let receiver = node.child_by_field_name("receiver").and_then(|r| {
        let mut found = None;
        for i in 0..r.child_count() {
            if let Some(c) = r.child(i)
                && c.kind() == "parameter_declaration"
            {
                found = c
                    .child_by_field_name("type")
                    .map(|t| node_text(t, source).replace('*', ""));
                break;
            }
        }
        found
    });

    let vis = if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Some("public".into())
    } else {
        Some("private".into())
    };

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: match &receiver {
            Some(r) => format!("{}.{}", r, name),
            None => name.clone(),
        },
        kind: SymbolKind::Method,
        language: "go".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: vis,
        parent_name: receiver,
        body: Some(node_text(node, source)),
    })
}

fn extract_type_spec(node: tree_sitter::Node, source: &str) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let type_node = node.child_by_field_name("type")?;
    let kind = match type_node.kind() {
        "struct_type" => SymbolKind::Struct,
        "interface_type" => SymbolKind::Interface,
        _ => SymbolKind::TypeAlias,
    };

    let vis = if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        Some("public".into())
    } else {
        Some("private".into())
    };

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: name,
        kind,
        language: "go".into(),
        signature: None,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: vis,
        parent_name: None,
        body: Some(node_text(node, source)),
    })
}

fn node_text(node: tree_sitter::Node, source: &str) -> String {
    source[node.byte_range()].to_string()
}

/// Extract Go imports from single and grouped import declarations.
pub fn extract_imports(
    _tree: &tree_sitter::Tree,
    source: &str,
    source_path: &str,
) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let mut in_group = false;

    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("import (") {
            in_group = true;
            continue;
        }
        if in_group {
            if trimmed == ")" {
                in_group = false;
                continue;
            }
            if let Some((target_path, alias)) = parse_go_import_line(trimmed) {
                imports.push(RawImport {
                    source_qualified_name: format!("file::{}", source_path),
                    target_qualified_name: target_path.clone(),
                    target_name: alias.unwrap_or_else(|| {
                        target_path.rsplit('/').next().unwrap_or("").to_string()
                    }),
                    import_line: (idx + 1) as u32,
                });
            }
            continue;
        }

        if trimmed.starts_with("import ")
            && let Some((target_path, alias)) =
                parse_go_import_line(trimmed.trim_start_matches("import ").trim())
        {
            imports.push(RawImport {
                source_qualified_name: format!("file::{}", source_path),
                target_qualified_name: target_path.clone(),
                target_name: alias
                    .unwrap_or_else(|| target_path.rsplit('/').next().unwrap_or("").to_string()),
                import_line: (idx + 1) as u32,
            });
        }
    }

    imports
}

fn parse_go_import_line(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // alias "pkg/path"
    if let Some((alias, rest)) = trimmed.split_once(' ') {
        let target_path = extract_go_path(rest.trim())?;
        let alias = alias.trim();
        let alias_value = if alias == "_" || alias == "." || alias.is_empty() {
            None
        } else {
            Some(alias.to_string())
        };
        return Some((target_path, alias_value));
    }

    // "pkg/path"
    extract_go_path(trimmed).map(|p| (p, None))
}

fn extract_go_path(fragment: &str) -> Option<String> {
    let start = fragment.find('"')?;
    let rest = &fragment[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::extract_imports;
    use crate::parser;
    use std::collections::HashSet;

    #[test]
    fn extract_imports_handles_single_grouped_and_aliased_forms() {
        let source = r#"
import "fmt"
import (
    "github.com/org/pkg/auth"
    cfg "github.com/org/pkg/config"
)
"#;
        let tree = parser::parse_file(source, "go").unwrap();
        let imports = extract_imports(&tree, source, "main.go");
        let qualified_targets: HashSet<String> = imports
            .iter()
            .map(|item| item.target_qualified_name.clone())
            .collect();
        assert!(qualified_targets.contains("fmt"));
        assert!(qualified_targets.contains("github.com/org/pkg/auth"));
        assert!(qualified_targets.contains("github.com/org/pkg/config"));

        let target_names: HashSet<String> =
            imports.into_iter().map(|item| item.target_name).collect();
        assert!(target_names.contains("fmt"));
        assert!(target_names.contains("auth"));
        assert!(target_names.contains("cfg"));
    }
}
