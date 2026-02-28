use super::ExtractedCallSite;
use super::text::node_text_owned;
use crate::import_extract::RawImport;

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

/// Extract Go call-sites using `call_expression` nodes.
pub fn extract_call_sites(tree: &tree_sitter::Tree, source: &str) -> Vec<ExtractedCallSite> {
    let mut calls = Vec::new();
    collect_call_sites(tree.root_node(), source, &mut calls);
    calls
}

fn collect_call_sites(node: tree_sitter::Node, source: &str, calls: &mut Vec<ExtractedCallSite>) {
    if node.kind() == "call_expression"
        && let Some(call) = parse_call_node(node, source)
    {
        calls.push(call);
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
    let confidence = if prefix.contains('.') {
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
    let value = prefix.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
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
