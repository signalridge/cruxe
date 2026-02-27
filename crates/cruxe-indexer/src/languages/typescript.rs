use super::{ExtractedCallSite, ExtractedSymbol};
use crate::import_extract::RawImport;
use cruxe_core::types::SymbolKind;
use std::path::{Component, Path, PathBuf};

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
        "function_declaration" | "function" => {
            if let Some(sym) = extract_function(node, source, parent) {
                symbols.push(sym);
            }
        }
        "class_declaration" => {
            if let Some(sym) = extract_named(node, source, parent, SymbolKind::Class) {
                let name = sym.name.clone();
                symbols.push(sym);
                extract_children(node, source, Some(&name), symbols);
                return;
            }
        }
        "interface_declaration" => {
            if let Some(sym) = extract_named(node, source, parent, SymbolKind::Interface) {
                symbols.push(sym);
            }
        }
        "enum_declaration" => {
            if let Some(sym) = extract_named(node, source, parent, SymbolKind::Enum) {
                symbols.push(sym);
            }
        }
        "type_alias_declaration" => {
            if let Some(sym) = extract_named(node, source, parent, SymbolKind::TypeAlias) {
                symbols.push(sym);
            }
        }
        "method_definition" => {
            if let Some(sym) = extract_method(node, source, parent) {
                symbols.push(sym);
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            // Extract const/let/var declarations
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() != "variable_declarator" {
                        continue;
                    }
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = node_text(name_node, source);
                        symbols.push(ExtractedSymbol {
                            name: name.clone(),
                            qualified_name: make_qualified(parent, &name),
                            kind: SymbolKind::Constant,
                            language: "typescript".into(),
                            signature: None,
                            line_start: node.start_position().row as u32 + 1,
                            line_end: node.end_position().row as u32 + 1,
                            visibility: None,
                            parent_name: parent.map(String::from),
                            body: Some(node_text(node, source)),
                        });
                    }
                }
            }
        }
        "export_statement" => {
            // Look inside export for declarations
            extract_children(node, source, parent, symbols);
            return;
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

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind: if parent.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        },
        language: "typescript".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: None,
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn extract_method(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);
    let sig = node_text(node, source).lines().next()?.trim().to_string();

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind: SymbolKind::Method,
        language: "typescript".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: None,
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn extract_named(
    node: tree_sitter::Node,
    source: &str,
    parent: Option<&str>,
    kind: SymbolKind,
) -> Option<ExtractedSymbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind,
        language: "typescript".into(),
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

/// Extract TypeScript call-sites using `call_expression` and `new_expression`.
pub fn extract_call_sites(tree: &tree_sitter::Tree, source: &str) -> Vec<ExtractedCallSite> {
    let mut calls = Vec::new();
    collect_call_sites(tree.root_node(), source, &mut calls);
    calls
}

fn collect_call_sites(node: tree_sitter::Node, source: &str, calls: &mut Vec<ExtractedCallSite>) {
    match node.kind() {
        "call_expression" | "new_expression" => {
            if let Some(call) = parse_call_node(node, source) {
                calls.push(call);
            }
        }
        _ => {}
    }
    for idx in 0..node.child_count() {
        if let Some(child) = node.child(idx) {
            collect_call_sites(child, source, calls);
        }
    }
}

fn parse_call_node(node: tree_sitter::Node, source: &str) -> Option<ExtractedCallSite> {
    let text = node_text(node, source);
    let mut prefix = text.split('(').next()?.trim().to_string();
    if prefix.starts_with("new ") {
        prefix = prefix.trim_start_matches("new ").trim().to_string();
    }
    let normalized = normalize_call_target(&prefix)?;
    let confidence = if prefix.contains('.') || prefix.contains("?.") || prefix.contains('[') {
        "heuristic"
    } else {
        "static"
    };
    Some(ExtractedCallSite {
        callee_name: normalized,
        line: node.start_position().row as u32 + 1,
        confidence: confidence.to_string(),
    })
}

fn normalize_call_target(prefix: &str) -> Option<String> {
    let value = prefix
        .trim()
        .trim_end_matches('?')
        .trim_end_matches('!')
        .trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

/// Extract TypeScript/JavaScript imports and require() calls.
pub fn extract_imports(
    _tree: &tree_sitter::Tree,
    source: &str,
    source_path: &str,
) -> Vec<RawImport> {
    let mut results = Vec::new();
    let mut buffer = String::new();
    let mut start_line = 0usize;
    let mut in_multiline = false;

    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        if in_multiline {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(trimmed);
            if trimmed.contains(" from ") || trimmed.ends_with(';') {
                in_multiline = false;
                let joined = buffer.trim().to_string();
                if joined.starts_with("import ") || joined.contains(" from ") {
                    results.extend(parse_es_module_import(&joined, source_path, start_line));
                }
                buffer.clear();
            }
            continue;
        }

        if (trimmed.starts_with("import ")
            || (trimmed.starts_with("export ") && trimmed.contains(" from ")))
            && !trimmed.contains(" from ")
        {
            // Multi-line import: e.g. `import {` without `from` on same line
            in_multiline = true;
            start_line = idx + 1;
            buffer.clear();
            buffer.push_str(trimmed);
            continue;
        }

        if trimmed.starts_with("import ")
            || (trimmed.starts_with("export ") && trimmed.contains(" from "))
        {
            results.extend(parse_es_module_import(trimmed, source_path, idx + 1));
            continue;
        }
        if trimmed.contains("require(") {
            results.extend(parse_require_call(trimmed, source_path, idx + 1));
        }
    }

    results
}

fn parse_es_module_import(statement: &str, source_path: &str, line_no: usize) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let Some((left, right)) = statement.split_once(" from ") else {
        return imports;
    };
    let Some(module_spec) = extract_quoted(right) else {
        return imports;
    };

    let left = left
        .trim_start_matches("import ")
        .trim_start_matches("export ")
        .trim();
    let source_qualified_name = format!("file::{}", source_path);
    let resolved_module = resolve_module_path(source_path, &module_spec);

    if left.starts_with('{') {
        let inner = left.trim_start_matches('{').trim_end_matches('}').trim();
        for part in inner.split(',') {
            let name = part.trim().split(" as ").next().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            imports.push(RawImport {
                source_qualified_name: source_qualified_name.clone(),
                target_qualified_name: format!("{}::{}", resolved_module, name),
                target_name: name.to_string(),
                import_line: line_no as u32,
            });
        }
        return imports;
    }

    if left.starts_with("* as ") {
        let ns = left.trim_start_matches("* as ").trim();
        imports.push(RawImport {
            source_qualified_name,
            target_qualified_name: format!("{}::*", resolved_module),
            target_name: ns.to_string(),
            import_line: line_no as u32,
        });
        return imports;
    }

    let default_name = left.split(',').next().unwrap_or("").trim();
    if !default_name.is_empty() {
        imports.push(RawImport {
            source_qualified_name,
            target_qualified_name: format!("{}::{}", resolved_module, default_name),
            target_name: default_name.to_string(),
            import_line: line_no as u32,
        });
    }
    imports
}

fn parse_require_call(statement: &str, source_path: &str, line_no: usize) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let Some(require_idx) = statement.find("require(") else {
        return imports;
    };
    let after = &statement[require_idx + "require(".len()..];
    let Some(end_idx) = after.find(')') else {
        return imports;
    };
    let inner = after[..end_idx].trim();
    let Some(module_spec) = extract_quoted(inner) else {
        return imports;
    };
    let resolved_module = resolve_module_path(source_path, &module_spec);
    let source_qualified_name = format!("file::{}", source_path);

    // Check for destructured require: `const { A, B } = require("...")`
    let lhs = statement.split('=').next().unwrap_or("");
    if let Some(open) = lhs.find('{')
        && let Some(close) = lhs.find('}')
    {
        let inner_names = &lhs[open + 1..close];
        for part in inner_names.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            imports.push(RawImport {
                source_qualified_name: source_qualified_name.clone(),
                target_qualified_name: format!("{}::{}", resolved_module, name),
                target_name: name.to_string(),
                import_line: line_no as u32,
            });
        }
        return imports;
    }

    let target_name = lhs
        .split_whitespace()
        .last()
        .unwrap_or("*")
        .trim()
        .to_string();
    imports.push(RawImport {
        source_qualified_name,
        target_qualified_name: format!("{}::{}", resolved_module, target_name),
        target_name,
        import_line: line_no as u32,
    });
    imports
}

fn resolve_module_path(source_path: &str, module_spec: &str) -> String {
    if !module_spec.starts_with('.') {
        return module_spec.to_string();
    }
    let base = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = normalize_path(&base.join(module_spec));
    joined
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .to_string()
}

fn normalize_path(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(seg) => normalized.push(seg),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized.to_string_lossy().replace('\\', "/")
}

fn extract_quoted(input: &str) -> Option<String> {
    let single = input.find('\'');
    let double = input.find('"');
    let (quote, start) = match (single, double) {
        (Some(s), Some(d)) => {
            if s < d {
                ('\'', s)
            } else {
                ('"', d)
            }
        }
        (Some(s), None) => ('\'', s),
        (None, Some(d)) => ('"', d),
        (None, None) => return None,
    };
    let rest = &input[start + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::extract_imports;
    use crate::parser;
    use std::collections::HashSet;

    #[test]
    fn extract_imports_named_default_namespace_and_require() {
        let source = r#"
import { Router } from "./router";
import AuthClient from "./auth/client";
import * as Utils from "./utils";
const cfg = require("./config");
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let imports = extract_imports(&tree, source, "src/index.ts");

        let target_names: HashSet<String> = imports
            .iter()
            .map(|item| item.target_name.clone())
            .collect();
        assert!(target_names.contains("Router"));
        assert!(target_names.contains("AuthClient"));
        assert!(target_names.contains("Utils"));
        assert!(target_names.contains("cfg"));

        let qualified_targets: HashSet<String> = imports
            .into_iter()
            .map(|item| item.target_qualified_name)
            .collect();
        assert!(qualified_targets.contains("src/router::Router"));
        assert!(qualified_targets.contains("src/auth/client::AuthClient"));
        assert!(qualified_targets.contains("src/utils::*"));
        assert!(qualified_targets.contains("src/config::cfg"));
    }

    #[test]
    fn extract_imports_multiline_import() {
        let source = r#"
import {
  Router,
  Request
} from "./router";
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let imports = extract_imports(&tree, source, "src/index.ts");

        let target_names: HashSet<String> = imports
            .iter()
            .map(|item| item.target_name.clone())
            .collect();
        assert!(target_names.contains("Router"), "missing Router");
        assert!(target_names.contains("Request"), "missing Request");
    }

    #[test]
    fn extract_imports_destructured_require() {
        let source = r#"
const { Router, Request } = require("express");
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let imports = extract_imports(&tree, source, "src/index.ts");

        let target_names: HashSet<String> = imports
            .iter()
            .map(|item| item.target_name.clone())
            .collect();
        assert!(target_names.contains("Router"), "missing Router");
        assert!(target_names.contains("Request"), "missing Request");
    }
}
