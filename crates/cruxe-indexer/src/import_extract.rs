use crate::languages;
use cruxe_core::error::StateError;
use cruxe_core::types::SymbolEdge;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Raw import extracted from source code before symbol resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawImport {
    pub source_qualified_name: String,
    pub target_qualified_name: String,
    pub target_name: String,
    pub import_line: u32,
}

/// Deterministic pseudo symbol ID for file-scoped import edges.
pub fn source_symbol_id_for_path(path: &str) -> String {
    format!("file::{}", path)
}

/// Extract raw imports by language.
pub fn extract_imports(
    tree: &tree_sitter::Tree,
    source: &str,
    language: &str,
    source_path: &str,
) -> Vec<RawImport> {
    match language {
        "rust" => languages::rust::extract_imports(tree, source, source_path),
        "typescript" => languages::typescript::extract_imports(tree, source, source_path),
        "python" => languages::python::extract_imports(tree, source, source_path),
        "go" => languages::go::extract_imports(tree, source, source_path),
        _ => Vec::new(),
    }
}

/// Resolve raw imports to SymbolEdge records.
///
/// `to_symbol_id` uses target `symbol_stable_id` when resolved; unresolved targets
/// use a deterministic stable ID derived from `blake3("unresolved:" + qualified_name)`.
pub fn resolve_imports(
    conn: &Connection,
    raw_imports: Vec<RawImport>,
    repo: &str,
    ref_name: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    for raw in raw_imports {
        let to_symbol_id = resolve_target_symbol_stable_id(conn, repo, ref_name, &raw)?
            .unwrap_or_else(|| unresolved_symbol_stable_id(&raw.target_qualified_name));

        let edge = SymbolEdge {
            repo: repo.to_string(),
            ref_name: ref_name.to_string(),
            from_symbol_id: raw.source_qualified_name,
            to_symbol_id,
            edge_type: "imports".to_string(),
            confidence: "static".to_string(),
        };

        let dedupe_key = (
            edge.repo.clone(),
            edge.ref_name.clone(),
            edge.from_symbol_id.clone(),
            edge.to_symbol_id.clone(),
            edge.edge_type.clone(),
        );
        if seen.insert(dedupe_key) {
            edges.push(edge);
        }
    }

    Ok(edges)
}

fn resolve_target_symbol_stable_id(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    raw: &RawImport,
) -> Result<Option<String>, StateError> {
    if !raw.target_qualified_name.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_stable_id FROM symbol_relations
                 WHERE repo = ?1 AND \"ref\" = ?2 AND qualified_name = ?3
                 LIMIT 1",
            )
            .map_err(StateError::sqlite)?;
        let exact = stmt
            .query_row(params![repo, ref_name, raw.target_qualified_name], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        if exact.is_some() {
            return Ok(exact);
        }
    }

    if !raw.target_name.is_empty() {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_stable_id FROM symbol_relations
                 WHERE repo = ?1 AND \"ref\" = ?2 AND name = ?3
                 ORDER BY line_start
                 LIMIT 1",
            )
            .map_err(StateError::sqlite)?;
        let by_name = stmt
            .query_row(params![repo, ref_name, raw.target_name], |row| {
                row.get::<_, String>(0)
            })
            .ok();
        if by_name.is_some() {
            return Ok(by_name);
        }
    }

    Ok(None)
}

fn unresolved_symbol_stable_id(target_qualified_name: &str) -> String {
    let input = format!("unresolved:{}", target_qualified_name);
    blake3::hash(input.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema, symbols};
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn insert_symbol(conn: &Connection, name: &str, qualified_name: &str, stable_id: &str) {
        let record = SymbolRecord {
            repo: "my-repo".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: "src/auth.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: format!("sym_{name}"),
            symbol_stable_id: stable_id.to_string(),
            name: name.to_string(),
            qualified_name: qualified_name.to_string(),
            kind: SymbolKind::Struct,
            signature: None,
            line_start: 1,
            line_end: 10,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        };
        symbols::insert_symbol(conn, &record).unwrap();
    }

    #[test]
    fn resolve_imports_prefers_exact_qualified_name() {
        let conn = setup_test_db();
        insert_symbol(&conn, "Claims", "auth::Claims", "stable_claims");

        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "auth::Claims".to_string(),
            target_name: "Claims".to_string(),
            import_line: 3,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_symbol_id, "stable_claims");
    }

    #[test]
    fn resolve_imports_fallbacks_to_name() {
        let conn = setup_test_db();
        insert_symbol(&conn, "Claims", "Claims", "stable_claims_by_name");

        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: "auth::Claims".to_string(),
            target_name: "Claims".to_string(),
            import_line: 3,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_symbol_id, "stable_claims_by_name");
    }

    #[test]
    fn resolve_imports_generates_unresolved_id() {
        let conn = setup_test_db();
        let target = "missing::Symbol".to_string();
        let imports = vec![RawImport {
            source_qualified_name: source_symbol_id_for_path("src/lib.rs"),
            target_qualified_name: target.clone(),
            target_name: "Symbol".to_string(),
            import_line: 8,
        }];

        let edges = resolve_imports(&conn, imports, "my-repo", "main").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_symbol_id, unresolved_symbol_stable_id(&target));
    }

    #[test]
    fn extract_imports_dispatch_rust_nested_use() {
        let source = r#"
use crate::auth::Claims;
use crate::auth::{validate_token, refresh_token};
use crate::db::*;
use crate::types::User as AppUser;
"#;
        let tree = parser::parse_file(source, "rust").unwrap();
        let imports = extract_imports(&tree, source, "rust", "src/lib.rs");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("auth::Claims"));
        assert!(targets.contains("auth::validate_token"));
        assert!(targets.contains("auth::refresh_token"));
        assert!(targets.contains("db::*"));
        assert!(targets.contains("types::User"));
    }

    #[test]
    fn extract_imports_dispatch_typescript_variants() {
        let source = r#"
import { Router } from "./router";
import AuthClient from "./auth/client";
import * as Utils from "./utils";
const cfg = require("./config");
"#;
        let tree = parser::parse_file(source, "typescript").unwrap();
        let imports = extract_imports(&tree, source, "typescript", "src/index.ts");
        let names: HashSet<String> = imports.into_iter().map(|i| i.target_name).collect();
        assert!(names.contains("Router"));
        assert!(names.contains("AuthClient"));
        assert!(names.contains("Utils"));
        assert!(names.contains("cfg"));
    }

    #[test]
    fn extract_imports_dispatch_python_variants() {
        let source = r#"
import os
from auth.jwt import validate_token, Claims
from .models import User as AppUser
"#;
        let tree = parser::parse_file(source, "python").unwrap();
        let imports = extract_imports(&tree, source, "python", "pkg/handlers.py");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("os"));
        assert!(targets.contains("auth.jwt.validate_token"));
        assert!(targets.contains("auth.jwt.Claims"));
        assert!(targets.contains("pkg.models.User"));
    }

    #[test]
    fn extract_imports_dispatch_go_variants() {
        let source = r#"
import "fmt"
import (
    "github.com/org/pkg/auth"
    cfg "github.com/org/pkg/config"
)
"#;
        let tree = parser::parse_file(source, "go").unwrap();
        let imports = extract_imports(&tree, source, "go", "main.go");
        let targets: HashSet<String> = imports
            .into_iter()
            .map(|i| i.target_qualified_name)
            .collect();
        assert!(targets.contains("fmt"));
        assert!(targets.contains("github.com/org/pkg/auth"));
        assert!(targets.contains("github.com/org/pkg/config"));
    }
}
