use cruxe_core::edge_confidence::assign_edge_confidence;
use cruxe_core::error::StateError;
use cruxe_core::types::{CallEdge, SymbolEdge};
use rusqlite::{Connection, Statement, ToSql, params, params_from_iter};
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
             (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence, edge_provider, resolution_outcome, confidence_weight)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .map_err(StateError::sqlite)?;

    for edge in edges {
        let assignment = assign_edge_confidence(
            None,
            Some(edge.edge_type.as_str()),
            None,
            Some(edge.to_symbol_id.as_str()),
            None,
            Some(edge.confidence.as_str()),
        );
        stmt.execute(params![
            repo,
            ref_name,
            edge.from_symbol_id,
            edge.to_symbol_id,
            edge.edge_type,
            assignment.bucket,
            assignment.provider,
            assignment.outcome,
            assignment.weight
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
        "cruxe_edges_replace_{}",
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
                 (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence, edge_provider, resolution_outcome, confidence_weight)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(StateError::sqlite)?;
        for edge in new_edges {
            let assignment = assign_edge_confidence(
                None,
                Some(edge.edge_type.as_str()),
                None,
                Some(edge.to_symbol_id.as_str()),
                None,
                Some(edge.confidence.as_str()),
            );
            ins_stmt
                .execute(params![
                    repo,
                    ref_name,
                    edge.from_symbol_id,
                    edge.to_symbol_id,
                    edge.edge_type,
                    assignment.bucket,
                    assignment.provider,
                    assignment.outcome,
                    assignment.weight
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
            "SELECT repo, \"ref\", from_symbol_id, COALESCE(to_symbol_id, to_name, '') AS to_symbol_id, edge_type, confidence
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
            "SELECT repo, \"ref\", from_symbol_id, COALESCE(to_symbol_id, to_name, '') AS to_symbol_id, edge_type, confidence
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

/// Get outgoing edges from a symbol constrained to one edge type.
pub fn get_edges_from_by_type(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    from_symbol_id: &str,
    edge_type: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, COALESCE(to_symbol_id, to_name, '') AS to_symbol_id, edge_type, confidence
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND from_symbol_id = ?3 AND edge_type = ?4
             ORDER BY to_symbol_id",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, from_symbol_id, edge_type], |row| {
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

/// Get incoming edges to a symbol constrained to one edge type.
pub fn get_edges_to_by_type(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    to_symbol_id: &str,
    edge_type: &str,
) -> Result<Vec<SymbolEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, COALESCE(to_symbol_id, to_name, '') AS to_symbol_id, edge_type, confidence
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND to_symbol_id = ?3 AND edge_type = ?4
             ORDER BY from_symbol_id",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, to_symbol_id, edge_type], |row| {
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
            "SELECT repo, \"ref\", from_symbol_id, COALESCE(to_symbol_id, to_name, '') AS to_symbol_id, edge_type, confidence
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

/// Insert extracted call edges for a repo/ref scope.
pub fn insert_call_edges(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edges: &[CallEdge],
) -> Result<(), StateError> {
    conn.execute_batch("SAVEPOINT cc_insert_call_edges")
        .map_err(StateError::sqlite)?;
    let result = (|| {
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO symbol_edges
                 (repo, \"ref\", from_symbol_id, to_symbol_id, to_name, edge_type, confidence, edge_provider, resolution_outcome, confidence_weight, source_file, source_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(StateError::sqlite)?;
        for edge in edges {
            execute_insert_call_edge(&mut stmt, repo, ref_name, edge)?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("RELEASE SAVEPOINT cc_insert_call_edges")
                .map_err(StateError::sqlite)?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO SAVEPOINT cc_insert_call_edges; RELEASE SAVEPOINT cc_insert_call_edges",
            );
            Err(err)
        }
    }
}

/// Replace call edges for multiple files atomically in one savepoint.
///
/// Existing call edges from each `source_file` in `edges_by_file` are removed
/// and then replaced with the provided edges.
pub fn replace_call_edges_for_files(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edges_by_file: &[(String, Vec<CallEdge>)],
) -> Result<(), StateError> {
    if edges_by_file.is_empty() {
        return Ok(());
    }

    conn.execute_batch("SAVEPOINT cc_replace_call_edges_for_files")
        .map_err(StateError::sqlite)?;
    let result = (|| {
        let mut delete_stmt = conn
            .prepare(
                "DELETE FROM symbol_edges
                 WHERE repo = ?1 AND \"ref\" = ?2 AND edge_type = 'calls' AND source_file = ?3",
            )
            .map_err(StateError::sqlite)?;
        for (source_file, _) in edges_by_file {
            delete_stmt
                .execute(params![repo, ref_name, source_file])
                .map_err(StateError::sqlite)?;
        }
        drop(delete_stmt);

        let mut insert_stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO symbol_edges
                 (repo, \"ref\", from_symbol_id, to_symbol_id, to_name, edge_type, confidence, edge_provider, resolution_outcome, confidence_weight, source_file, source_line)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .map_err(StateError::sqlite)?;

        for (_, edges) in edges_by_file {
            for edge in edges {
                execute_insert_call_edge(&mut insert_stmt, repo, ref_name, edge)?;
            }
        }

        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("RELEASE SAVEPOINT cc_replace_call_edges_for_files")
                .map_err(StateError::sqlite)?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO SAVEPOINT cc_replace_call_edges_for_files; RELEASE SAVEPOINT cc_replace_call_edges_for_files",
            );
            Err(err)
        }
    }
}

fn execute_insert_call_edge(
    stmt: &mut Statement<'_>,
    repo: &str,
    ref_name: &str,
    edge: &CallEdge,
) -> Result<(), StateError> {
    let edge_type = if edge.edge_type.trim().is_empty() {
        "calls"
    } else {
        edge.edge_type.as_str()
    };
    let assignment = assign_edge_confidence(
        None,
        Some(edge_type),
        None,
        edge.to_symbol_id.as_deref(),
        edge.to_name.as_deref(),
        Some(edge.confidence.as_str()),
    );
    stmt.execute(params![
        repo,
        ref_name,
        edge.from_symbol_id,
        edge.to_symbol_id,
        edge.to_name,
        edge_type,
        assignment.bucket,
        assignment.provider,
        assignment.outcome,
        assignment.weight,
        edge.source_file,
        i64::from(edge.source_line),
    ])
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Delete call edges produced from a specific source file.
pub fn delete_call_edges_for_file(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    source_file: &str,
) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM symbol_edges
         WHERE repo = ?1 AND \"ref\" = ?2 AND edge_type = 'calls' AND source_file = ?3",
        params![repo, ref_name, source_file],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Delete call edges that target any of the provided symbol stable IDs.
pub fn delete_call_edges_to_symbols(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    to_symbol_ids: &[String],
) -> Result<(), StateError> {
    if to_symbol_ids.is_empty() {
        return Ok(());
    }

    // SQLite default max bind parameters is 999. We reserve 2 for repo/ref and
    // batch the variable IN-list to stay below that hard limit.
    const MAX_SQLITE_BIND_PARAMS: usize = 999;
    const STATIC_BIND_PARAMS: usize = 2;
    const MAX_TO_SYMBOL_IDS_PER_BATCH: usize = MAX_SQLITE_BIND_PARAMS - STATIC_BIND_PARAMS;

    for id_batch in to_symbol_ids.chunks(MAX_TO_SYMBOL_IDS_PER_BATCH) {
        let placeholders = std::iter::repeat_n("?", id_batch.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM symbol_edges
             WHERE repo = ? AND \"ref\" = ? AND edge_type = 'calls' AND to_symbol_id IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql).map_err(StateError::sqlite)?;

        let mut bind_params: Vec<&dyn ToSql> = Vec::with_capacity(2 + id_batch.len());
        bind_params.push(&repo);
        bind_params.push(&ref_name);
        for to_symbol_id in id_batch {
            bind_params.push(to_symbol_id);
        }
        stmt.execute(params_from_iter(bind_params))
            .map_err(StateError::sqlite)?;
    }
    Ok(())
}

/// Get all caller call-edges that target a symbol.
pub fn get_callers(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_id: &str,
) -> Result<Vec<CallEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, to_symbol_id, to_name, edge_type, confidence, source_file, source_line
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND edge_type = 'calls' AND to_symbol_id = ?3
             ORDER BY source_file, source_line, from_symbol_id",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, symbol_id], map_call_edge_row)
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

/// Get all callee call-edges originating from a symbol.
pub fn get_callees(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_id: &str,
) -> Result<Vec<CallEdge>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", from_symbol_id, to_symbol_id, to_name, edge_type, confidence, source_file, source_line
             FROM symbol_edges
             WHERE repo = ?1 AND \"ref\" = ?2 AND edge_type = 'calls' AND from_symbol_id = ?3
             ORDER BY source_file, source_line, COALESCE(to_symbol_id, to_name)",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, ref_name, symbol_id], map_call_edge_row)
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

fn map_call_edge_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CallEdge> {
    let source_line = row.get::<_, Option<i64>>(8)?.unwrap_or_default().max(0) as u32;
    Ok(CallEdge {
        repo: row.get(0)?,
        ref_name: row.get(1)?,
        from_symbol_id: row.get(2)?,
        to_symbol_id: row.get(3)?,
        to_name: row.get(4)?,
        edge_type: row.get(5)?,
        confidence: row.get(6)?,
        source_file: row.get(7)?,
        source_line,
    })
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

    fn call_edge(
        from_symbol_id: &str,
        to_symbol_id: Option<&str>,
        to_name: Option<&str>,
        source_file: &str,
        source_line: u32,
        confidence: &str,
    ) -> CallEdge {
        CallEdge {
            repo: "my-repo".to_string(),
            ref_name: "main".to_string(),
            from_symbol_id: from_symbol_id.to_string(),
            to_symbol_id: to_symbol_id.map(str::to_string),
            to_name: to_name.map(str::to_string),
            edge_type: "calls".to_string(),
            confidence: confidence.to_string(),
            source_file: source_file.to_string(),
            source_line,
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

        let outgoing_imports =
            get_edges_from_by_type(&conn, "my-repo", "main", "file2::module", "imports").unwrap();
        assert_eq!(outgoing_imports.len(), 1);
        assert_eq!(outgoing_imports[0].to_symbol_id, "auth::claims");

        let incoming_calls =
            get_edges_to_by_type(&conn, "my-repo", "main", "auth::claims", "calls").unwrap();
        assert_eq!(incoming_calls.len(), 1);
        assert_eq!(incoming_calls[0].from_symbol_id, "file2::module");
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

    #[test]
    fn test_query_plan_uses_symbol_edge_type_indexes() {
        let conn = setup_test_db();
        insert_edges(
            &conn,
            "my-repo",
            "main",
            vec![
                edge("file1::module", "auth::claims", "imports"),
                edge("file2::module", "auth::claims", "calls"),
            ],
        )
        .unwrap();

        let forward_plan = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT 1 FROM symbol_edges
                 WHERE repo = ?1 AND \"ref\" = ?2 AND from_symbol_id = ?3 AND edge_type = ?4",
            )
            .unwrap()
            .query_map(
                params!["my-repo", "main", "file1::module", "imports"],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            forward_plan
                .iter()
                .any(|line| line.contains("idx_symbol_edges_from_type")),
            "forward edge query should use idx_symbol_edges_from_type: {forward_plan:?}"
        );

        let reverse_plan = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT 1 FROM symbol_edges
                 WHERE repo = ?1 AND \"ref\" = ?2 AND to_symbol_id = ?3 AND edge_type = ?4",
            )
            .unwrap()
            .query_map(params!["my-repo", "main", "auth::claims", "calls"], |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            reverse_plan
                .iter()
                .any(|line| line.contains("idx_symbol_edges_to_type")),
            "reverse edge query should use idx_symbol_edges_to_type: {reverse_plan:?}"
        );
    }

    #[test]
    fn test_insert_and_query_call_edges() {
        let conn = setup_test_db();
        let calls = vec![
            call_edge(
                "sym::handler",
                Some("sym::validate"),
                None,
                "src/handler.rs",
                12,
                "static",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("external::audit"),
                "src/handler.rs",
                13,
                "heuristic",
            ),
            call_edge(
                "sym::entry",
                Some("sym::validate"),
                None,
                "src/entry.rs",
                7,
                "static",
            ),
        ];
        insert_call_edges(&conn, "my-repo", "main", &calls).unwrap();

        let callees = get_callees(&conn, "my-repo", "main", "sym::handler").unwrap();
        assert_eq!(callees.len(), 2);
        assert_eq!(callees[0].to_symbol_id.as_deref(), Some("sym::validate"));
        assert_eq!(callees[0].to_name.as_deref(), None);
        assert_eq!(callees[1].to_symbol_id, None);
        assert_eq!(callees[1].to_name.as_deref(), Some("external::audit"));
        assert_eq!(callees[1].confidence, "low");

        let callers = get_callers(&conn, "my-repo", "main", "sym::validate").unwrap();
        assert_eq!(callers.len(), 2);
        assert!(
            callers
                .iter()
                .any(|edge| edge.from_symbol_id == "sym::handler")
        );
        assert!(
            callers
                .iter()
                .any(|edge| edge.from_symbol_id == "sym::entry")
        );
    }

    #[test]
    fn test_call_edge_confidence_assignment_persists_resolved_external_and_unresolved_outcomes() {
        let conn = setup_test_db();
        let calls = vec![
            call_edge(
                "sym::handler",
                Some("sym::validate"),
                None,
                "src/handler.rs",
                12,
                "static",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("external::audit"),
                "src/handler.rs",
                13,
                "",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("fallback"),
                "src/handler.rs",
                14,
                "heuristic",
            ),
        ];
        insert_call_edges(&conn, "my-repo", "main", &calls).unwrap();

        let persisted: Vec<(String, String, String, f64)> = conn
            .prepare(
                "SELECT confidence, edge_provider, resolution_outcome, confidence_weight
                 FROM symbol_edges
                 WHERE edge_type = 'calls'
                 ORDER BY source_line",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            persisted[0],
            (
                "high".to_string(),
                "call_resolver".to_string(),
                "resolved_internal".to_string(),
                1.0
            )
        );
        assert_eq!(
            persisted[1],
            (
                "medium".to_string(),
                "call_resolver".to_string(),
                "external_reference".to_string(),
                0.6
            )
        );
        assert_eq!(
            persisted[2],
            (
                "low".to_string(),
                "call_resolver".to_string(),
                "unresolved".to_string(),
                0.2
            )
        );
    }

    #[test]
    fn test_delete_call_edges_for_file() {
        let conn = setup_test_db();
        let calls = vec![
            call_edge(
                "sym::handler",
                Some("sym::validate"),
                None,
                "src/handler.rs",
                12,
                "static",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("external::audit"),
                "src/handler.rs",
                13,
                "heuristic",
            ),
            call_edge(
                "sym::entry",
                Some("sym::validate"),
                None,
                "src/entry.rs",
                7,
                "static",
            ),
        ];
        insert_call_edges(&conn, "my-repo", "main", &calls).unwrap();

        delete_call_edges_for_file(&conn, "my-repo", "main", "src/handler.rs").unwrap();

        let handler_callees = get_callees(&conn, "my-repo", "main", "sym::handler").unwrap();
        assert!(handler_callees.is_empty());

        let entry_callees = get_callees(&conn, "my-repo", "main", "sym::entry").unwrap();
        assert_eq!(entry_callees.len(), 1);
        assert_eq!(entry_callees[0].source_file, "src/entry.rs");
    }

    #[test]
    fn test_delete_call_edges_to_symbols() {
        let conn = setup_test_db();
        let calls = vec![
            call_edge(
                "sym::handler",
                Some("sym::validate"),
                None,
                "src/handler.rs",
                12,
                "static",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("external::audit"),
                "src/handler.rs",
                13,
                "heuristic",
            ),
            call_edge(
                "sym::entry",
                Some("sym::validate"),
                None,
                "src/entry.rs",
                7,
                "static",
            ),
        ];
        insert_call_edges(&conn, "my-repo", "main", &calls).unwrap();

        delete_call_edges_to_symbols(&conn, "my-repo", "main", &["sym::validate".to_string()])
            .unwrap();

        let callers = get_callers(&conn, "my-repo", "main", "sym::validate").unwrap();
        assert!(callers.is_empty());

        // Unresolved external edges must remain untouched.
        let handler_callees = get_callees(&conn, "my-repo", "main", "sym::handler").unwrap();
        assert_eq!(handler_callees.len(), 1);
        assert_eq!(handler_callees[0].to_symbol_id, None);
        assert_eq!(
            handler_callees[0].to_name.as_deref(),
            Some("external::audit")
        );
    }

    #[test]
    fn test_delete_call_edges_to_symbols_batches_large_inputs() {
        let conn = setup_test_db();
        let mut calls = Vec::new();
        let mut to_symbol_ids = Vec::new();

        for idx in 0..1100 {
            let target_id = format!("sym::target_{idx}");
            to_symbol_ids.push(target_id.clone());
            calls.push(call_edge(
                "sym::handler",
                Some(target_id.as_str()),
                None,
                "src/handler.rs",
                (idx + 1) as u32,
                "static",
            ));
        }
        insert_call_edges(&conn, "my-repo", "main", &calls).unwrap();

        delete_call_edges_to_symbols(&conn, "my-repo", "main", &to_symbol_ids).unwrap();

        let remaining = get_callees(&conn, "my-repo", "main", "sym::handler").unwrap();
        assert!(
            remaining.is_empty(),
            "batched delete should remove all resolved edges"
        );
    }

    #[test]
    fn test_replace_call_edges_for_files_replaces_each_source_file_atomically() {
        let conn = setup_test_db();
        let initial = vec![
            call_edge(
                "sym::handler",
                Some("sym::validate"),
                None,
                "src/handler.rs",
                12,
                "static",
            ),
            call_edge(
                "sym::handler",
                None,
                Some("external::audit"),
                "src/handler.rs",
                13,
                "heuristic",
            ),
            call_edge(
                "sym::entry",
                Some("sym::validate"),
                None,
                "src/entry.rs",
                7,
                "static",
            ),
        ];
        insert_call_edges(&conn, "my-repo", "main", &initial).unwrap();

        let replacements = vec![
            (
                "src/handler.rs".to_string(),
                vec![call_edge(
                    "sym::handler",
                    Some("sym::audit"),
                    None,
                    "src/handler.rs",
                    20,
                    "static",
                )],
            ),
            (
                "src/entry.rs".to_string(),
                vec![call_edge(
                    "sym::entry",
                    None,
                    Some("external::sink"),
                    "src/entry.rs",
                    9,
                    "heuristic",
                )],
            ),
        ];

        replace_call_edges_for_files(&conn, "my-repo", "main", &replacements).unwrap();

        let handler_callees = get_callees(&conn, "my-repo", "main", "sym::handler").unwrap();
        assert_eq!(handler_callees.len(), 1);
        assert_eq!(
            handler_callees[0].to_symbol_id.as_deref(),
            Some("sym::audit")
        );
        assert_eq!(handler_callees[0].source_line, 20);

        let entry_callees = get_callees(&conn, "my-repo", "main", "sym::entry").unwrap();
        assert_eq!(entry_callees.len(), 1);
        assert_eq!(entry_callees[0].to_symbol_id, None);
        assert_eq!(entry_callees[0].to_name.as_deref(), Some("external::sink"));
        assert_eq!(entry_callees[0].source_line, 9);
    }
}
