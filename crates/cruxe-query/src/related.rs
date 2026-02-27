use cruxe_core::error::StateError;
use cruxe_indexer::import_extract::source_symbol_id_for_path;
use cruxe_state::{edges, symbols};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelatedScope {
    File,
    Module,
    Package,
}

impl RelatedScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Package => "package",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedAnchor {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line_start: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedSymbol {
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
    pub relation: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedResponse {
    pub anchor: RelatedAnchor,
    pub related: Vec<RelatedSymbol>,
    pub scope_used: String,
    pub total_found: usize,
}

#[derive(Debug, Error)]
pub enum RelatedError {
    #[error("symbol not found")]
    SymbolNotFound,
    #[error("ambiguous symbol match ({count} candidates)")]
    AmbiguousSymbol { count: usize },
    #[error("state error: {0}")]
    State(#[from] StateError),
}

pub fn find_related_symbols(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_name: &str,
    path: Option<&str>,
    scope: RelatedScope,
    limit: usize,
) -> Result<RelatedResponse, RelatedError> {
    let matches = symbols::find_symbols_by_name(conn, repo, ref_name, symbol_name, path)?;
    if matches.is_empty() {
        return Err(RelatedError::SymbolNotFound);
    }
    if path.is_none() && matches.len() > 1 {
        let distinct_paths: HashSet<&str> = matches.iter().map(|m| m.path.as_str()).collect();
        if distinct_paths.len() > 1 {
            return Err(RelatedError::AmbiguousSymbol {
                count: matches.len(),
            });
        }
    }
    let anchor = matches[0].clone();
    let mut related_map: HashMap<String, (u8, RelatedSymbol)> = HashMap::new();
    let mut visited = HashSet::new();
    visited.insert(anchor.symbol_id.clone());

    // same-file scope
    for symbol in symbols::list_symbols_in_file(conn, repo, ref_name, &anchor.path)? {
        if !visited.insert(symbol.symbol_id.clone()) {
            continue;
        }
        let related = to_related_symbol(symbol, "same_file");
        related_map.insert(related.symbol_id.clone(), (0, related));
    }

    if matches!(scope, RelatedScope::Module | RelatedScope::Package) {
        let module_prefix = module_prefix(&anchor.path);
        if !module_prefix.is_empty() {
            for symbol in
                symbols::list_symbols_by_path_prefix(conn, repo, ref_name, &module_prefix)?
            {
                if !visited.insert(symbol.symbol_id.clone()) || symbol.path == anchor.path {
                    continue;
                }
                let related = to_related_symbol(symbol, "same_module");
                related_map
                    .entry(related.symbol_id.clone())
                    .or_insert((1, related));
            }
        }
    }

    if matches!(scope, RelatedScope::Package) {
        let package_prefix = package_prefix(&anchor.path);
        if !package_prefix.is_empty() {
            for symbol in
                symbols::list_symbols_by_path_prefix(conn, repo, ref_name, &package_prefix)?
            {
                if !visited.insert(symbol.symbol_id.clone()) {
                    continue;
                }
                let related = to_related_symbol(symbol, "same_package");
                related_map
                    .entry(related.symbol_id.clone())
                    .or_insert((2, related));
            }
        }
    }

    if matches!(scope, RelatedScope::Module | RelatedScope::Package) {
        let mut edge_source_ids = vec![
            anchor.symbol_id.clone(),
            source_symbol_id_for_path(&anchor.path),
        ];
        edge_source_ids.sort();
        edge_source_ids.dedup();

        for source_id in edge_source_ids {
            for edge in edges::get_edges_from(conn, repo, ref_name, &source_id)? {
                if edge.edge_type != "imports" {
                    continue;
                }
                let Some(target_symbol) =
                    symbols::get_symbol_by_stable_id(conn, repo, ref_name, &edge.to_symbol_id)?
                else {
                    continue;
                };
                if !visited.insert(target_symbol.symbol_id.clone()) {
                    continue;
                }
                let related = to_related_symbol(target_symbol, "imported");
                related_map
                    .entry(related.symbol_id.clone())
                    .or_insert((3, related));
            }
        }
    }

    let mut related = related_map.into_values().collect::<Vec<_>>();
    related.sort_by(|(pa, a), (pb, b)| {
        pa.cmp(pb)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    let total_found = related.len();
    let related = related
        .into_iter()
        .take(limit)
        .map(|(_priority, symbol)| symbol)
        .collect::<Vec<_>>();

    Ok(RelatedResponse {
        anchor: RelatedAnchor {
            symbol_id: anchor.symbol_id,
            symbol_stable_id: anchor.symbol_stable_id,
            name: anchor.name,
            kind: anchor.kind.as_str().to_string(),
            path: anchor.path,
            line_start: anchor.line_start,
        },
        related,
        scope_used: scope.as_str().to_string(),
        total_found,
    })
}

fn module_prefix(path: &str) -> String {
    let prefix = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    if prefix.is_empty() {
        prefix
    } else {
        format!("{}/", prefix)
    }
}

fn package_prefix(path: &str) -> String {
    let p = Path::new(path);
    let mut comps = p.components();
    let prefix = comps
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_default();
    if prefix.is_empty() {
        prefix
    } else {
        format!("{}/", prefix)
    }
}

fn to_related_symbol(symbol: cruxe_core::types::SymbolRecord, relation: &str) -> RelatedSymbol {
    RelatedSymbol {
        symbol_id: symbol.symbol_id,
        symbol_stable_id: symbol.symbol_stable_id,
        name: symbol.name,
        kind: symbol.kind.as_str().to_string(),
        qualified_name: symbol.qualified_name,
        path: symbol.path,
        line_start: symbol.line_start,
        line_end: symbol.line_end,
        signature: symbol.signature,
        relation: relation.to_string(),
        language: symbol.language,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::{SymbolEdge, SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema};
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn insert_symbol(
        conn: &Connection,
        symbol_id: &str,
        stable_id: &str,
        name: &str,
        path: &str,
        line_start: u32,
    ) {
        let record = SymbolRecord {
            repo: "repo".into(),
            r#ref: "main".into(),
            commit: None,
            path: path.into(),
            language: "rust".into(),
            symbol_id: symbol_id.into(),
            symbol_stable_id: stable_id.into(),
            name: name.into(),
            qualified_name: format!("mod::{name}"),
            kind: SymbolKind::Function,
            signature: Some(format!("fn {name}()")),
            line_start,
            line_end: line_start + 1,
            parent_symbol_id: None,
            visibility: Some("pub".into()),
            content: None,
        };
        symbols::insert_symbol(conn, &record).unwrap();
    }

    #[test]
    fn find_related_symbols_scope_file() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            "a",
            "stable_a",
            "validate",
            "src/auth/handler.rs",
            10,
        );
        insert_symbol(&conn, "b", "stable_b", "parse", "src/auth/handler.rs", 20);
        insert_symbol(&conn, "c", "stable_c", "other", "src/auth/other.rs", 30);

        let response = find_related_symbols(
            &conn,
            "repo",
            "main",
            "validate",
            Some("src/auth/handler.rs"),
            RelatedScope::File,
            20,
        )
        .unwrap();

        assert_eq!(response.related.len(), 1);
        assert_eq!(response.related[0].name, "parse");
        assert_eq!(response.related[0].relation, "same_file");
    }

    #[test]
    fn find_related_symbols_scope_module_includes_imported() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            "a",
            "stable_a",
            "validate",
            "src/auth/handler.rs",
            10,
        );
        insert_symbol(&conn, "b", "stable_b", "parse", "src/auth/handler.rs", 20);
        insert_symbol(&conn, "c", "stable_c", "refresh", "src/auth/token.rs", 30);
        insert_symbol(&conn, "d", "stable_d", "claims", "src/types.rs", 40);

        let edge = SymbolEdge {
            repo: "repo".into(),
            ref_name: "main".into(),
            from_symbol_id: source_symbol_id_for_path("src/auth/handler.rs"),
            to_symbol_id: "stable_d".into(),
            edge_type: "imports".into(),
            confidence: "static".into(),
        };
        edges::insert_edges(&conn, "repo", "main", vec![edge]).unwrap();

        let response = find_related_symbols(
            &conn,
            "repo",
            "main",
            "validate",
            Some("src/auth/handler.rs"),
            RelatedScope::Module,
            20,
        )
        .unwrap();

        assert!(response.related.iter().any(|r| r.relation == "same_module"));
        assert!(response.related.iter().any(|r| r.relation == "imported"));
    }

    #[test]
    fn find_related_symbols_scope_package_includes_same_package() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            "a",
            "stable_a",
            "validate",
            "src/auth/handler.rs",
            10,
        );
        insert_symbol(&conn, "b", "stable_b", "parse", "src/auth/token.rs", 20);
        insert_symbol(
            &conn,
            "c",
            "stable_c",
            "package_helper",
            "src/common/util.rs",
            30,
        );

        let response = find_related_symbols(
            &conn,
            "repo",
            "main",
            "validate",
            Some("src/auth/handler.rs"),
            RelatedScope::Package,
            20,
        )
        .unwrap();

        assert!(
            response
                .related
                .iter()
                .any(|r| r.relation == "same_package"),
            "expected package scope results to include same_package relations"
        );
    }

    #[test]
    fn find_related_symbols_limit_applies_after_ranking() {
        let conn = setup_test_db();
        insert_symbol(
            &conn,
            "a",
            "stable_a",
            "validate",
            "src/auth/handler.rs",
            10,
        );
        insert_symbol(&conn, "b", "stable_b", "parse", "src/auth/handler.rs", 20);
        insert_symbol(&conn, "c", "stable_c", "refresh", "src/auth/token.rs", 30);
        insert_symbol(&conn, "d", "stable_d", "claims", "src/auth/config.rs", 40);

        let response = find_related_symbols(
            &conn,
            "repo",
            "main",
            "validate",
            Some("src/auth/handler.rs"),
            RelatedScope::Module,
            1,
        )
        .unwrap();

        assert_eq!(response.related.len(), 1);
        assert_eq!(response.related[0].relation, "same_file");
    }

    #[test]
    fn find_related_symbols_without_path_returns_ambiguous_when_multiple_files() {
        let conn = setup_test_db();
        insert_symbol(&conn, "a", "stable_a", "validate", "src/a.rs", 10);
        insert_symbol(
            &conn,
            "a_sibling",
            "stable_a_sibling",
            "helper",
            "src/a.rs",
            20,
        );
        insert_symbol(&conn, "b", "stable_b", "validate", "src/z.rs", 10);
        insert_symbol(
            &conn,
            "b_sibling",
            "stable_b_sibling",
            "helper_b",
            "src/z.rs",
            20,
        );

        let err = find_related_symbols(
            &conn,
            "repo",
            "main",
            "validate",
            None,
            RelatedScope::File,
            10,
        )
        .unwrap_err();

        assert!(
            matches!(err, RelatedError::AmbiguousSymbol { count: 2 }),
            "expected AmbiguousSymbol, got: {err:?}"
        );
    }
}
