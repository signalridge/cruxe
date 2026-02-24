use codecompass_core::error::StateError;
use codecompass_core::types::SymbolEdge;
use rusqlite::{Connection, params};
use std::sync::atomic::{AtomicU64, Ordering};

static SAVEPOINT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Insert or replace symbol edges for a repo/ref scope.
pub fn insert_edges(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edges: Vec<SymbolEdge>,
) -> Result<(), StateError> {
    let mut stmt = conn
        .prepare(
            "INSERT OR REPLACE INTO symbol_edges
             (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(StateError::sqlite)?;

    for edge in edges {
        let confidence = if edge.confidence.is_empty() {
            "static"
        } else {
            edge.confidence.as_str()
        };
        stmt.execute(params![
            repo,
            ref_name,
            edge.from_symbol_id,
            edge.to_symbol_id,
            edge.edge_type,
            confidence
        ])
        .map_err(StateError::sqlite)?;
    }
    Ok(())
}

/// Delete all edges originating from symbols that belong to a file.
pub fn delete_edges_for_file(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    from_symbol_ids: Vec<&str>,
) -> Result<(), StateError> {
    let mut stmt = conn
        .prepare(
            "DELETE FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND from_symbol_id = ?3",
        )
        .map_err(StateError::sqlite)?;

    for from_symbol_id in from_symbol_ids {
        stmt.execute(params![repo, ref_name, from_symbol_id])
            .map_err(StateError::sqlite)?;
    }
    Ok(())
}

/// Atomically replace edges for a file: delete old edges then insert new ones in a transaction.
pub fn replace_edges_for_file(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    from_symbol_ids: Vec<&str>,
    new_edges: Vec<SymbolEdge>,
) -> Result<(), StateError> {
    let savepoint = format!(
        "codecompass_edges_replace_{}",
        SAVEPOINT_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    conn.execute_batch(&format!("SAVEPOINT {savepoint}"))
        .map_err(StateError::sqlite)?;

    let result = (|| {
        let mut del_stmt = conn
            .prepare(
                "DELETE FROM symbol_edges
                 WHERE repo = ?1 AND \"ref\" = ?2 AND from_symbol_id = ?3",
            )
            .map_err(StateError::sqlite)?;
        for from_id in from_symbol_ids {
            del_stmt
                .execute(params![repo, ref_name, from_id])
                .map_err(StateError::sqlite)?;
        }

        let mut ins_stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO symbol_edges
                 (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(StateError::sqlite)?;
        for edge in new_edges {
            let confidence = if edge.confidence.is_empty() {
                "static"
            } else {
                edge.confidence.as_str()
            };
            ins_stmt
                .execute(params![
                    repo,
                    ref_name,
                    edge.from_symbol_id,
                    edge.to_symbol_id,
                    edge.edge_type,
                    confidence
                ])
                .map_err(StateError::sqlite)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch(&format!("RELEASE {savepoint}"))
                .map_err(StateError::sqlite)?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute_batch(&format!("ROLLBACK TO {savepoint}; RELEASE {savepoint}"));
            Err(err)
        }
    }
}

/// Get all outgoing edges from a specific symbol.
pub fn get_edges_from(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    from_symbol_id: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND from_symbol_id = ?3
             ORDER BY to_symbol_id, edge_type",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, from_symbol_id], |row| {
            Ok(SymbolEdge {
                repo: row.get(0)?,
                ref_name: row.get(1)?,
                from_symbol_id: row.get(2)?,
                to_symbol_id: row.get(3)?,
                edge_type: row.get(4)?,
                confidence: row.get(5)?,
            })
        })
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

/// Get all incoming edges to a specific symbol.
pub fn get_edges_to(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    to_symbol_id: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND to_symbol_id = ?3
             ORDER BY from_symbol_id, edge_type",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, to_symbol_id], |row| {
            Ok(SymbolEdge {
                repo: row.get(0)?,
                ref_name: row.get(1)?,
                from_symbol_id: row.get(2)?,
                to_symbol_id: row.get(3)?,
                edge_type: row.get(4)?,
                confidence: row.get(5)?,
            })
        })
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

/// Get all edges of a given edge type within a repo/ref.
pub fn get_edges_by_type(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edge_type: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND edge_type = ?3
             ORDER BY from_symbol_id, to_symbol_id",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, edge_type], |row| {
            Ok(SymbolEdge {
                repo: row.get(0)?,
                ref_name: row.get(1)?,
                from_symbol_id: row.get(2)?,
                to_symbol_id: row.get(3)?,
                edge_type: row.get(4)?,
                confidence: row.get(5)?,
            })
        })
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::schema;
    use std::collections::HashSet;
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn edge(from_symbol_id: &str, to_symbol_id: &str, edge_type: &str) -> SymbolEdge {
        SymbolEdge {
            repo: "my-repo".to_string(),
            ref_name: "main".to_string(),
            from_symbol_id: from_symbol_id.to_string(),
            to_symbol_id: to_symbol_id.to_string(),
            edge_type: edge_type.to_string(),
            confidence: "static".to_string(),
        }
    }

    #[test]
    fn test_insert_and_get_edges_from() {
        let conn = setup_test_db();
        let edges = vec![
            edge("file1::module", "auth::claims", "imports"),
            edge("file1::module", "auth::validate", "imports"),
            edge("file2::module", "auth::claims", "imports"),
        ];

        insert_edges(&conn, "my-repo", "main", edges).unwrap();

        let got = get_edges_from(&conn, "my-repo", "main", "file1::module").unwrap();
        let got_pairs: HashSet<(String, String)> = got
            .iter()
            .map(|e| (e.from_symbol_id.clone(), e.to_symbol_id.clone()))
            .collect();
        let expected: HashSet<(String, String)> = vec![
            ("file1::module".to_string(), "auth::claims".to_string()),
            ("file1::module".to_string(), "auth::validate".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(got_pairs, expected);
    }

    #[test]
    fn test_get_edges_to_and_by_type() {
        let conn = setup_test_db();
        let edges = vec![
            edge("file1::module", "auth::claims", "imports"),
            edge("file2::module", "auth::claims", "imports"),
            edge("file2::module", "auth::claims", "calls"),
        ];

        insert_edges(&conn, "my-repo", "main", edges).unwrap();

        let incoming = get_edges_to(&conn, "my-repo", "main", "auth::claims").unwrap();
        assert_eq!(incoming.len(), 3);

        let imports = get_edges_by_type(&conn, "my-repo", "main", "imports").unwrap();
        assert_eq!(imports.len(), 2);
        assert!(imports.iter().all(|e| e.edge_type == "imports"));
    }

    #[test]
    fn test_delete_edges_for_file() {
        let conn = setup_test_db();
        let edges = vec![
            edge("file1::module", "auth::claims", "imports"),
            edge("file1::module", "auth::validate", "imports"),
            edge("file2::module", "auth::claims", "imports"),
        ];

        insert_edges(&conn, "my-repo", "main", edges).unwrap();

        delete_edges_for_file(&conn, "my-repo", "main", vec!["file1::module"]).unwrap();

        let file1 = get_edges_from(&conn, "my-repo", "main", "file1::module").unwrap();
        let file2 = get_edges_from(&conn, "my-repo", "main", "file2::module").unwrap();
        assert!(file1.is_empty());
        assert_eq!(file2.len(), 1);
    }

    #[test]
    fn test_atomic_replacement_pattern() {
        let conn = setup_test_db();
        let original = vec![
            edge("file1::module", "auth::claims", "imports"),
            edge("file1::module", "auth::validate", "imports"),
            edge("file2::module", "auth::claims", "imports"),
        ];
        insert_edges(&conn, "my-repo", "main", original).unwrap();

        replace_edges_for_file(
            &conn,
            "my-repo",
            "main",
            vec!["file1::module"],
            vec![edge("file1::module", "auth::refresh", "imports")],
        )
        .unwrap();

        let file1 = get_edges_from(&conn, "my-repo", "main", "file1::module").unwrap();
        let file2 = get_edges_from(&conn, "my-repo", "main", "file2::module").unwrap();
        assert_eq!(file1.len(), 1);
        assert_eq!(file1[0].to_symbol_id, "auth::refresh");
        assert_eq!(file2.len(), 1);
        assert_eq!(file2[0].to_symbol_id, "auth::claims");
    }

    #[test]
    fn test_replace_edges_for_file_inside_outer_transaction() {
        let conn = setup_test_db();
        insert_edges(
            &conn,
            "my-repo",
            "main",
            vec![edge("file1::module", "auth::claims", "imports")],
        )
        .unwrap();

        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        replace_edges_for_file(
            &conn,
            "my-repo",
            "main",
            vec!["file1::module"],
            vec![edge("file1::module", "auth::refresh", "imports")],
        )
        .unwrap();
        conn.execute_batch("COMMIT").unwrap();

        let file1 = get_edges_from(&conn, "my-repo", "main", "file1::module").unwrap();
        assert_eq!(file1.len(), 1);
        assert_eq!(file1[0].to_symbol_id, "auth::refresh");
    }
}
