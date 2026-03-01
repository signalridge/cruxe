use cruxe_core::error::StateError;
use rusqlite::{Connection, params};
use std::collections::HashMap;

/// Compute max-normalized file centrality from resolved inter-file symbol edges.
///
/// Centrality is defined as:
/// `distinct inbound source files to symbols in path / max inbound across files`.
pub fn compute_file_centrality(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
) -> Result<HashMap<String, f64>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT sr.path AS target_file,
                    COUNT(DISTINCT se.source_file) AS inbound_file_count
             FROM symbol_edges se
             JOIN symbol_relations sr
               ON sr.repo = se.repo
              AND sr.\"ref\" = se.\"ref\"
              AND sr.symbol_stable_id = se.to_symbol_id
             WHERE se.repo = ?1
               AND se.\"ref\" = ?2
               AND se.to_symbol_id IS NOT NULL
               AND COALESCE(se.source_file, '') <> ''
               AND se.source_file != sr.path
             GROUP BY sr.path",
        )
        .map_err(StateError::sqlite)?;

    let mut counts: HashMap<String, f64> = HashMap::new();
    let mut max_inbound = 0.0_f64;
    let rows = stmt
        .query_map(params![repo, ref_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(StateError::sqlite)?;
    for row in rows {
        let (path, inbound) = row.map_err(StateError::sqlite)?;
        let inbound = (inbound.max(0)) as f64;
        max_inbound = max_inbound.max(inbound);
        counts.insert(path, inbound);
    }
    if max_inbound <= f64::EPSILON {
        return Ok(HashMap::new());
    }

    let normalized = counts
        .into_iter()
        .map(|(path, inbound)| (path, (inbound / max_inbound).clamp(0.0, 1.0)))
        .collect();
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema, symbols};
    use tempfile::TempDir;

    struct TestDb {
        _dir: TempDir,
        conn: Connection,
    }

    fn setup_db() -> TestDb {
        let dir = tempfile::tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        TestDb { _dir: dir, conn }
    }

    fn insert_symbol(conn: &Connection, path: &str, stable_id: &str) {
        symbols::insert_symbol(
            conn,
            &SymbolRecord {
                repo: "repo".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: path.to_string(),
                language: "rust".to_string(),
                symbol_id: format!("sym-{stable_id}"),
                symbol_stable_id: stable_id.to_string(),
                name: stable_id.to_string(),
                qualified_name: stable_id.to_string(),
                kind: SymbolKind::Function,
                signature: Some(format!("fn {stable_id}()")),
                line_start: 1,
                line_end: 2,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some("fn demo() {}".to_string()),
            },
        )
        .unwrap();
    }

    fn insert_edge(conn: &Connection, from_symbol_id: &str, to_symbol_id: &str, source_file: &str) {
        conn.execute(
            "INSERT INTO symbol_edges
             (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence, edge_provider, resolution_outcome, confidence_weight, source_file, source_line)
             VALUES ('repo', 'main', ?1, ?2, 'calls', 'high', 'call_resolver', 'resolved_internal', 1.0, ?3, 1)",
            params![from_symbol_id, to_symbol_id, source_file],
        )
        .unwrap();
    }

    #[test]
    fn centrality_returns_empty_map_for_empty_graph() {
        let db = setup_db();
        let map = compute_file_centrality(&db.conn, "repo", "main").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn centrality_excludes_self_edges() {
        let db = setup_db();
        insert_symbol(&db.conn, "src/a.rs", "stable-a");
        insert_edge(&db.conn, "from-self", "stable-a", "src/a.rs");
        insert_edge(&db.conn, "from-b", "stable-a", "src/b.rs");

        let map = compute_file_centrality(&db.conn, "repo", "main").unwrap();
        assert_eq!(map.get("src/a.rs").copied(), Some(1.0));
    }

    #[test]
    fn centrality_is_max_normalized() {
        let db = setup_db();
        insert_symbol(&db.conn, "src/a.rs", "stable-a");
        insert_symbol(&db.conn, "src/b.rs", "stable-b");

        insert_edge(&db.conn, "from-b", "stable-a", "src/b.rs");
        insert_edge(&db.conn, "from-c", "stable-a", "src/c.rs");
        insert_edge(&db.conn, "from-a", "stable-b", "src/a.rs");

        let map = compute_file_centrality(&db.conn, "repo", "main").unwrap();
        assert_eq!(map.get("src/a.rs").copied(), Some(1.0));
        assert_eq!(map.get("src/b.rs").copied(), Some(0.5));
    }
}
