use cruxe_core::error::StateError;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum SymbolCompareError {
    #[error("symbol not found")]
    SymbolNotFound,
    #[error(transparent)]
    State(#[from] StateError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolVersion {
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub signature: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub kind: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolDiffSummary {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_added: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_removed: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range_shifted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolComparisonResult {
    pub symbol: String,
    pub path: String,
    pub symbol_stable_id: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
    pub base_version: Option<SymbolVersion>,
    pub head_version: Option<SymbolVersion>,
    pub diff_summary: SymbolDiffSummary,
}

const MAX_LCS_LINES: usize = 2000;

#[derive(Debug, Clone)]
struct SymbolSnapshot {
    symbol_id: String,
    symbol_stable_id: String,
    name: String,
    path: String,
    signature: Option<String>,
    kind: String,
    language: String,
    line_start: u32,
    line_end: u32,
    content_hash: String,
    content: Option<String>,
}

pub fn compare_symbol_between_refs(
    conn: &Connection,
    repo: &str,
    symbol_name: &str,
    path: Option<&str>,
    base_ref: &str,
    head_ref: &str,
) -> Result<SymbolComparisonResult, SymbolCompareError> {
    let base = resolve_symbol_snapshot(conn, repo, base_ref, symbol_name, path)?;
    let head = resolve_symbol_snapshot(conn, repo, head_ref, symbol_name, path)?;

    if base.is_none() && head.is_none() {
        return Err(SymbolCompareError::SymbolNotFound);
    }

    let symbol = head
        .as_ref()
        .or(base.as_ref())
        .map(|snapshot| snapshot.name.clone())
        .unwrap_or_else(|| symbol_name.to_string());
    let selected_path = path
        .map(ToString::to_string)
        .or_else(|| head.as_ref().map(|snapshot| snapshot.path.clone()))
        .or_else(|| base.as_ref().map(|snapshot| snapshot.path.clone()))
        .unwrap_or_default();
    let stable_id = head
        .as_ref()
        .or(base.as_ref())
        .map(|snapshot| snapshot.symbol_stable_id.clone());

    let base_version = base.as_ref().map(to_version);
    let head_version = head.as_ref().map(to_version);
    let diff_summary = build_diff_summary(base.as_ref(), head.as_ref());

    Ok(SymbolComparisonResult {
        symbol,
        path: selected_path,
        symbol_stable_id: stable_id,
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        base_version,
        head_version,
        diff_summary,
    })
}

fn build_diff_summary(
    base: Option<&SymbolSnapshot>,
    head: Option<&SymbolSnapshot>,
) -> SymbolDiffSummary {
    match (base, head) {
        (Some(base), Some(head)) => {
            let signature_changed = base.signature != head.signature;
            let body_changed = base.content_hash != head.content_hash;
            let (lines_added, lines_removed) = modified_line_delta(base, head);
            let line_range_shifted =
                base.line_start != head.line_start || base.line_end != head.line_end;
            let status = if !signature_changed && !body_changed && !line_range_shifted {
                "unchanged"
            } else {
                "modified"
            };
            SymbolDiffSummary {
                status: status.to_string(),
                signature_changed: Some(signature_changed),
                body_changed: Some(body_changed),
                lines_added: Some(lines_added),
                lines_removed: Some(lines_removed),
                line_range_shifted: Some(line_range_shifted),
            }
        }
        (None, Some(head)) => SymbolDiffSummary {
            status: "added".to_string(),
            signature_changed: Some(true),
            body_changed: Some(true),
            lines_added: Some(snapshot_line_count(head)),
            lines_removed: Some(0),
            line_range_shifted: Some(true),
        },
        (Some(base), None) => SymbolDiffSummary {
            status: "deleted".to_string(),
            signature_changed: Some(true),
            body_changed: Some(true),
            lines_added: Some(0),
            lines_removed: Some(snapshot_line_count(base)),
            line_range_shifted: Some(true),
        },
        (None, None) => SymbolDiffSummary {
            status: "unchanged".to_string(),
            signature_changed: None,
            body_changed: None,
            lines_added: None,
            lines_removed: None,
            line_range_shifted: None,
        },
    }
}

fn line_span(snapshot: &SymbolSnapshot) -> u32 {
    snapshot
        .line_end
        .saturating_sub(snapshot.line_start)
        .saturating_add(1)
}

fn snapshot_line_count(snapshot: &SymbolSnapshot) -> u32 {
    match snapshot.content.as_deref() {
        Some(content) => {
            let count = content.lines().count() as u32;
            if count > 0 {
                count
            } else {
                line_span(snapshot)
            }
        }
        None => line_span(snapshot),
    }
}

fn modified_line_delta(base: &SymbolSnapshot, head: &SymbolSnapshot) -> (u32, u32) {
    match (base.content.as_deref(), head.content.as_deref()) {
        (Some(base_content), Some(head_content)) => line_diff_counts(base_content, head_content),
        _ => {
            let base_span = line_span(base);
            let head_span = line_span(head);
            (
                head_span.saturating_sub(base_span),
                base_span.saturating_sub(head_span),
            )
        }
    }
}

fn line_diff_counts(base_content: &str, head_content: &str) -> (u32, u32) {
    let base_lines: Vec<&str> = base_content.lines().collect();
    let head_lines: Vec<&str> = head_content.lines().collect();
    if base_lines == head_lines {
        return (0, 0);
    }
    if base_lines.len() > MAX_LCS_LINES || head_lines.len() > MAX_LCS_LINES {
        let added = head_lines.len().saturating_sub(base_lines.len()) as u32;
        let removed = base_lines.len().saturating_sub(head_lines.len()) as u32;
        if added == 0 && removed == 0 {
            return (1, 1);
        }
        return (added, removed);
    }

    let lcs_len = lcs_length(&base_lines, &head_lines);
    let lines_added = head_lines.len().saturating_sub(lcs_len) as u32;
    let lines_removed = base_lines.len().saturating_sub(lcs_len) as u32;
    (lines_added, lines_removed)
}

fn lcs_length(left: &[&str], right: &[&str]) -> usize {
    if left.is_empty() || right.is_empty() {
        return 0;
    }

    let mut prev = vec![0usize; right.len() + 1];
    for left_line in left {
        let mut curr = vec![0usize; right.len() + 1];
        for (j, right_line) in right.iter().enumerate() {
            curr[j + 1] = if left_line == right_line {
                prev[j] + 1
            } else {
                curr[j].max(prev[j + 1])
            };
        }
        prev = curr;
    }
    prev[right.len()]
}

fn to_version(snapshot: &SymbolSnapshot) -> SymbolVersion {
    SymbolVersion {
        symbol_id: snapshot.symbol_id.clone(),
        symbol_stable_id: snapshot.symbol_stable_id.clone(),
        signature: snapshot.signature.clone(),
        line_start: snapshot.line_start,
        line_end: snapshot.line_end,
        kind: snapshot.kind.clone(),
        language: snapshot.language.clone(),
    }
}

fn resolve_symbol_snapshot(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_name: &str,
    path: Option<&str>,
) -> Result<Option<SymbolSnapshot>, StateError> {
    let mut matches = query_symbol_snapshots_by(conn, repo, ref_name, symbol_name, path, "name")?;
    if matches.is_empty() {
        matches =
            query_symbol_snapshots_by(conn, repo, ref_name, symbol_name, path, "qualified_name")?;
    }
    matches.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line_start.cmp(&right.line_start))
            .then_with(|| left.symbol_id.cmp(&right.symbol_id))
    });
    Ok(matches.into_iter().next())
}

fn query_symbol_snapshots_by(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    symbol_name: &str,
    path: Option<&str>,
    field: &str,
) -> Result<Vec<SymbolSnapshot>, StateError> {
    let field_name = match field {
        "name" => "name",
        "qualified_name" => "qualified_name",
        _ => return Ok(Vec::new()),
    };
    let sql = if path.is_some() {
        format!(
            "SELECT symbol_id, symbol_stable_id, name, path, signature, kind, language, line_start, line_end, content_hash, content
             FROM symbol_relations
             WHERE repo = ?1 AND \"ref\" = ?2 AND {field_name} = ?3 AND path = ?4
             ORDER BY line_start"
        )
    } else {
        format!(
            "SELECT symbol_id, symbol_stable_id, name, path, signature, kind, language, line_start, line_end, content_hash, content
             FROM symbol_relations
             WHERE repo = ?1 AND \"ref\" = ?2 AND {field_name} = ?3
             ORDER BY path, line_start"
        )
    };
    let mut stmt = conn.prepare(&sql).map_err(StateError::sqlite)?;
    let rows = if let Some(file_path) = path {
        stmt.query_map(
            params![repo, ref_name, symbol_name, file_path],
            row_to_snapshot,
        )
        .map_err(StateError::sqlite)?
    } else {
        stmt.query_map(params![repo, ref_name, symbol_name], row_to_snapshot)
            .map_err(StateError::sqlite)?
    };
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

fn row_to_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolSnapshot> {
    Ok(SymbolSnapshot {
        symbol_id: row.get(0)?,
        symbol_stable_id: row.get(1)?,
        name: row.get(2)?,
        path: row.get(3)?,
        signature: row.get(4)?,
        kind: row.get(5)?,
        language: row.get(6)?,
        line_start: row.get(7)?,
        line_end: row.get(8)?,
        content_hash: row.get(9)?,
        content: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema, symbols};

    fn setup() -> Connection {
        let tmp = tempfile::tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn make_symbol(
        ref_name: &str,
        name: &str,
        path: &str,
        signature: &str,
        line_start: u32,
        line_end: u32,
        body: &str,
    ) -> SymbolRecord {
        SymbolRecord {
            repo: "repo".to_string(),
            r#ref: ref_name.to_string(),
            commit: None,
            path: path.to_string(),
            symbol_id: format!("{ref_name}::{name}"),
            symbol_stable_id: format!("stable::{name}"),
            name: name.to_string(),
            qualified_name: format!("crate::{name}"),
            kind: SymbolKind::Function,
            language: "rust".to_string(),
            line_start,
            line_end,
            signature: Some(signature.to_string()),
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some(body.to_string()),
        }
    }

    #[test]
    fn compare_symbol_between_refs_reports_modified_summary() {
        let conn = setup();
        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "main",
                "process",
                "src/lib.rs",
                "fn process()",
                10,
                14,
                "a();",
            ),
        )
        .unwrap();
        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "feat/auth",
                "process",
                "src/lib.rs",
                "fn process(ctx: Ctx)",
                10,
                18,
                "a();\nb();",
            ),
        )
        .unwrap();

        let result = compare_symbol_between_refs(
            &conn,
            "repo",
            "process",
            Some("src/lib.rs"),
            "main",
            "feat/auth",
        )
        .unwrap();

        assert_eq!(result.diff_summary.status, "modified");
        assert_eq!(result.diff_summary.signature_changed, Some(true));
        assert_eq!(result.diff_summary.body_changed, Some(true));
        assert_eq!(result.diff_summary.lines_added, Some(1));
        assert_eq!(result.diff_summary.lines_removed, Some(0));
    }

    #[test]
    fn compare_symbol_between_refs_counts_line_diff_from_body_content() {
        let conn = setup();
        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "main",
                "process",
                "src/lib.rs",
                "fn process()",
                10,
                18,
                "a();\nb();",
            ),
        )
        .unwrap();
        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "feat/auth",
                "process",
                "src/lib.rs",
                "fn process()",
                10,
                18,
                "a();\nc();\nd();",
            ),
        )
        .unwrap();

        let result = compare_symbol_between_refs(
            &conn,
            "repo",
            "process",
            Some("src/lib.rs"),
            "main",
            "feat/auth",
        )
        .unwrap();

        assert_eq!(result.diff_summary.status, "modified");
        assert_eq!(result.diff_summary.signature_changed, Some(false));
        assert_eq!(result.diff_summary.body_changed, Some(true));
        assert_eq!(result.diff_summary.lines_added, Some(2));
        assert_eq!(result.diff_summary.lines_removed, Some(1));
        assert_eq!(result.diff_summary.line_range_shifted, Some(false));
    }

    #[test]
    fn compare_symbol_between_refs_reports_added_and_deleted() {
        let conn = setup();
        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "feat/auth",
                "new_fn",
                "src/lib.rs",
                "fn new_fn()",
                30,
                34,
                "x();",
            ),
        )
        .unwrap();
        let added = compare_symbol_between_refs(
            &conn,
            "repo",
            "new_fn",
            Some("src/lib.rs"),
            "main",
            "feat/auth",
        )
        .unwrap();
        assert_eq!(added.diff_summary.status, "added");
        assert!(added.base_version.is_none());
        assert!(added.head_version.is_some());

        symbols::insert_symbol(
            &conn,
            &make_symbol(
                "main",
                "old_fn",
                "src/lib.rs",
                "fn old_fn()",
                50,
                55,
                "y();",
            ),
        )
        .unwrap();
        let deleted = compare_symbol_between_refs(
            &conn,
            "repo",
            "old_fn",
            Some("src/lib.rs"),
            "main",
            "feat/auth",
        )
        .unwrap();
        assert_eq!(deleted.diff_summary.status, "deleted");
        assert!(deleted.base_version.is_some());
        assert!(deleted.head_version.is_none());
    }

    #[test]
    fn compare_symbol_between_refs_reports_unchanged_when_hash_and_signature_match() {
        let conn = setup();
        let symbol = make_symbol(
            "main",
            "validate",
            "src/lib.rs",
            "fn validate()",
            5,
            9,
            "ok()",
        );
        symbols::insert_symbol(&conn, &symbol).unwrap();
        let mut same = symbol.clone();
        same.r#ref = "feat/auth".to_string();
        same.symbol_id = "feat/auth::validate".to_string();
        symbols::insert_symbol(&conn, &same).unwrap();

        let result = compare_symbol_between_refs(
            &conn,
            "repo",
            "validate",
            Some("src/lib.rs"),
            "main",
            "feat/auth",
        )
        .unwrap();

        assert_eq!(result.diff_summary.status, "unchanged");
        assert_eq!(result.diff_summary.signature_changed, Some(false));
        assert_eq!(result.diff_summary.body_changed, Some(false));
        assert_eq!(result.diff_summary.lines_added, Some(0));
        assert_eq!(result.diff_summary.lines_removed, Some(0));
        assert_eq!(result.diff_summary.line_range_shifted, Some(false));
    }

    #[test]
    fn line_diff_counts_large_bodies_use_linear_fallback() {
        let base = std::iter::repeat_n("a()", 2001)
            .collect::<Vec<_>>()
            .join("\n");
        let head = std::iter::repeat_n("b()", 2001)
            .collect::<Vec<_>>()
            .join("\n");
        let (lines_added, lines_removed) = line_diff_counts(&base, &head);

        assert_eq!(
            (lines_added, lines_removed),
            (1, 1),
            "large bodies should avoid quadratic LCS cost and return coarse fallback deltas"
        );
    }
}
