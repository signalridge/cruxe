use cruxe_core::error::StateError;
use cruxe_core::types::{CallEdge, SymbolKind, SymbolRecord};
use cruxe_state::{edges, symbols};
use rusqlite::{Connection, ToSql, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

pub const MAX_CALL_GRAPH_DEPTH: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallGraphDirection {
    Callers,
    Callees,
    Both,
}

impl CallGraphDirection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "callers" => Some(Self::Callers),
            "callees" => Some(Self::Callees),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CallGraphError {
    #[error("symbol not found")]
    SymbolNotFound,
    #[error(transparent)]
    State(#[from] StateError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallGraphSymbol {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub name: String,
    pub qualified_name: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallSite {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallGraphEdgeResult {
    pub symbol: CallGraphSymbol,
    pub call_site: CallSite,
    pub confidence: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphResult {
    pub symbol: CallGraphSymbol,
    pub callers: Vec<CallGraphEdgeResult>,
    pub callees: Vec<CallGraphEdgeResult>,
    pub total_edges: usize,
    pub truncated: bool,
    pub depth_applied: u32,
}

#[derive(Debug, Clone)]
pub struct CallGraphRequest<'a> {
    pub symbol_name: &'a str,
    pub path: Option<&'a str>,
    pub direction: CallGraphDirection,
    pub depth: u32,
    pub limit: usize,
}

pub fn clamp_depth(depth: u32) -> u32 {
    depth.clamp(1, MAX_CALL_GRAPH_DEPTH)
}

pub fn get_call_graph(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    request: &CallGraphRequest<'_>,
) -> Result<CallGraphResult, CallGraphError> {
    let root = resolve_root_symbol(conn, repo, ref_name, request.symbol_name, request.path)?
        .ok_or(CallGraphError::SymbolNotFound)?;
    let root_symbol = to_call_graph_symbol(&root);
    let depth_applied = clamp_depth(request.depth);
    let limit = request.limit.max(1);

    let (callers, callers_truncated) = match request.direction {
        CallGraphDirection::Callers | CallGraphDirection::Both => traverse_direction(
            conn,
            repo,
            ref_name,
            &root.symbol_stable_id,
            depth_applied,
            limit,
            TraversalMode::Callers,
        )?,
        CallGraphDirection::Callees => (Vec::new(), false),
    };

    let (callees, callees_truncated) = match request.direction {
        CallGraphDirection::Callees | CallGraphDirection::Both => traverse_direction(
            conn,
            repo,
            ref_name,
            &root.symbol_stable_id,
            depth_applied,
            limit,
            TraversalMode::Callees,
        )?,
        CallGraphDirection::Callers => (Vec::new(), false),
    };

    let total_edges = callers.len() + callees.len();
    Ok(CallGraphResult {
        symbol: root_symbol,
        callers,
        callees,
        total_edges,
        truncated: callers_truncated || callees_truncated,
        depth_applied,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraversalMode {
    Callers,
    Callees,
}

fn traverse_direction(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    root_symbol_stable_id: &str,
    depth_limit: u32,
    limit: usize,
    mode: TraversalMode,
) -> Result<(Vec<CallGraphEdgeResult>, bool), StateError> {
    let mut queue = VecDeque::from([(root_symbol_stable_id.to_string(), 0u32)]);
    let mut expanded = HashSet::from([root_symbol_stable_id.to_string()]);
    let mut emitted = HashSet::<(String, String, u32, u32)>::new();
    let mut results = Vec::new();
    let mut truncated = false;

    while let Some((current_symbol_id, current_depth)) = queue.pop_front() {
        if current_depth >= depth_limit {
            continue;
        }

        let edges_for_symbol = match mode {
            TraversalMode::Callers => {
                edges::get_callers(conn, repo, ref_name, current_symbol_id.as_str())?
            }
            TraversalMode::Callees => {
                edges::get_callees(conn, repo, ref_name, current_symbol_id.as_str())?
            }
        };
        let resolved_targets =
            resolve_target_symbols_batch(conn, repo, ref_name, &edges_for_symbol, mode)?;

        for edge in edges_for_symbol {
            let Some(target_lookup_id) = target_id_for_edge(&edge, mode) else {
                continue;
            };
            let Some((target_id, target_symbol)) = resolved_targets.get(target_lookup_id) else {
                continue;
            };
            let target_id = target_id.clone();
            let target_symbol = target_symbol.clone();

            let edge_depth = current_depth + 1;
            let dedup_key = (
                target_symbol.symbol_stable_id.clone(),
                edge.source_file.clone(),
                edge.source_line,
                edge_depth,
            );
            if !emitted.insert(dedup_key) {
                continue;
            }

            if results.len() >= limit {
                truncated = true;
                break;
            }

            results.push(CallGraphEdgeResult {
                symbol: target_symbol.clone(),
                call_site: CallSite {
                    file: edge.source_file,
                    line: edge.source_line,
                },
                confidence: edge.confidence,
                depth: edge_depth,
            });

            if expanded.insert(target_id.clone()) {
                queue.push_back((target_id, edge_depth));
            }
        }

        if truncated {
            break;
        }
    }

    Ok((results, truncated))
}

fn target_id_for_edge(edge: &CallEdge, mode: TraversalMode) -> Option<&str> {
    match mode {
        TraversalMode::Callers => Some(edge.from_symbol_id.as_str()),
        TraversalMode::Callees => edge.to_symbol_id.as_deref(),
    }
}

fn resolve_target_symbols_batch(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    edges: &[CallEdge],
    mode: TraversalMode,
) -> Result<HashMap<String, (String, CallGraphSymbol)>, StateError> {
    let mut deduped_ids = Vec::new();
    let mut seen = HashSet::new();
    for edge in edges {
        let Some(target_id) = target_id_for_edge(edge, mode) else {
            continue;
        };
        if seen.insert(target_id.to_string()) {
            deduped_ids.push(target_id.to_string());
        }
    }
    if deduped_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut resolved = HashMap::new();
    for record in query_symbols_by_field(conn, repo, ref_name, "symbol_stable_id", &deduped_ids)? {
        let key = record.symbol_stable_id.clone();
        resolved.entry(key).or_insert_with(|| {
            let symbol = to_call_graph_symbol(&record);
            (symbol.symbol_stable_id.clone(), symbol)
        });
    }

    let unresolved: Vec<String> = deduped_ids
        .into_iter()
        .filter(|target_id| !resolved.contains_key(target_id))
        .collect();
    if unresolved.is_empty() {
        return Ok(resolved);
    }

    for record in query_symbols_by_field(conn, repo, ref_name, "symbol_id", &unresolved)? {
        let key = record.symbol_id.clone();
        resolved.entry(key).or_insert_with(|| {
            let symbol = to_call_graph_symbol(&record);
            (symbol.symbol_stable_id.clone(), symbol)
        });
    }

    Ok(resolved)
}

fn query_symbols_by_field(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    field: &str,
    target_ids: &[String],
) -> Result<Vec<SymbolRecord>, StateError> {
    if target_ids.is_empty() {
        return Ok(Vec::new());
    }

    let field_name = match field {
        "symbol_stable_id" => "symbol_stable_id",
        "symbol_id" => "symbol_id",
        _ => return Ok(Vec::new()),
    };
    let mut records = Vec::new();
    for ids_chunk in target_ids.chunks(400) {
        let placeholders = std::iter::repeat_n("?", ids_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT repo, \"ref\", \"commit\", path, symbol_id, symbol_stable_id, name, qualified_name, \
                    kind, language, line_start, line_end, signature, parent_symbol_id, visibility \
             FROM symbol_relations
             WHERE repo = ? AND \"ref\" = ? AND {field_name} IN ({placeholders})
             ORDER BY path, line_start, symbol_stable_id"
        );
        let mut stmt = conn.prepare(&sql).map_err(StateError::sqlite)?;
        let mut bind_params: Vec<&dyn ToSql> = Vec::with_capacity(2 + ids_chunk.len());
        bind_params.push(&repo);
        bind_params.push(&ref_name);
        for target_id in ids_chunk {
            bind_params.push(target_id);
        }

        let rows = stmt
            .query_map(params_from_iter(bind_params), row_to_symbol_record)
            .map_err(StateError::sqlite)?;
        records.extend(
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StateError::sqlite)?,
        );
    }

    Ok(records)
}
fn resolve_root_symbol(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_name: &str,
    path: Option<&str>,
) -> Result<Option<SymbolRecord>, StateError> {
    let mut matches = symbols::find_symbols_by_name(conn, repo, ref_name, symbol_name, path)?;
    if matches.is_empty() {
        matches = find_symbols_by_qualified_name(conn, repo, ref_name, symbol_name, path)?;
    }
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line_start.cmp(&right.line_start))
            .then_with(|| left.qualified_name.cmp(&right.qualified_name))
    });
    Ok(matches.into_iter().next())
}

fn find_symbols_by_qualified_name(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    qualified_name: &str,
    path: Option<&str>,
) -> Result<Vec<SymbolRecord>, StateError> {
    let sql = if path.is_some() {
        "SELECT repo, \"ref\", \"commit\", path, symbol_id, symbol_stable_id, name, qualified_name, \
                kind, language, line_start, line_end, signature, parent_symbol_id, visibility \
         FROM symbol_relations
         WHERE repo = ?1 AND \"ref\" = ?2 AND qualified_name = ?3 AND path = ?4
         ORDER BY line_start"
    } else {
        "SELECT repo, \"ref\", \"commit\", path, symbol_id, symbol_stable_id, name, qualified_name, \
                kind, language, line_start, line_end, signature, parent_symbol_id, visibility \
         FROM symbol_relations
         WHERE repo = ?1 AND \"ref\" = ?2 AND qualified_name = ?3
         ORDER BY path, line_start"
    };
    let mut stmt = conn.prepare(sql).map_err(StateError::sqlite)?;
    let rows = if let Some(file_path) = path {
        stmt.query_map(
            params![repo, ref_name, qualified_name, file_path],
            row_to_symbol_record,
        )
        .map_err(StateError::sqlite)?
    } else {
        stmt.query_map(
            params![repo, ref_name, qualified_name],
            row_to_symbol_record,
        )
        .map_err(StateError::sqlite)?
    };
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

fn row_to_symbol_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRecord> {
    Ok(SymbolRecord {
        repo: row.get(0)?,
        r#ref: row.get(1)?,
        commit: row.get(2)?,
        path: row.get(3)?,
        symbol_id: row.get(4)?,
        symbol_stable_id: row.get(5)?,
        name: row.get(6)?,
        qualified_name: row.get(7)?,
        kind: SymbolKind::parse_kind(&row.get::<_, String>(8)?).unwrap_or(SymbolKind::Function),
        language: row.get(9)?,
        line_start: row.get(10)?,
        line_end: row.get(11)?,
        signature: row.get(12)?,
        parent_symbol_id: row.get(13)?,
        visibility: row.get(14)?,
        content: None,
    })
}

fn to_call_graph_symbol(symbol: &SymbolRecord) -> CallGraphSymbol {
    CallGraphSymbol {
        symbol_id: symbol.symbol_id.clone(),
        symbol_stable_id: symbol.symbol_stable_id.clone(),
        name: symbol.name.clone(),
        qualified_name: symbol.qualified_name.clone(),
        path: symbol.path.clone(),
        line_start: symbol.line_start,
        line_end: symbol.line_end,
        kind: symbol.kind.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::CallEdge;
    use cruxe_state::{db, schema, symbols};

    fn setup() -> Connection {
        let tmp = tempfile::tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn symbol(stable: &str, name: &str, path: &str, line: u32) -> SymbolRecord {
        SymbolRecord {
            repo: "repo".to_string(),
            r#ref: "main".to_string(),
            commit: None,
            path: path.to_string(),
            language: "rust".to_string(),
            symbol_id: format!("sym::{name}"),
            symbol_stable_id: stable.to_string(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind: SymbolKind::Function,
            signature: Some(format!("fn {name}()")),
            line_start: line,
            line_end: line + 2,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        }
    }

    fn call(from: &str, to: Option<&str>, file: &str, line: u32) -> CallEdge {
        CallEdge {
            repo: "repo".to_string(),
            ref_name: "main".to_string(),
            from_symbol_id: from.to_string(),
            to_symbol_id: to.map(ToString::to_string),
            to_name: None,
            edge_type: "calls".to_string(),
            confidence: "static".to_string(),
            source_file: file.to_string(),
            source_line: line,
        }
    }

    #[test]
    fn get_call_graph_returns_transitive_callees() {
        let conn = setup();
        for record in [
            symbol("stable-a", "a", "src/a.rs", 1),
            symbol("stable-b", "b", "src/b.rs", 10),
            symbol("stable-c", "c", "src/c.rs", 20),
        ] {
            symbols::insert_symbol(&conn, &record).unwrap();
        }
        edges::insert_call_edges(
            &conn,
            "repo",
            "main",
            &[
                call("stable-a", Some("stable-b"), "src/a.rs", 2),
                call("stable-b", Some("stable-c"), "src/b.rs", 11),
            ],
        )
        .unwrap();

        let depth1 = get_call_graph(
            &conn,
            "repo",
            "main",
            &CallGraphRequest {
                symbol_name: "a",
                path: None,
                direction: CallGraphDirection::Callees,
                depth: 1,
                limit: 20,
            },
        )
        .unwrap();
        assert_eq!(depth1.callees.len(), 1);
        assert_eq!(depth1.callees[0].symbol.name, "b");
        assert_eq!(depth1.callees[0].depth, 1);

        let depth2 = get_call_graph(
            &conn,
            "repo",
            "main",
            &CallGraphRequest {
                symbol_name: "a",
                path: None,
                direction: CallGraphDirection::Callees,
                depth: 2,
                limit: 20,
            },
        )
        .unwrap();
        assert_eq!(depth2.callees.len(), 2);
        assert_eq!(depth2.callees[0].symbol.name, "b");
        assert_eq!(depth2.callees[1].symbol.name, "c");
        assert_eq!(depth2.callees[1].depth, 2);
    }

    #[test]
    fn depth_cap_and_cycle_detection_prevent_infinite_traversal() {
        let conn = setup();
        for record in [
            symbol("stable-a", "a", "src/a.rs", 1),
            symbol("stable-b", "b", "src/b.rs", 10),
        ] {
            symbols::insert_symbol(&conn, &record).unwrap();
        }
        edges::insert_call_edges(
            &conn,
            "repo",
            "main",
            &[
                call("stable-a", Some("stable-a"), "src/a.rs", 2),
                call("stable-a", Some("stable-b"), "src/a.rs", 3),
                call("stable-b", Some("stable-a"), "src/b.rs", 11),
            ],
        )
        .unwrap();

        let graph = get_call_graph(
            &conn,
            "repo",
            "main",
            &CallGraphRequest {
                symbol_name: "a",
                path: None,
                direction: CallGraphDirection::Callees,
                depth: 99,
                limit: 20,
            },
        )
        .unwrap();

        assert_eq!(graph.depth_applied, MAX_CALL_GRAPH_DEPTH);
        assert!(graph.callees.iter().any(|edge| edge.symbol.name == "a"));
        assert!(graph.callees.iter().any(|edge| edge.symbol.name == "b"));
        assert!(
            graph
                .callees
                .iter()
                .all(|edge| edge.depth <= MAX_CALL_GRAPH_DEPTH),
            "edge depths should be capped"
        );
    }

    #[test]
    fn get_call_graph_resolves_callee_via_symbol_id_fallback() {
        let conn = setup();
        let a = symbol("stable-a", "a", "src/a.rs", 1);
        let b = symbol("stable-b", "b", "src/b.rs", 10);
        symbols::insert_symbol(&conn, &a).unwrap();
        symbols::insert_symbol(&conn, &b).unwrap();

        edges::insert_call_edges(
            &conn,
            "repo",
            "main",
            &[CallEdge {
                repo: "repo".to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "stable-a".to_string(),
                // Intentionally use symbol_id (not symbol_stable_id) to exercise fallback.
                to_symbol_id: Some(b.symbol_id.clone()),
                to_name: None,
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
                source_file: "src/a.rs".to_string(),
                source_line: 2,
            }],
        )
        .unwrap();

        let graph = get_call_graph(
            &conn,
            "repo",
            "main",
            &CallGraphRequest {
                symbol_name: "a",
                path: None,
                direction: CallGraphDirection::Callees,
                depth: 1,
                limit: 20,
            },
        )
        .unwrap();

        assert_eq!(graph.callees.len(), 1);
        assert_eq!(graph.callees[0].symbol.symbol_stable_id, "stable-b");
        assert_eq!(graph.callees[0].symbol.name, "b");
    }

    #[test]
    #[ignore = "benchmark harness"]
    fn benchmark_t360_get_call_graph_p95_depth1_depth2_under_500ms() {
        let conn = setup();
        let symbol_count = 10_000usize;
        let mut edges_batch = Vec::with_capacity(symbol_count);

        for idx in 0..symbol_count {
            let name = format!("fn_{idx:05}");
            let stable = format!("stable-{idx:05}");
            symbols::insert_symbol(
                &conn,
                &symbol(&stable, &name, "src/synth.rs", idx as u32 + 1),
            )
            .unwrap();

            let next = (idx + 1) % symbol_count;
            let next_stable = format!("stable-{next:05}");
            edges_batch.push(call(
                &stable,
                Some(&next_stable),
                "src/synth.rs",
                idx as u32 + 1,
            ));
        }
        edges::insert_call_edges(&conn, "repo", "main", &edges_batch).unwrap();

        let mut depth1_times = Vec::new();
        let mut depth2_times = Vec::new();
        for _ in 0..30 {
            let start_depth1 = std::time::Instant::now();
            let result_depth1 = get_call_graph(
                &conn,
                "repo",
                "main",
                &CallGraphRequest {
                    symbol_name: "fn_00000",
                    path: Some("src/synth.rs"),
                    direction: CallGraphDirection::Callees,
                    depth: 1,
                    limit: 256,
                },
            )
            .unwrap();
            assert!(!result_depth1.callees.is_empty());
            depth1_times.push(start_depth1.elapsed().as_millis() as u64);

            let start_depth2 = std::time::Instant::now();
            let result_depth2 = get_call_graph(
                &conn,
                "repo",
                "main",
                &CallGraphRequest {
                    symbol_name: "fn_00000",
                    path: Some("src/synth.rs"),
                    direction: CallGraphDirection::Callees,
                    depth: 2,
                    limit: 256,
                },
            )
            .unwrap();
            assert!(result_depth2.callees.len() >= result_depth1.callees.len());
            depth2_times.push(start_depth2.elapsed().as_millis() as u64);
        }

        let p95_depth1 = percentile_95(&mut depth1_times);
        let p95_depth2 = percentile_95(&mut depth2_times);

        assert!(
            p95_depth1 < 500,
            "depth=1 p95 should be < 500ms, got {p95_depth1}ms"
        );
        assert!(
            p95_depth2 < 500,
            "depth=2 p95 should be < 500ms, got {p95_depth2}ms"
        );
    }

    fn percentile_95(values: &mut [u64]) -> u64 {
        values.sort_unstable();
        let idx = ((values.len() * 95).saturating_sub(1)) / 100;
        values[idx]
    }
}
