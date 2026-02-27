//! LanceDB vector storage backend (feature-gated).
//!
//! Stores vectors in LanceDB tables for efficient ANN search while keeping
//! schema-versioning metadata in SQLite via the parent module's
//! `semantic_vector_meta` table.
//!
//! ## Design decisions
//!
//! * **One table per dimension** (`semantic_vectors_{dim}d`) so the vector
//!   column can be a `FixedSizeList<Float32>` and ANN indices work out of
//!   the box.
//! * **Shared tokio runtime** (`LANCE_RUNTIME`) bridges async LanceDB calls
//!   into the synchronous `rusqlite::Connection`-centric API surface.
//! * **Connection cache** (`LANCE_CONN_CACHE`) avoids re-opening the
//!   database on every operation.
//! * **Merge-insert** for upserts keyed on
//!   `(project_id, ref_name, symbol_stable_id, snippet_hash, embedding_model_version)`.
//! * **Cosine distance** — LanceDB returns `_distance = 1 − cosine_similarity`,
//!   so we convert back: `score = 1.0 − _distance`.

use super::{VectorMatch, VectorQuery, VectorRecord};
use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    StringArray, UInt32Array,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use codecompass_core::error::StateError;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{Connection as LanceConnection, DistanceType};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::Runtime;

// ── Runtime & Connection Cache ──────────────────────────────────────────

/// Maximum number of cached LanceDB connections to prevent unbounded growth
/// in long-running processes.
const MAX_LANCE_CONN_CACHE_ENTRIES: usize = 64;

static LANCE_RUNTIME: OnceLock<Runtime> = OnceLock::new();
static LANCE_CONN_CACHE: OnceLock<Mutex<HashMap<String, LanceConnection>>> = OnceLock::new();

fn rt() -> Result<&'static Runtime, StateError> {
    // OnceLock::get_or_try_init is unstable, so we init eagerly and only
    // report failure on first access.
    if let Some(rt) = LANCE_RUNTIME.get() {
        return Ok(rt);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| {
            StateError::external(format!("failed to create tokio runtime for lancedb: {e}"))
        })?;
    // Another thread may have raced us; that's fine — the loser's runtime
    // is dropped and we use the winner's.
    Ok(LANCE_RUNTIME.get_or_init(|| runtime))
}

fn conn_cache() -> &'static Mutex<HashMap<String, LanceConnection>> {
    LANCE_CONN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn block_on<F: std::future::Future>(f: F) -> Result<F::Output, StateError> {
    // Detect if we're already inside a tokio runtime to avoid nested runtime
    // panics. Use `block_in_place` when inside a runtime, otherwise create a
    // fresh `block_on` call.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        Ok(tokio::task::block_in_place(|| handle.block_on(f)))
    } else {
        Ok(rt()?.block_on(f))
    }
}

// ── Path & Naming ───────────────────────────────────────────────────────

fn lancedb_dir(conn: &Connection) -> PathBuf {
    match conn.path() {
        Some(p) => std::path::Path::new(p)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("lancedb"),
        None => {
            // Use a per-connection unique directory for in-memory SQLite so
            // multiple instances don't collide on the same Lance temp path.
            let unique = format!("codecompass_lancedb_{:p}", conn);
            std::env::temp_dir().join(unique)
        }
    }
}

fn table_name(dim: usize) -> String {
    format!("semantic_vectors_{dim}d")
}

// ── Arrow Schema ────────────────────────────────────────────────────────

fn make_schema(dim: i32) -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("project_id", DataType::Utf8, false),
        Field::new("ref_name", DataType::Utf8, false),
        Field::new("symbol_stable_id", DataType::Utf8, false),
        Field::new("snippet_hash", DataType::Utf8, false),
        Field::new("embedding_model_id", DataType::Utf8, false),
        Field::new("embedding_model_version", DataType::Utf8, false),
        Field::new("path", DataType::Utf8, false),
        Field::new("line_start", DataType::UInt32, false),
        Field::new("line_end", DataType::UInt32, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("chunk_type", DataType::Utf8, true),
        Field::new("snippet_text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dim),
            false,
        ),
    ]))
}

// ── Connection Management ───────────────────────────────────────────────

async fn open_lance(conn: &Connection) -> Result<LanceConnection, StateError> {
    let dir = lancedb_dir(conn);
    let key = dir.to_string_lossy().to_string();

    if let Ok(cache) = conn_cache().lock()
        && let Some(c) = cache.get(&key)
    {
        return Ok(c.clone());
    }

    std::fs::create_dir_all(&dir).map_err(StateError::Io)?;

    let c = lancedb::connect(dir.to_str().unwrap_or("."))
        .execute()
        .await
        .map_err(|e| StateError::external(format!("lancedb connect: {e}")))?;

    if let Ok(mut cache) = conn_cache().lock() {
        // Evict oldest entry when cache is full.
        if cache.len() >= MAX_LANCE_CONN_CACHE_ENTRIES
            && let Some(evict_key) = cache.keys().next().cloned()
        {
            cache.remove(&evict_key);
        }
        cache.insert(key, c.clone());
    }
    Ok(c)
}

/// Minimum row count before attempting to build a vector index.
/// Below this threshold brute-force scan is fast enough and IVF-PQ would be
/// poorly calibrated.
const ANN_INDEX_ROW_THRESHOLD: usize = 256;

async fn open_or_create_table(
    lance: &LanceConnection,
    dim: usize,
) -> Result<lancedb::Table, StateError> {
    let name = table_name(dim);
    match lance.open_table(&name).execute().await {
        Ok(t) => Ok(t),
        Err(lancedb::Error::TableNotFound { .. }) => {
            let schema = make_schema(dim as i32);
            lance
                .create_empty_table(&name, schema)
                .execute()
                .await
                .map_err(|e| StateError::external(format!("lancedb create_table: {e}")))
        }
        Err(e) => Err(StateError::external(format!("lancedb open_table: {e}"))),
    }
}

/// Attempt to create an ANN vector index on the table if the row count is
/// above the threshold and no index exists yet.  This is best-effort: if index
/// creation fails we log and continue (brute-force scan still works).
async fn maybe_create_ann_index(table: &lancedb::Table) {
    let row_count = table.count_rows(None).await.unwrap_or(0);
    if row_count < ANN_INDEX_ROW_THRESHOLD {
        return;
    }

    // Check if an index already exists by listing indices.
    let indices = match table.list_indices().await {
        Ok(indices) => indices,
        Err(_) => return,
    };
    let has_vector_index = indices
        .iter()
        .any(|idx| idx.columns.contains(&"vector".to_string()));
    if has_vector_index {
        return;
    }

    tracing::info!(
        rows = row_count,
        "creating ANN vector index on lancedb table"
    );
    if let Err(e) = table
        .create_index(&["vector"], lancedb::index::Index::Auto)
        .execute()
        .await
    {
        tracing::warn!(error = %e, "failed to create ANN index (brute-force scan still works)");
    }
}

// ── RecordBatch Construction ────────────────────────────────────────────

fn records_to_batch(records: &[VectorRecord], dim: i32) -> Result<RecordBatch, StateError> {
    let schema = make_schema(dim);
    let dim_usize = dim as usize;

    let project_ids = StringArray::from(
        records
            .iter()
            .map(|r| r.project_id.as_str())
            .collect::<Vec<_>>(),
    );
    let ref_names = StringArray::from(
        records
            .iter()
            .map(|r| r.ref_name.as_str())
            .collect::<Vec<_>>(),
    );
    let symbol_stable_ids = StringArray::from(
        records
            .iter()
            .map(|r| r.symbol_stable_id.as_str())
            .collect::<Vec<_>>(),
    );
    let snippet_hashes = StringArray::from(
        records
            .iter()
            .map(|r| r.snippet_hash.as_str())
            .collect::<Vec<_>>(),
    );
    let model_ids = StringArray::from(
        records
            .iter()
            .map(|r| r.embedding_model_id.as_str())
            .collect::<Vec<_>>(),
    );
    let model_versions = StringArray::from(
        records
            .iter()
            .map(|r| r.embedding_model_version.as_str())
            .collect::<Vec<_>>(),
    );
    let paths = StringArray::from(records.iter().map(|r| r.path.as_str()).collect::<Vec<_>>());
    let line_starts = UInt32Array::from(records.iter().map(|r| r.line_start).collect::<Vec<_>>());
    let line_ends = UInt32Array::from(records.iter().map(|r| r.line_end).collect::<Vec<_>>());
    let languages = StringArray::from(
        records
            .iter()
            .map(|r| r.language.as_str())
            .collect::<Vec<_>>(),
    );
    let chunk_types = StringArray::from(
        records
            .iter()
            .map(|r| r.chunk_type.as_deref())
            .collect::<Vec<Option<&str>>>(),
    );
    let snippet_texts = StringArray::from(
        records
            .iter()
            .map(|r| r.snippet_text.as_str())
            .collect::<Vec<_>>(),
    );

    // Validate all vectors match the expected dimension.
    for (i, r) in records.iter().enumerate() {
        if r.vector.len() != dim_usize {
            return Err(StateError::external(format!(
                "lancedb records_to_batch: vector at index {i} has dimension {} but expected {dim_usize}",
                r.vector.len()
            )));
        }
    }
    let flat_values: Vec<f32> = records
        .iter()
        .flat_map(|r| r.vector.iter().copied())
        .collect();
    let values = Float32Array::from(flat_values);
    let inner_field = Arc::new(Field::new("item", DataType::Float32, true));
    let vectors = FixedSizeListArray::new(inner_field, dim, Arc::new(values), None);

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(project_ids) as ArrayRef,
            Arc::new(ref_names),
            Arc::new(symbol_stable_ids),
            Arc::new(snippet_hashes),
            Arc::new(model_ids),
            Arc::new(model_versions),
            Arc::new(paths),
            Arc::new(line_starts),
            Arc::new(line_ends),
            Arc::new(languages),
            Arc::new(chunk_types),
            Arc::new(snippet_texts),
            Arc::new(vectors),
        ],
    )
    .map_err(|e| StateError::external(format!("arrow RecordBatch: {e}")))
}

// ── Public API ──────────────────────────────────────────────────────────

pub(super) fn ensure_schema(conn: &Connection) -> Result<(), StateError> {
    // Reuse the canonical DDL (includes meta + SQLite vector tables).
    // The SQLite `semantic_vectors` table is harmless to create even when
    // LanceDB is the active backend — it simply stays empty.
    conn.execute_batch(super::SEMANTIC_VECTOR_DDL)
        .map_err(StateError::sqlite)?;

    // Ensure LanceDB directory exists.
    let dir = lancedb_dir(conn);
    std::fs::create_dir_all(&dir).map_err(StateError::Io)?;

    Ok(())
}

pub(super) fn schema_version(conn: &Connection) -> Result<i64, StateError> {
    ensure_schema(conn)?;
    conn.query_row(
        "SELECT CAST(meta_value AS INTEGER)
         FROM semantic_vector_meta
         WHERE meta_key = 'vector_schema_version'",
        [],
        |row| row.get(0),
    )
    .map_err(StateError::sqlite)
}

pub(super) fn upsert_vectors(
    conn: &Connection,
    vectors: &[VectorRecord],
) -> Result<usize, StateError> {
    if vectors.is_empty() {
        return Ok(0);
    }
    ensure_schema(conn)?;

    let valid: Vec<&VectorRecord> = vectors.iter().filter(|v| !v.vector.is_empty()).collect();
    if valid.is_empty() {
        return Ok(0);
    }

    // All records in a single batch must share the same dimension.
    let dim = valid[0].embedding_dimensions;
    let mismatched: Vec<_> = valid
        .iter()
        .filter(|v| v.embedding_dimensions != dim)
        .collect();
    if !mismatched.is_empty() {
        return Err(StateError::external(format!(
            "lancedb upsert: dimension mismatch — expected {dim} but {} records have different dimensions",
            mismatched.len()
        )));
    }

    let consistent: Vec<VectorRecord> = valid.into_iter().cloned().collect();
    let count = consistent.len();

    let batch = records_to_batch(&consistent, dim as i32)?;
    let schema = batch.schema();
    let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);

    block_on(async {
        let lance = open_lance(conn).await?;
        let table = open_or_create_table(&lance, dim).await?;
        let mut merge = table.merge_insert(&[
            "project_id",
            "ref_name",
            "symbol_stable_id",
            "snippet_hash",
            "embedding_model_version",
        ]);
        merge
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge
            .execute(Box::new(reader))
            .await
            .map_err(|e| StateError::external(format!("lancedb merge_insert: {e}")))?;

        // Best-effort ANN index creation after upsert.
        maybe_create_ann_index(&table).await;

        Ok::<(), StateError>(())
    })??;

    Ok(count)
}

pub(super) fn delete_vectors_for_symbol(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_id: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let predicate = format!(
        "project_id = '{}' AND ref_name = '{}' AND symbol_stable_id = '{}'",
        escape_filter_value(project_id),
        escape_filter_value(ref_name),
        escape_filter_value(symbol_stable_id),
    );
    delete_with_predicate(conn, &predicate)
}

pub(super) fn delete_vectors_for_symbols(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_ids: &[String],
) -> Result<usize, StateError> {
    if symbol_stable_ids.is_empty() {
        return Ok(0);
    }
    ensure_schema(conn)?;

    let escaped_project_id = escape_filter_value(project_id);
    let escaped_ref_name = escape_filter_value(ref_name);
    let mut predicates = Vec::new();
    const CHUNK_SIZE: usize = 256;

    for chunk in symbol_stable_ids.chunks(CHUNK_SIZE) {
        let ids_list = chunk
            .iter()
            .map(|id| format!("'{}'", escape_filter_value(id)))
            .collect::<Vec<_>>()
            .join(", ");
        predicates.push(format!(
            "project_id = '{}' AND ref_name = '{}' AND symbol_stable_id IN ({ids_list})",
            escaped_project_id, escaped_ref_name,
        ));
    }

    delete_with_predicates(conn, &predicates)
}

pub(super) fn delete_vectors_for_path(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let predicate = format!(
        "project_id = '{}' AND ref_name = '{}' AND path = '{}'",
        escape_filter_value(project_id),
        escape_filter_value(ref_name),
        escape_filter_value(path),
    );
    delete_with_predicate(conn, &predicate)
}

pub(super) fn delete_vectors_for_ref(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let predicate = format!(
        "project_id = '{}' AND ref_name = '{}'",
        escape_filter_value(project_id),
        escape_filter_value(ref_name),
    );
    delete_with_predicate(conn, &predicate)
}

pub(super) fn query_nearest(
    conn: &Connection,
    query: &VectorQuery,
) -> Result<Vec<VectorMatch>, StateError> {
    ensure_schema(conn)?;
    if query.query_vector.is_empty() || query.limit == 0 || query.project_id.trim().is_empty() {
        return Ok(Vec::new());
    }

    let dim = query.query_vector.len();
    tracing::debug!(
        dim,
        limit = query.limit,
        "lancedb: query_nearest searching table"
    );
    let filter = format!(
        "project_id = '{}' AND ref_name = '{}' AND embedding_model_version = '{}'",
        escape_filter_value(&query.project_id),
        escape_filter_value(&query.ref_name),
        escape_filter_value(&query.embedding_model_version),
    );

    block_on(async {
        let lance = open_lance(conn).await?;
        let table = match lance.open_table(table_name(dim)).execute().await {
            Ok(t) => t,
            // Table not yet created → no vectors to return.
            Err(lancedb::Error::TableNotFound { .. }) => return Ok(Vec::new()),
            Err(e) => {
                return Err(StateError::external(format!("lancedb open_table: {e}")));
            }
        };

        let results = table
            .vector_search(query.query_vector.as_slice())
            .map_err(|e| StateError::external(format!("lancedb vector_search: {e}")))?
            .distance_type(DistanceType::Cosine)
            .limit(query.limit)
            .only_if(&filter)
            .select(Select::columns(&[
                "symbol_stable_id",
                "path",
                "line_start",
                "line_end",
                "language",
                "chunk_type",
                "snippet_text",
            ]))
            .execute()
            .await
            .map_err(|e| StateError::external(format!("lancedb execute: {e}")))?
            .try_collect::<Vec<RecordBatch>>()
            .await
            .map_err(|e| StateError::external(format!("lancedb collect: {e}")))?;

        let mut matches = Vec::new();
        for batch in &results {
            parse_vector_matches(batch, &mut matches);
        }
        // Sort by score descending for stable ordering across batches.
        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(matches)
    })?
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn delete_with_predicate(conn: &Connection, predicate: &str) -> Result<usize, StateError> {
    delete_with_predicates(conn, &[predicate.to_string()])
}

fn delete_with_predicates(conn: &Connection, predicates: &[String]) -> Result<usize, StateError> {
    if predicates.is_empty() {
        return Ok(0);
    }

    block_on(async {
        let lance = open_lance(conn).await?;
        let tables = lance
            .table_names()
            .execute()
            .await
            .map_err(|e| StateError::external(format!("lancedb table_names: {e}")))?;

        let mut total = 0usize;
        for name in tables {
            if !name.starts_with("semantic_vectors_") {
                continue;
            }
            let table =
                lance.open_table(&name).execute().await.map_err(|e| {
                    StateError::external(format!("lancedb open_table({}): {e}", name))
                })?;
            let before = table
                .count_rows(None)
                .await
                .map_err(|e| StateError::external(format!("lancedb count_rows({}): {e}", name)))?;
            for predicate in predicates {
                table
                    .delete(predicate)
                    .await
                    .map_err(|e| StateError::external(format!("lancedb delete({}): {e}", name)))?;
            }
            let after = table
                .count_rows(None)
                .await
                .map_err(|e| StateError::external(format!("lancedb count_rows({}): {e}", name)))?;
            total += before.saturating_sub(after);
        }
        Ok(total)
    })?
}

fn parse_vector_matches(batch: &RecordBatch, out: &mut Vec<VectorMatch>) {
    let n = batch.num_rows();
    let symbol_ids = batch
        .column_by_name("symbol_stable_id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let paths = batch
        .column_by_name("path")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let line_starts = batch
        .column_by_name("line_start")
        .and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
    let line_ends = batch
        .column_by_name("line_end")
        .and_then(|c| c.as_any().downcast_ref::<UInt32Array>());
    let languages = batch
        .column_by_name("language")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let chunk_types = batch
        .column_by_name("chunk_type")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let snippets = batch
        .column_by_name("snippet_text")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let distances = batch
        .column_by_name("_distance")
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let (
        Some(symbol_ids),
        Some(paths),
        Some(line_starts),
        Some(line_ends),
        Some(languages),
        Some(snippets),
        Some(distances),
    ) = (
        symbol_ids,
        paths,
        line_starts,
        line_ends,
        languages,
        snippets,
        distances,
    )
    else {
        return;
    };

    for i in 0..n {
        let distance = distances.value(i) as f64;
        // Cosine distance = 1 − cosine_similarity  →  score = 1.0 − distance
        let score = 1.0 - distance;
        out.push(VectorMatch {
            symbol_stable_id: symbol_ids.value(i).to_string(),
            path: paths.value(i).to_string(),
            line_start: line_starts.value(i),
            line_end: line_ends.value(i),
            language: languages.value(i).to_string(),
            chunk_type: chunk_types.and_then(|ct| {
                if ct.is_null(i) {
                    None
                } else {
                    Some(ct.value(i).to_string())
                }
            }),
            snippet_text: snippets.value(i).to_string(),
            score,
        });
    }
}

/// Escape a string value for use in LanceDB filter predicates.
///
/// LanceDB filter syntax is SQL-like: string literals are single-quoted,
/// single quotes are doubled, and backslashes must be escaped.
fn escape_filter_value(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}
