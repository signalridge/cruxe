use cruxe_core::error::StateError;
use cruxe_core::types::{SourceLayer, SymbolRecord};
use cruxe_state::{project, symbols, tombstones};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum FindReferencesError {
    #[error("symbol not found")]
    SymbolNotFound,
    #[error("no edges available")]
    NoEdgesAvailable,
    #[error(transparent)]
    State(#[from] StateError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSymbol {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub path: String,
    pub line_start: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceResult {
    pub path: String,
    pub line_start: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
    pub edge_type: String,
    pub source_layer: SourceLayer,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub from_symbol: ReferenceSymbol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReferencesResult {
    pub symbol: ReferenceSymbol,
    pub references: Vec<ReferenceResult>,
    pub total_references: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EdgeRow {
    from_symbol_id: String,
    to_symbol_id: String,
    edge_type: String,
}

pub fn find_references(
    conn: &Connection,
    workspace: &Path,
    project_id: &str,
    ref_name: &str,
    kind_filter: Option<&str>,
    symbol_name: &str,
    limit: usize,
) -> Result<FindReferencesResult, FindReferencesError> {
    let Some(project_row) = project::get_by_id(conn, project_id)? else {
        return Err(FindReferencesError::State(StateError::ProjectNotFound {
            project_id: project_id.to_string(),
        }));
    };

    let Some(target_symbol) = resolve_target_symbol(
        conn,
        project_id,
        ref_name,
        &project_row.default_ref,
        project_row.vcs_mode,
        symbol_name,
    )?
    else {
        return Err(FindReferencesError::SymbolNotFound);
    };

    let target_result_symbol = symbol_to_reference_symbol(&target_symbol);
    let target_ids = [
        target_symbol.symbol_id.as_str(),
        target_symbol.symbol_stable_id.as_str(),
    ];

    let no_edges = if project_row.vcs_mode && ref_name != project_row.default_ref {
        edge_count(conn, project_id, &project_row.default_ref)?
            + edge_count(conn, project_id, ref_name)?
            == 0
    } else {
        edge_count(conn, project_id, ref_name)? == 0
    };
    if no_edges {
        return Err(FindReferencesError::NoEdgesAvailable);
    }

    let (base_rows, overlay_rows) = if project_row.vcs_mode && ref_name != project_row.default_ref {
        let base_rows = query_edge_rows(
            conn,
            project_id,
            &project_row.default_ref,
            &target_ids,
            kind_filter,
        )?;
        let overlay_rows = query_edge_rows(conn, project_id, ref_name, &target_ids, kind_filter)?;
        (base_rows, overlay_rows)
    } else {
        (
            query_edge_rows(conn, project_id, ref_name, &target_ids, kind_filter)?,
            Vec::new(),
        )
    };

    let mut merged: HashMap<(String, String, String), ReferenceResult> = HashMap::new();
    let tombstone_paths = if project_row.vcs_mode && ref_name != project_row.default_ref {
        tombstones::list_paths_for_ref(conn, project_id, ref_name)?
            .into_iter()
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    for row in base_rows {
        let reference = to_reference_result(
            conn,
            workspace,
            project_id,
            &project_row.default_ref,
            row,
            SourceLayer::Base,
        )?;
        if tombstone_paths.contains(&reference.path) {
            continue;
        }
        let key = reference_key(&reference);
        merged.insert(key, reference);
    }
    for row in overlay_rows {
        let reference = to_reference_result(
            conn,
            workspace,
            project_id,
            ref_name,
            row,
            SourceLayer::Overlay,
        )?;
        let key = reference_key(&reference);
        merged.insert(key, reference);
    }

    let mut references: Vec<ReferenceResult> = merged.into_values().collect();
    references.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line_start.cmp(&b.line_start))
            .then_with(|| a.edge_type.cmp(&b.edge_type))
            .then_with(|| a.from_symbol.symbol_id.cmp(&b.from_symbol.symbol_id))
    });
    let total_references = references.len();
    if limit > 0 && references.len() > limit {
        references.truncate(limit);
    }

    Ok(FindReferencesResult {
        symbol: target_result_symbol,
        references,
        total_references,
    })
}

fn resolve_target_symbol(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    default_ref: &str,
    vcs_mode: bool,
    symbol_name: &str,
) -> Result<Option<SymbolRecord>, StateError> {
    if let Some(symbol) = lookup_symbol(conn, project_id, ref_name, symbol_name)? {
        return Ok(Some(symbol));
    }
    if vcs_mode && ref_name != default_ref {
        return lookup_symbol(conn, project_id, default_ref, symbol_name);
    }
    Ok(None)
}

fn lookup_symbol(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_name: &str,
) -> Result<Option<SymbolRecord>, StateError> {
    let exact = symbols::find_symbols_by_name(conn, project_id, ref_name, symbol_name, None)?;
    if let Some(first) = exact.into_iter().next() {
        return Ok(Some(first));
    }

    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", \"commit\", path, symbol_id, symbol_stable_id, \
                    name, qualified_name, kind, language, line_start, line_end, \
                    signature, parent_symbol_id, visibility \
             FROM symbol_relations \
             WHERE repo = ?1 AND \"ref\" = ?2 AND qualified_name = ?3 \
             ORDER BY line_start LIMIT 1",
        )
        .map_err(StateError::sqlite)?;
    match stmt.query_row(params![project_id, ref_name, symbol_name], |row| {
        Ok(SymbolRecord {
            repo: row.get(0)?,
            r#ref: row.get(1)?,
            commit: row.get(2)?,
            path: row.get(3)?,
            symbol_id: row.get(4)?,
            symbol_stable_id: row.get(5)?,
            name: row.get(6)?,
            qualified_name: row.get(7)?,
            kind: cruxe_core::types::SymbolKind::parse_kind(&row.get::<_, String>(8)?)
                .unwrap_or(cruxe_core::types::SymbolKind::Function),
            language: row.get(9)?,
            line_start: row.get(10)?,
            line_end: row.get(11)?,
            signature: row.get(12)?,
            parent_symbol_id: row.get(13)?,
            visibility: row.get(14)?,
            content: None,
        })
    }) {
        Ok(symbol) => Ok(Some(symbol)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(StateError::sqlite(err)),
    }
}

fn query_edge_rows(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    target_ids: &[&str; 2],
    kind_filter: Option<&str>,
) -> Result<Vec<EdgeRow>, StateError> {
    let mut sql = String::from(
        "SELECT from_symbol_id, to_symbol_id, edge_type \
         FROM symbol_edges \
         WHERE repo = ?1 AND \"ref\" = ?2 \
           AND (to_symbol_id = ?3 OR to_symbol_id = ?4)",
    );
    if kind_filter.is_some() {
        sql.push_str(" AND edge_type = ?5");
    }
    sql.push_str(" ORDER BY from_symbol_id, to_symbol_id, edge_type");
    let mut stmt = conn.prepare(&sql).map_err(StateError::sqlite)?;
    let rows = if let Some(kind) = kind_filter {
        stmt.query_map(
            params![project_id, ref_name, target_ids[0], target_ids[1], kind],
            map_edge_row,
        )
        .map_err(StateError::sqlite)?
    } else {
        stmt.query_map(
            params![project_id, ref_name, target_ids[0], target_ids[1]],
            map_edge_row,
        )
        .map_err(StateError::sqlite)?
    };
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

fn map_edge_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EdgeRow> {
    Ok(EdgeRow {
        from_symbol_id: row.get(0)?,
        to_symbol_id: row.get(1)?,
        edge_type: row.get(2)?,
    })
}

fn edge_count(conn: &Connection, project_id: &str, ref_name: &str) -> Result<u64, StateError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_edges WHERE repo = ?1 AND \"ref\" = ?2",
            params![project_id, ref_name],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;
    Ok(count.max(0) as u64)
}

fn to_reference_result(
    conn: &Connection,
    workspace: &Path,
    project_id: &str,
    ref_name: &str,
    row: EdgeRow,
    source_layer: SourceLayer,
) -> Result<ReferenceResult, StateError> {
    let from_symbol = resolve_from_symbol(conn, project_id, ref_name, &row.from_symbol_id)?;
    let path = from_symbol.path.clone();
    let line_start = from_symbol.line_start;
    Ok(ReferenceResult {
        path: path.clone(),
        line_start,
        line_end: from_symbol.line_end,
        edge_type: row.edge_type,
        source_layer,
        context: read_source_line(workspace, ref_name, &path, line_start),
        from_symbol,
    })
}

fn resolve_from_symbol(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    from_symbol_id: &str,
) -> Result<ReferenceSymbol, StateError> {
    if let Some(symbol) = symbols::get_symbol_by_id(conn, project_id, ref_name, from_symbol_id)? {
        return Ok(symbol_to_reference_symbol(&symbol));
    }
    if let Some(symbol) =
        symbols::get_symbol_by_stable_id(conn, project_id, ref_name, from_symbol_id)?
    {
        return Ok(symbol_to_reference_symbol(&symbol));
    }
    if let Some(path) = from_symbol_id.strip_prefix("file::") {
        let fallback_name = Path::new(path)
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("module")
            .to_string();
        return Ok(ReferenceSymbol {
            symbol_id: from_symbol_id.to_string(),
            symbol_stable_id: from_symbol_id.to_string(),
            name: fallback_name,
            qualified_name: from_symbol_id.to_string(),
            kind: "module".to_string(),
            path: path.to_string(),
            line_start: 1,
            line_end: None,
        });
    }
    Ok(ReferenceSymbol {
        symbol_id: from_symbol_id.to_string(),
        symbol_stable_id: from_symbol_id.to_string(),
        name: from_symbol_id.to_string(),
        qualified_name: from_symbol_id.to_string(),
        kind: "unknown".to_string(),
        path: String::new(),
        line_start: 0,
        line_end: None,
    })
}

fn symbol_to_reference_symbol(symbol: &SymbolRecord) -> ReferenceSymbol {
    ReferenceSymbol {
        symbol_id: symbol.symbol_id.clone(),
        symbol_stable_id: symbol.symbol_stable_id.clone(),
        name: symbol.name.clone(),
        qualified_name: symbol.qualified_name.clone(),
        kind: symbol.kind.as_str().to_string(),
        path: symbol.path.clone(),
        line_start: symbol.line_start,
        line_end: Some(symbol.line_end),
    }
}

fn read_source_line(
    workspace: &Path,
    ref_name: &str,
    relative_path: &str,
    line_start: u32,
) -> Option<String> {
    if line_start == 0 {
        return None;
    }
    read_source_line_from_git_ref(workspace, ref_name, relative_path, line_start)
        .or_else(|| read_source_line_from_workspace(workspace, relative_path, line_start))
}

fn read_source_line_from_workspace(
    workspace: &Path,
    relative_path: &str,
    line_start: u32,
) -> Option<String> {
    let path = workspace.join(relative_path);
    let content = std::fs::read_to_string(path).ok()?;
    content
        .lines()
        .nth(line_start.saturating_sub(1) as usize)
        .map(|value| value.trim().to_string())
}

fn read_source_line_from_git_ref(
    workspace: &Path,
    ref_name: &str,
    relative_path: &str,
    line_start: u32,
) -> Option<String> {
    if !cruxe_core::vcs::is_git_repo(workspace) {
        return None;
    }
    let object = format!("{ref_name}:{relative_path}");
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .arg("show")
        .arg(&object)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let content = String::from_utf8(output.stdout).ok()?;
    content
        .lines()
        .nth(line_start.saturating_sub(1) as usize)
        .map(|value| value.trim().to_string())
}

fn reference_key(reference: &ReferenceResult) -> (String, String, String) {
    (
        reference.path.clone(),
        reference.from_symbol.symbol_id.clone(),
        reference.edge_type.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::{SymbolEdge, SymbolKind};
    use cruxe_state::{db, edges, schema, symbols};

    fn setup() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        (tmp, conn)
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_symbol(
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        id: &str,
        stable_id: &str,
        name: &str,
        path: &str,
        line_start: u32,
    ) {
        symbols::insert_symbol(
            conn,
            &SymbolRecord {
                repo: repo.to_string(),
                r#ref: ref_name.to_string(),
                commit: None,
                path: path.to_string(),
                language: "rust".to_string(),
                symbol_id: id.to_string(),
                symbol_stable_id: stable_id.to_string(),
                name: name.to_string(),
                qualified_name: format!("crate::{name}"),
                kind: SymbolKind::Function,
                signature: Some(format!("fn {name}()")),
                line_start,
                line_end: line_start + 2,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some(name.to_string()),
            },
        )
        .unwrap();
    }

    #[test]
    fn find_references_returns_edges_with_metadata() {
        let (tmp, conn) = setup();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "use crate::validate_token;\nfn call_site() { validate_token(); }\n",
        )
        .unwrap();

        let project_id = "proj";
        let now = "2026-02-25T00:00:00Z".to_string();
        project::create_project(
            &conn,
            &cruxe_core::types::Project {
                project_id: project_id.to_string(),
                repo_root: workspace.to_string_lossy().to_string(),
                display_name: Some("test".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .unwrap();

        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-target",
            "stable-target",
            "validate_token",
            "src/auth.rs",
            10,
        );
        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-caller",
            "stable-caller",
            "call_site",
            "src/lib.rs",
            2,
        );

        edges::insert_edges(
            &conn,
            project_id,
            "main",
            vec![SymbolEdge {
                repo: project_id.to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "sym-caller".to_string(),
                to_symbol_id: "stable-target".to_string(),
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
            }],
        )
        .unwrap();

        let result = find_references(
            &conn,
            &workspace,
            project_id,
            "main",
            None,
            "validate_token",
            20,
        )
        .unwrap();

        assert_eq!(result.symbol.name, "validate_token");
        assert_eq!(result.total_references, 1);
        assert_eq!(result.references[0].edge_type, "calls");
        assert_eq!(result.references[0].source_layer, SourceLayer::Base);
        assert_eq!(result.references[0].from_symbol.name, "call_site");
    }

    #[test]
    fn find_references_only_returns_incoming_edges_to_target_symbol() {
        let (tmp, conn) = setup();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src/lib.rs"),
            "fn call_site() { validate_token(); }\nfn validate_token() {}\n",
        )
        .unwrap();

        let project_id = "proj";
        let now = "2026-02-25T00:00:00Z".to_string();
        project::create_project(
            &conn,
            &cruxe_core::types::Project {
                project_id: project_id.to_string(),
                repo_root: workspace.to_string_lossy().to_string(),
                display_name: Some("test".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: false,
                schema_version: 1,
                parser_version: 1,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .unwrap();

        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-target",
            "stable-target",
            "validate_token",
            "src/lib.rs",
            2,
        );
        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-caller",
            "stable-caller",
            "call_site",
            "src/lib.rs",
            1,
        );
        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-outgoing",
            "stable-outgoing",
            "other_symbol",
            "src/lib.rs",
            3,
        );

        edges::insert_edges(
            &conn,
            project_id,
            "main",
            vec![
                SymbolEdge {
                    repo: project_id.to_string(),
                    ref_name: "main".to_string(),
                    from_symbol_id: "sym-caller".to_string(),
                    to_symbol_id: "stable-target".to_string(),
                    edge_type: "calls".to_string(),
                    confidence: "static".to_string(),
                },
                SymbolEdge {
                    repo: project_id.to_string(),
                    ref_name: "main".to_string(),
                    from_symbol_id: "stable-target".to_string(),
                    to_symbol_id: "stable-outgoing".to_string(),
                    edge_type: "calls".to_string(),
                    confidence: "static".to_string(),
                },
            ],
        )
        .unwrap();

        let result = find_references(
            &conn,
            &workspace,
            project_id,
            "main",
            None,
            "validate_token",
            20,
        )
        .unwrap();

        assert_eq!(result.total_references, 1);
        assert_eq!(result.references[0].from_symbol.symbol_id, "sym-caller");
        assert_eq!(result.references[0].from_symbol.name, "call_site");
    }

    #[test]
    fn find_references_reads_context_from_requested_ref() {
        let (tmp, conn) = setup();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();

        git(&workspace, &["init"]);
        git(&workspace, &["config", "user.email", "tests@example.com"]);
        git(&workspace, &["config", "user.name", "Cruxe Tests"]);
        std::fs::write(
            workspace.join("src/lib.rs"),
            "fn call_site() { validate_token_main(); }\n",
        )
        .unwrap();
        git(&workspace, &["add", "."]);
        git(&workspace, &["commit", "-m", "base"]);
        git(&workspace, &["branch", "-M", "main"]);
        git(&workspace, &["checkout", "-b", "feat/auth"]);
        std::fs::write(
            workspace.join("src/lib.rs"),
            "fn call_site() { validate_token_feat(); }\n",
        )
        .unwrap();
        git(&workspace, &["add", "."]);
        git(&workspace, &["commit", "-m", "feature"]);

        let project_id = "proj";
        let now = "2026-02-25T00:00:00Z".to_string();
        project::create_project(
            &conn,
            &cruxe_core::types::Project {
                project_id: project_id.to_string(),
                repo_root: workspace.to_string_lossy().to_string(),
                display_name: Some("test".to_string()),
                default_ref: "main".to_string(),
                vcs_mode: true,
                schema_version: 1,
                parser_version: 1,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .unwrap();

        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-target-main",
            "stable-target",
            "validate_token",
            "src/auth.rs",
            10,
        );
        insert_symbol(
            &conn,
            project_id,
            "feat/auth",
            "sym-target-feat",
            "stable-target",
            "validate_token",
            "src/auth.rs",
            10,
        );
        insert_symbol(
            &conn,
            project_id,
            "main",
            "sym-caller-main",
            "stable-caller-main",
            "call_site",
            "src/lib.rs",
            1,
        );
        insert_symbol(
            &conn,
            project_id,
            "feat/auth",
            "sym-caller-feat",
            "stable-caller-feat",
            "call_site",
            "src/lib.rs",
            1,
        );
        edges::insert_edges(
            &conn,
            project_id,
            "main",
            vec![SymbolEdge {
                repo: project_id.to_string(),
                ref_name: "main".to_string(),
                from_symbol_id: "sym-caller-main".to_string(),
                to_symbol_id: "stable-target".to_string(),
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
            }],
        )
        .unwrap();
        edges::insert_edges(
            &conn,
            project_id,
            "feat/auth",
            vec![SymbolEdge {
                repo: project_id.to_string(),
                ref_name: "feat/auth".to_string(),
                from_symbol_id: "sym-caller-feat".to_string(),
                to_symbol_id: "stable-target".to_string(),
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
            }],
        )
        .unwrap();

        let result = find_references(
            &conn,
            &workspace,
            project_id,
            "feat/auth",
            Some("calls"),
            "validate_token",
            20,
        )
        .unwrap();
        let base = result
            .references
            .iter()
            .find(|reference| reference.source_layer == SourceLayer::Base)
            .expect("base reference");
        let overlay = result
            .references
            .iter()
            .find(|reference| reference.source_layer == SourceLayer::Overlay)
            .expect("overlay reference");
        assert_eq!(
            base.context.as_deref(),
            Some("fn call_site() { validate_token_main(); }")
        );
        assert_eq!(
            overlay.context.as_deref(),
            Some("fn call_site() { validate_token_feat(); }")
        );
    }
}
