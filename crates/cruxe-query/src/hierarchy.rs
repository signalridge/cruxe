use cruxe_core::error::StateError;
use cruxe_state::symbols;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HierarchyDirection {
    Ancestors,
    Descendants,
}

impl HierarchyDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ancestors => "ancestors",
            Self::Descendants => "descendants",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub name: String,
    pub kind: String,
    pub qualified_name: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub depth: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<HierarchyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyResponse {
    pub hierarchy: Vec<HierarchyNode>,
    pub direction: String,
    pub chain_length: usize,
}

#[derive(Debug, Error)]
pub enum HierarchyError {
    #[error("symbol not found")]
    SymbolNotFound,
    #[error("ambiguous symbol match ({count} candidates)")]
    AmbiguousSymbol { count: usize },
    #[error("state error: {0}")]
    State(#[from] StateError),
}

pub fn get_symbol_hierarchy(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_name: &str,
    path: Option<&str>,
    direction: HierarchyDirection,
) -> Result<HierarchyResponse, HierarchyError> {
    let matches = symbols::find_symbols_by_name(conn, repo, ref_name, symbol_name, path)?;
    if matches.is_empty() {
        return Err(HierarchyError::SymbolNotFound);
    }
    if path.is_none() && matches.len() > 1 {
        // Check if matches span multiple distinct files
        let distinct_paths: HashSet<&str> = matches.iter().map(|m| m.path.as_str()).collect();
        if distinct_paths.len() > 1 {
            return Err(HierarchyError::AmbiguousSymbol {
                count: matches.len(),
            });
        }
    }
    let anchor = matches[0].clone();

    match direction {
        HierarchyDirection::Ancestors => {
            let mut nodes = Vec::new();
            let mut visited = HashSet::new();
            let mut current = anchor;
            let mut depth = 0u32;

            loop {
                if !visited.insert(current.symbol_id.clone()) {
                    break;
                }
                nodes.push(to_hierarchy_node(current.clone(), depth, Vec::new()));
                let Some(parent_id) = current.parent_symbol_id.as_deref() else {
                    break;
                };
                let Some(parent) = symbols::get_symbol_by_id(conn, repo, ref_name, parent_id)?
                else {
                    break;
                };
                current = parent;
                depth += 1;
            }

            let chain_length = nodes.len();
            Ok(HierarchyResponse {
                hierarchy: nodes,
                direction: direction.as_str().to_string(),
                chain_length,
            })
        }
        HierarchyDirection::Descendants => {
            let mut visited = HashSet::new();
            let root = build_descendants(conn, repo, ref_name, &anchor, 0, &mut visited)?;
            let chain_length = count_nodes(&root);
            Ok(HierarchyResponse {
                hierarchy: vec![root],
                direction: direction.as_str().to_string(),
                chain_length,
            })
        }
    }
}

fn build_descendants(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol: &cruxe_core::types::SymbolRecord,
    depth: u32,
    visited: &mut HashSet<String>,
) -> Result<HierarchyNode, HierarchyError> {
    if !visited.insert(symbol.symbol_id.clone()) {
        return Ok(to_hierarchy_node(symbol.clone(), depth, Vec::new()));
    }

    let children = symbols::get_children_symbols(conn, repo, ref_name, &symbol.symbol_id)?;
    let mut child_nodes = Vec::new();
    for child in children {
        child_nodes.push(build_descendants(
            conn,
            repo,
            ref_name,
            &child,
            depth + 1,
            visited,
        )?);
    }

    Ok(to_hierarchy_node(symbol.clone(), depth, child_nodes))
}

fn to_hierarchy_node(
    symbol: cruxe_core::types::SymbolRecord,
    depth: u32,
    children: Vec<HierarchyNode>,
) -> HierarchyNode {
    HierarchyNode {
        symbol_id: symbol.symbol_id,
        symbol_stable_id: symbol.symbol_stable_id,
        name: symbol.name,
        kind: symbol.kind.as_str().to_string(),
        qualified_name: symbol.qualified_name,
        path: symbol.path,
        line_start: symbol.line_start,
        line_end: symbol.line_end,
        signature: symbol.signature,
        depth,
        children,
    }
}

fn count_nodes(node: &HierarchyNode) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema};
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    struct TestSymbolInput<'a> {
        symbol_id: &'a str,
        name: &'a str,
        qualified_name: &'a str,
        kind: SymbolKind,
        parent_symbol_id: Option<&'a str>,
        line_start: u32,
    }

    fn insert_symbol(conn: &Connection, input: TestSymbolInput<'_>) {
        insert_symbol_with_path(conn, "src/auth/handler.rs", input);
    }

    fn insert_symbol_with_path(conn: &Connection, path: &str, input: TestSymbolInput<'_>) {
        let TestSymbolInput {
            symbol_id,
            name,
            qualified_name,
            kind,
            parent_symbol_id,
            line_start,
        } = input;
        let record = SymbolRecord {
            repo: "repo".into(),
            r#ref: "main".into(),
            commit: None,
            path: path.into(),
            language: "rust".into(),
            symbol_id: symbol_id.into(),
            symbol_stable_id: format!("stable_{symbol_id}"),
            name: name.into(),
            qualified_name: qualified_name.into(),
            kind,
            signature: None,
            line_start,
            line_end: line_start + 2,
            parent_symbol_id: parent_symbol_id.map(String::from),
            visibility: Some("pub".into()),
            content: None,
        };
        cruxe_state::symbols::insert_symbol(conn, &record).unwrap();
    }

    #[test]
    fn get_symbol_hierarchy_ancestors_chain() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "mod_auth",
                name: "auth",
                qualified_name: "auth",
                kind: SymbolKind::Module,
                parent_symbol_id: None,
                line_start: 1,
            },
        );
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "impl_handler",
                name: "AuthHandler",
                qualified_name: "auth::AuthHandler",
                kind: SymbolKind::Struct,
                parent_symbol_id: Some("mod_auth"),
                line_start: 10,
            },
        );
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "method_validate",
                name: "validate",
                qualified_name: "auth::AuthHandler::validate",
                kind: SymbolKind::Method,
                parent_symbol_id: Some("impl_handler"),
                line_start: 20,
            },
        );

        let response = get_symbol_hierarchy(
            &conn,
            "repo",
            "main",
            "validate",
            Some("src/auth/handler.rs"),
            HierarchyDirection::Ancestors,
        )
        .unwrap();

        assert_eq!(response.chain_length, 3);
        assert_eq!(response.hierarchy[0].name, "validate");
        assert_eq!(response.hierarchy[1].name, "AuthHandler");
        assert_eq!(response.hierarchy[2].name, "auth");
    }

    #[test]
    fn get_symbol_hierarchy_descendants_includes_children() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "struct_handler",
                name: "AuthHandler",
                qualified_name: "auth::AuthHandler",
                kind: SymbolKind::Struct,
                parent_symbol_id: None,
                line_start: 1,
            },
        );
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "method_validate",
                name: "validate",
                qualified_name: "auth::AuthHandler::validate",
                kind: SymbolKind::Method,
                parent_symbol_id: Some("struct_handler"),
                line_start: 10,
            },
        );
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "method_refresh",
                name: "refresh",
                qualified_name: "auth::AuthHandler::refresh",
                kind: SymbolKind::Method,
                parent_symbol_id: Some("struct_handler"),
                line_start: 20,
            },
        );

        let response = get_symbol_hierarchy(
            &conn,
            "repo",
            "main",
            "AuthHandler",
            Some("src/auth/handler.rs"),
            HierarchyDirection::Descendants,
        )
        .unwrap();

        assert_eq!(response.hierarchy.len(), 1);
        assert_eq!(response.hierarchy[0].children.len(), 2);
    }

    #[test]
    fn get_symbol_hierarchy_cycle_detection() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "a",
                name: "A",
                qualified_name: "A",
                kind: SymbolKind::Class,
                parent_symbol_id: Some("b"),
                line_start: 1,
            },
        );
        insert_symbol(
            &conn,
            TestSymbolInput {
                symbol_id: "b",
                name: "B",
                qualified_name: "B",
                kind: SymbolKind::Class,
                parent_symbol_id: Some("a"),
                line_start: 10,
            },
        );

        let response = get_symbol_hierarchy(
            &conn,
            "repo",
            "main",
            "A",
            Some("src/auth/handler.rs"),
            HierarchyDirection::Ancestors,
        )
        .unwrap();
        assert!(response.chain_length <= 2);
    }

    #[test]
    fn get_symbol_hierarchy_without_path_returns_ambiguous_when_multiple_files() {
        let conn = setup_test_db();
        insert_symbol_with_path(
            &conn,
            "src/a.rs",
            TestSymbolInput {
                symbol_id: "a",
                name: "validate",
                qualified_name: "mod_a::validate",
                kind: SymbolKind::Function,
                parent_symbol_id: None,
                line_start: 10,
            },
        );
        insert_symbol_with_path(
            &conn,
            "src/z.rs",
            TestSymbolInput {
                symbol_id: "b",
                name: "validate",
                qualified_name: "mod_b::validate",
                kind: SymbolKind::Function,
                parent_symbol_id: None,
                line_start: 10,
            },
        );

        let err = get_symbol_hierarchy(
            &conn,
            "repo",
            "main",
            "validate",
            None,
            HierarchyDirection::Ancestors,
        )
        .unwrap_err();
        assert!(
            matches!(err, HierarchyError::AmbiguousSymbol { count: 2 }),
            "expected AmbiguousSymbol, got: {err:?}"
        );
    }
}
