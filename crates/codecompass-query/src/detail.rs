use codecompass_core::types::DetailLevel;
use rusqlite::Connection;
use serde_json::{Value, json};

/// Fields included at each detail level.
/// Core identity fields (symbol_id, symbol_stable_id, result_id, result_type, score)
/// are always included since they're needed for cross-referencing and deduplication.
const LOCATION_FIELDS: &[&str] = &[
    "symbol_id",
    "symbol_stable_id",
    "result_id",
    "result_type",
    "source_layer",
    "path",
    "line_start",
    "line_end",
    "kind",
    "name",
    "score",
];
const SIGNATURE_FIELDS: &[&str] = &[
    "symbol_id",
    "symbol_stable_id",
    "result_id",
    "result_type",
    "source_layer",
    "path",
    "line_start",
    "line_end",
    "kind",
    "name",
    "qualified_name",
    "signature",
    "language",
    "visibility",
    "score",
];
const COMPACT_OMIT_FIELDS: &[&str] = &["snippet", "body_preview", "parent", "related_symbols"];

/// Serialize a result at the specified detail level.
/// Filters out fields not appropriate for the given level.
/// At `Context` level, all fields pass through (including enrichment fields).
pub fn serialize_result_at_level(result: &Value, level: DetailLevel, compact: bool) -> Value {
    let Some(obj) = result.as_object() else {
        return result.clone();
    };

    let mut output = match level {
        DetailLevel::Location => {
            let mut filtered = serde_json::Map::new();
            for &key in LOCATION_FIELDS {
                if let Some(v) = obj.get(key) {
                    filtered.insert(key.to_string(), v.clone());
                }
            }
            Value::Object(filtered)
        }
        DetailLevel::Signature => {
            let mut filtered = serde_json::Map::new();
            for &key in SIGNATURE_FIELDS {
                if let Some(v) = obj.get(key) {
                    filtered.insert(key.to_string(), v.clone());
                }
            }
            Value::Object(filtered)
        }
        DetailLevel::Context => {
            // At context level, pass through all fields (including enrichment)
            result.clone()
        }
    };

    if compact && let Some(out_obj) = output.as_object_mut() {
        for key in COMPACT_OMIT_FIELDS {
            out_obj.remove(*key);
        }
    }

    output
}

/// Serialize a list of results at the specified detail level.
pub fn serialize_results_at_level(
    results: &[Value],
    level: DetailLevel,
    compact: bool,
) -> Vec<Value> {
    results
        .iter()
        .map(|r| serialize_result_at_level(r, level, compact))
        .collect()
}

/// Generate a body_preview from full content: first N lines, truncated.
pub fn body_preview(content: Option<&str>, max_lines: usize) -> Option<String> {
    let content = content?;
    if content.is_empty() {
        return None;
    }
    let preview: String = content
        .lines()
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    if content.lines().count() > max_lines {
        Some(format!("{}\n    // ... truncated ...", preview))
    } else {
        Some(preview)
    }
}

/// Resolve parent context from symbol_relations.
/// Returns a JSON object with kind, name, path, line if parent exists.
pub fn resolve_parent(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    parent_symbol_id: &str,
) -> Option<Value> {
    if parent_symbol_id.is_empty() {
        return None;
    }
    let mut stmt = conn
        .prepare(
            "SELECT kind, name, path, line_start FROM symbol_relations
         WHERE repo = ?1 AND \"ref\" = ?2 AND symbol_id = ?3
         LIMIT 1",
        )
        .ok()?;

    stmt.query_row(rusqlite::params![repo, r#ref, parent_symbol_id], |row| {
        Ok(json!({
            "kind": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "path": row.get::<_, String>(2)?,
            "line": row.get::<_, u32>(3)?,
        }))
    })
    .ok()
}

/// Find related symbols in the same file (siblings or nearby symbols), limited to N.
pub fn resolve_related_symbols(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    path: &str,
    exclude_symbol_id: &str,
    limit: usize,
) -> Vec<Value> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT kind, name, path, line_start FROM symbol_relations
         WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3 AND symbol_id != ?4
         ORDER BY line_start
         LIMIT ?5",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map(
        rusqlite::params![repo, r#ref, path, exclude_symbol_id, limit as i64],
        |row| {
            Ok(json!({
                "kind": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "path": row.get::<_, String>(2)?,
                "line": row.get::<_, u32>(3)?,
            }))
        },
    ) else {
        return Vec::new();
    };

    rows.filter_map(|r| r.ok()).collect()
}

/// Enrich result JSON objects with body_preview from existing snippet/content fields.
/// Does not require a DB connection.
pub fn enrich_body_previews(results: &mut [Value]) {
    for result in results.iter_mut() {
        let Some(obj) = result.as_object_mut() else {
            continue;
        };

        let content = obj
            .get("snippet")
            .or_else(|| obj.get("content"))
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some(preview) = body_preview(content.as_deref(), 10) {
            obj.insert("body_preview".to_string(), Value::String(preview));
        }
    }
}

/// Enrich result JSON objects with parent and related_symbols from SQLite.
/// Requires a DB connection.
pub fn enrich_results_with_relations(
    results: &mut [Value],
    conn: &Connection,
    repo: &str,
    r#ref: &str,
) {
    for result in results.iter_mut() {
        let Some(obj) = result.as_object_mut() else {
            continue;
        };

        // parent resolution
        let parent_id = get_parent_symbol_id(conn, repo, r#ref, obj);
        if let Some(parent_id) = &parent_id
            && let Some(parent) = resolve_parent(conn, repo, r#ref, parent_id)
        {
            obj.insert("parent".to_string(), parent);
        }

        // related_symbols resolution
        let symbol_id = obj.get("symbol_id").and_then(|v| v.as_str()).unwrap_or("");
        let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if !path.is_empty() {
            let related = resolve_related_symbols(conn, repo, r#ref, path, symbol_id, 5);
            if !related.is_empty() {
                obj.insert("related_symbols".to_string(), Value::Array(related));
            }
        }
    }
}

/// Enrich result JSON objects with all context-level fields (body_preview, parent, related_symbols).
/// Convenience wrapper that calls both body preview and relation enrichment.
pub fn enrich_results_with_context(
    results: &mut [Value],
    conn: &Connection,
    repo: &str,
    r#ref: &str,
) {
    enrich_body_previews(results);
    enrich_results_with_relations(results, conn, repo, r#ref);
}

/// Look up parent_symbol_id from symbol_relations for a given result.
fn get_parent_symbol_id(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    obj: &serde_json::Map<String, Value>,
) -> Option<String> {
    let symbol_id = obj.get("symbol_id").and_then(|v| v.as_str())?;
    if symbol_id.is_empty() {
        return None;
    }
    let mut stmt = conn
        .prepare(
            "SELECT parent_symbol_id FROM symbol_relations
         WHERE repo = ?1 AND \"ref\" = ?2 AND symbol_id = ?3
         LIMIT 1",
        )
        .ok()?;
    stmt.query_row(rusqlite::params![repo, r#ref, symbol_id], |row| {
        row.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
    .filter(|s| !s.is_empty())
}
