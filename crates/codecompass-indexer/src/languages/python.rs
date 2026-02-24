use super::ExtractedSymbol;
use crate::import_extract::RawImport;
use codecompass_core::types::SymbolKind;
use std::path::Path;

pub fn extract(tree: &tree_sitter::Tree, source: &str) -> Vec<ExtractedSymbol> {
    let mut symbols = Vec::new();
    let root = tree.root_node();
    extract_from_node(root, source, None, &mut symbols);
    symbols
}

fn extract_from_node(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
    symbols: &mut Vec<ExtractedSymbol>,
) {
    match node.kind() {
        "function_definition" => {
            if let Some(sym) = extract_function(node, source, parent) {
                symbols.push(sym);
            }
        }
        "class_definition" => {
            if let Some(sym) = extract_class(node, source, parent) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Extract methods inside the class body
                if let Some(body) = node.child_by_field_name("body") {
                    extract_children(body, source, Some(&name), symbols);
                }
                return;
            }
        }
        "decorated_definition" => {
            // Skip decorator, extract the actual definition
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i)
                    && (child.kind() == "function_definition" || child.kind() == "class_definition")
                {
                    extract_from_node(child, source, parent, symbols);
                }
            }
            return;
        }
        "expression_statement" => {
            // Module-level assignments
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() != "assignment" || parent.is_some() {
                        continue;
                    }
                    if let Some(left) = child.child_by_field_name("left") {
                        if left.kind() != "identifier" {
                            continue;
                        }
                        let name = node_text(left, source);
                        symbols.push(ExtractedSymbol {
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Variable,
                            language: "python".into(),
                            signature: None,
                            line_start: node.start_position().row as u32 + 1,
                            line_end: node.end_position().row as u32 + 1,
                            visibility: None,
                            parent_name: None,
                            body: Some(node_text(node, source)),
                        });
                    }
                }
            }
        }
        _ => {}
    }

    extract_children(node, source, parent, symbols);
}

fn extract_children(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
    symbols: &mut Vec<ExtractedSymbol>,
) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_from_node(child, source, parent, symbols);
        }
    }
}

fn extract_function(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);
    let sig = node_text(node, source).lines().next()?.trim().to_string();

    let kind = if parent.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind,
        language: "python".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: if name.starts_with('_') {
            Some("private".into())
        } else {
            Some("public".into())
        },
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn extract_class(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind: SymbolKind::Class,
        language: "python".into(),
        signature: None,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: None,
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn make_qualified(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(p) => format!("{}.{}", p, name),
        None => name.to_string(),
    }
}

fn node_text(node: tree_sitter::Node, source: &str) -> String {
    source[node.byte_range()].to_string()
}

/// Extract Python import statements, including multi-line parenthesized forms.
pub fn extract_imports(
    _tree: &tree_sitter::Tree,
    source: &str,
    source_path: &str,
) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let mut buffer = String::new();
    let mut start_line = 0usize;
    let mut in_paren = false;

    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        if in_paren {
            // Strip closing paren if present
            let content = if let Some(pos) = trimmed.find(')') {
                in_paren = false;
                &trimmed[..pos]
            } else {
                trimmed
            };
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(content);

            if !in_paren {
                let joined = buffer.trim().to_string();
                if joined.starts_with("from ") {
                    imports.extend(parse_from_import_stmt(&joined, source_path, start_line));
                } else if joined.starts_with("import ") {
                    imports.extend(parse_import_stmt(&joined, source_path, start_line));
                }
                buffer.clear();
            }
            continue;
        }

        if (trimmed.starts_with("from ") || trimmed.starts_with("import "))
            && trimmed.contains('(')
            && !trimmed.contains(')')
        {
            // Multi-line parenthesized import: `from X import (`
            in_paren = true;
            start_line = idx + 1;
            buffer.clear();
            // Strip the opening paren for the buffer
            let clean = trimmed.replace('(', "");
            buffer.push_str(clean.trim());
            continue;
        }

        if trimmed.starts_with("import ") {
            imports.extend(parse_import_stmt(trimmed, source_path, idx + 1));
        } else if trimmed.starts_with("from ") {
            imports.extend(parse_from_import_stmt(trimmed, source_path, idx + 1));
        }
    }
    imports
}

fn parse_import_stmt(statement: &str, source_path: &str, line_no: usize) -> Vec<RawImport> {
    let mut results = Vec::new();
    let source_qualified_name = format!("file::{}", source_path);
    let modules = statement.trim_start_matches("import ").split(',');
    for module in modules {
        let module = module.trim();
        if module.is_empty() {
            continue;
        }
        let target_module = module.split(" as ").next().unwrap_or("").trim();
        if target_module.is_empty() {
            continue;
        }
        let target_name = target_module.rsplit('.').next().unwrap_or("").to_string();
        results.push(RawImport {
            source_qualified_name: source_qualified_name.clone(),
            target_qualified_name: target_module.to_string(),
            target_name,
            import_line: line_no as u32,
        });
    }
    results
}

fn parse_from_import_stmt(statement: &str, source_path: &str, line_no: usize) -> Vec<RawImport> {
    let mut results = Vec::new();
    let source_qualified_name = format!("file::{}", source_path);
    let body = statement.trim_start_matches("from ").trim();
    let Some((module_raw, imports_raw)) = body.split_once(" import ") else {
        return results;
    };
    let resolved_module = resolve_python_module(source_path, module_raw.trim());
    for imported in imports_raw.split(',') {
        let imported = imported.trim();
        if imported.is_empty() {
            continue;
        }
        let imported_name = imported.split(" as ").next().unwrap_or("").trim();
        if imported_name.is_empty() {
            continue;
        }
        let target_qualified_name = if imported_name == "*" {
            resolved_module.clone()
        } else if resolved_module.is_empty() {
            imported_name.to_string()
        } else {
            format!("{}.{}", resolved_module, imported_name)
        };
        results.push(RawImport {
            source_qualified_name: source_qualified_name.clone(),
            target_qualified_name,
            target_name: imported_name.to_string(),
            import_line: line_no as u32,
        });
    }
    results
}

fn resolve_python_module(source_path: &str, module: &str) -> String {
    if !module.starts_with('.') {
        return module.to_string();
    }
    let parent = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let parent_parts: Vec<String> = parent
        .components()
        .filter_map(|c| {
            let s = c.as_os_str().to_string_lossy().to_string();
            if s.is_empty() || s == "." {
                None
            } else {
                Some(s)
            }
        })
        .collect();

    let dot_count = module.chars().take_while(|c| *c == '.').count();
    let suffix = module.trim_start_matches('.');
    let keep_len = parent_parts
        .len()
        .saturating_sub(dot_count.saturating_sub(1));
    let mut base = parent_parts.into_iter().take(keep_len).collect::<Vec<_>>();
    if !suffix.is_empty() {
        base.push(suffix.to_string());
    }
    base.join(".")
}

#[cfg(test)]
mod tests {
    use super::extract_imports;
    use crate::parser;
    use std::collections::HashSet;

    #[test]
    fn extract_imports_handles_absolute_from_relative_and_alias_forms() {
        let source = r#"
import os
from auth.jwt import validate_token
from .models import User as AppUser
"#;
        let tree = parser::parse_file(source, "python").unwrap();
        let imports = extract_imports(&tree, source, "pkg/handlers.py");
        let qualified_targets: HashSet<String> = imports
            .iter()
            .map(|item| item.target_qualified_name.clone())
            .collect();
        assert!(qualified_targets.contains("os"));
        assert!(qualified_targets.contains("auth.jwt.validate_token"));
        assert!(qualified_targets.contains("pkg.models.User"));

        let target_names: HashSet<String> =
            imports.into_iter().map(|item| item.target_name).collect();
        assert!(target_names.contains("os"));
        assert!(target_names.contains("validate_token"));
        assert!(target_names.contains("User"));
    }

    #[test]
    fn extract_imports_multiline_parenthesized() {
        let source = r#"
from auth.jwt import (
    validate_token,
    refresh_token,
    Claims,
)
"#;
        let tree = parser::parse_file(source, "python").unwrap();
        let imports = extract_imports(&tree, source, "pkg/handlers.py");

        let target_names: HashSet<String> =
            imports.into_iter().map(|item| item.target_name).collect();
        assert!(
            target_names.contains("validate_token"),
            "missing validate_token"
        );
        assert!(
            target_names.contains("refresh_token"),
            "missing refresh_token"
        );
        assert!(target_names.contains("Claims"), "missing Claims");
    }
}
