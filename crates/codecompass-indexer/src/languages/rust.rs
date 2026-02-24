use super::ExtractedSymbol;
use crate::import_extract::RawImport;
use codecompass_core::types::SymbolKind;

/// Extract symbols from a Rust syntax tree.
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
    let kind_str = node.kind();

    match kind_str {
        "function_item" => {
            if let Some(sym) = extract_function(node, source, parent) {
                symbols.push(sym);
            }
        }
        "struct_item" => {
            if let Some(sym) = extract_named_item(node, source, parent, SymbolKind::Struct) {
                let name = sym.name.clone();
                symbols.push(sym);
                // Extract methods inside impl blocks are handled separately
                extract_children(node, source, Some(&name), symbols);
                return;
            }
        }
        "enum_item" => {
            if let Some(sym) = extract_named_item(node, source, parent, SymbolKind::Enum) {
                symbols.push(sym);
            }
        }
        "trait_item" => {
            if let Some(sym) = extract_named_item(node, source, parent, SymbolKind::Trait) {
                let name = sym.name.clone();
                symbols.push(sym);
                extract_children(node, source, Some(&name), symbols);
                return;
            }
        }
        "impl_item" => {
            // Get the type name being implemented
            let type_name = node
                .child_by_field_name("type")
                .map(|n| node_text(n, source));
            extract_children(node, source, type_name.as_deref(), symbols);
            return;
        }
        "const_item" | "static_item" => {
            if let Some(sym) = extract_named_item(node, source, parent, SymbolKind::Constant) {
                symbols.push(sym);
            }
        }
        "type_item" => {
            if let Some(sym) = extract_named_item(node, source, parent, SymbolKind::TypeAlias) {
                symbols.push(sym);
            }
        }
        "mod_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(name_node, source);
                symbols.push(ExtractedSymbol {
                    name: name.clone(),
                    qualified_name: make_qualified(parent, &name),
                    kind: SymbolKind::Module,
                    language: "rust".into(),
                    signature: None,
                    line_start: node.start_position().row as u32 + 1,
                    line_end: node.end_position().row as u32 + 1,
                    visibility: extract_visibility(node, source),
                    parent_name: parent.map(String::from),
                    body: Some(node_text(node, source)),
                });
                extract_children(node, source, Some(&name), symbols);
                return;
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

    // Build signature from the function definition line
    let sig = extract_signature(node, source);

    let kind = if parent.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    Some(ExtractedSymbol {
        name: name.clone(),
        qualified_name: make_qualified(parent, &name),
        kind,
        language: "rust".into(),
        signature: Some(sig),
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: extract_visibility(node, source),
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn extract_named_item(
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
        language: "rust".into(),
        signature: None,
        line_start: node.start_position().row as u32 + 1,
        line_end: node.end_position().row as u32 + 1,
        visibility: extract_visibility(node, source),
        parent_name: parent.map(String::from),
        body: Some(node_text(node, source)),
    })
}

fn extract_signature(node: tree_sitter::Node, source: &str) -> String {
    // Take the first line of the function as the signature
    let text = node_text(node, source);
    text.lines().next().unwrap_or("").trim().to_string()
}

fn extract_visibility(node: tree_sitter::Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i)
            && child.kind() == "visibility_modifier"
        {
            return Some(node_text(child, source));
        }
    }
    None
}

fn make_qualified(parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(p) => format!("{}::{}", p, name),
        None => name.to_string(),
    }
}

fn node_text(node: tree_sitter::Node, source: &str) -> String {
    source[node.byte_range()].to_string()
}

/// Extract Rust `use` imports from source text.
pub fn extract_imports(
    _tree: &tree_sitter::Tree,
    source: &str,
    source_path: &str,
) -> Vec<RawImport> {
    let mut results = Vec::new();
    let mut buffer = String::new();
    let mut start_line = 0usize;
    let mut in_use_stmt = false;

    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        // Skip comment lines
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }
        if !in_use_stmt && !trimmed.starts_with("use ") {
            continue;
        }

        if !in_use_stmt {
            in_use_stmt = true;
            start_line = idx + 1;
            buffer.clear();
        }

        if !buffer.is_empty() {
            buffer.push(' ');
        }
        buffer.push_str(trimmed);

        if trimmed.ends_with(';') {
            in_use_stmt = false;
            for target in parse_use_targets(&buffer) {
                let target_name = target
                    .trim_end_matches("::*")
                    .rsplit("::")
                    .next()
                    .unwrap_or("")
                    .to_string();
                results.push(RawImport {
                    source_qualified_name: format!("file::{}", source_path),
                    target_qualified_name: target,
                    target_name,
                    import_line: start_line as u32,
                });
            }
            buffer.clear();
        }
    }

    results
}

fn parse_use_targets(statement: &str) -> Vec<String> {
    let mut stmt = statement.trim();
    if let Some(rest) = stmt.strip_prefix("use ") {
        stmt = rest;
    }
    stmt = stmt.trim_end_matches(';').trim();
    expand_use_expr(stmt)
        .into_iter()
        .map(|s| normalize_rust_target(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

fn expand_use_expr(expr: &str) -> Vec<String> {
    let expr = expr.trim();
    let Some(open_idx) = expr.find('{') else {
        return vec![expr.to_string()];
    };
    let Some(close_idx) = expr.rfind('}') else {
        return vec![expr.to_string()];
    };
    if close_idx <= open_idx {
        return vec![expr.to_string()];
    }

    let prefix = expr[..open_idx].trim_end_matches("::").trim();
    let inner = &expr[open_idx + 1..close_idx];
    let suffix = expr[close_idx + 1..].trim();

    let mut targets = Vec::new();
    for part in split_top_level(inner) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let merged = if prefix.is_empty() {
            part.to_string()
        } else {
            format!("{}::{}", prefix, part)
        };
        for expanded in expand_use_expr(&merged) {
            let with_suffix = if suffix.is_empty() {
                expanded
            } else {
                format!("{}{}", expanded, suffix)
            };
            targets.push(with_suffix);
        }
    }
    targets
}

fn split_top_level(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in input.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn normalize_rust_target(target: &str) -> String {
    let mut cleaned = target.trim().to_string();
    if let Some((lhs, _rhs)) = cleaned.split_once(" as ") {
        cleaned = lhs.trim().to_string();
    }
    cleaned = cleaned.replace("self::", "");
    if let Some(rest) = cleaned.strip_prefix("crate::") {
        cleaned = rest.to_string();
    }
    // Keep super:: prefix — it signals a relative import that may match
    // a sibling module's qualified name.
    if cleaned.ends_with("::self") {
        cleaned = cleaned.trim_end_matches("::self").to_string();
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::extract_imports;
    use crate::parser;
    use std::collections::HashSet;

    #[test]
    fn extract_imports_simple_use_item() {
        let source = "use crate::auth::Claims;";
        let tree = parser::parse_file(source, "rust").unwrap();
        let imports = extract_imports(&tree, source, "src/lib.rs");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|item| item.target_qualified_name)
            .collect();
        assert!(targets.contains("auth::Claims"));
    }

    #[test]
    fn extract_imports_nested_use_expands_to_multiple_targets() {
        let source = "use a::{b, c};";
        let tree = parser::parse_file(source, "rust").unwrap();
        let imports = extract_imports(&tree, source, "src/lib.rs");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|item| item.target_qualified_name)
            .collect();
        assert!(targets.contains("a::b"));
        assert!(targets.contains("a::c"));
    }

    #[test]
    fn extract_imports_glob_use_keeps_wildcard_target() {
        let source = "use a::*;";
        let tree = parser::parse_file(source, "rust").unwrap();
        let imports = extract_imports(&tree, source, "src/lib.rs");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].target_qualified_name, "a::*");
    }
}
