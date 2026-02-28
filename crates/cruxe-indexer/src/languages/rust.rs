use super::ExtractedCallSite;
use super::text::node_text_owned;
use crate::import_extract::RawImport;

/// Extract Rust call-sites using `call_expression` and `method_call_expression` nodes.
pub fn extract_call_sites(tree: &tree_sitter::Tree, source: &str) -> Vec<ExtractedCallSite> {
    let mut calls = Vec::new();
    collect_call_sites(tree.root_node(), source, &mut calls);
    calls
}

fn collect_call_sites(node: tree_sitter::Node, source: &str, calls: &mut Vec<ExtractedCallSite>) {
    match node.kind() {
        "call_expression" | "method_call_expression" => {
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
    let text = node_text_owned(node, source);
    let prefix = text.split('(').next()?.trim();
    let normalized = normalize_call_target(prefix)?;
    let confidence =
        if node.kind() == "method_call_expression" || prefix.contains('.') || prefix.contains("->")
        {
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
    let mut value = prefix
        .trim()
        .trim_start_matches('&')
        .trim_start_matches('*');
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("self.") {
        value = stripped;
    }
    let value = value.trim_end_matches('!').trim_end_matches('?').trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
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
