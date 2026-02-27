use cruxe_core::error::StateError;
use rusqlite::{Connection, params};
use rusqlite::{params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::warn;

#[cfg(feature = "lancedb")]
mod lancedb_backend;

pub const VECTOR_SCHEMA_VERSION: i64 = 1;

/// Canonical DDL for the semantic vector tables (SQLite).
///
/// Re-used by both `ensure_schema()` (runtime) and `schema.rs` (baseline + migration)
/// so the schema is defined in exactly one place.
pub const SEMANTIC_VECTOR_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS semantic_vector_meta (
    meta_key TEXT PRIMARY KEY,
    meta_value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO semantic_vector_meta(meta_key, meta_value)
VALUES ('vector_schema_version', '1')
ON CONFLICT(meta_key) DO UPDATE SET
    meta_value = excluded.meta_value,
    updated_at = datetime('now');
CREATE TABLE IF NOT EXISTS semantic_vectors (
    project_id TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    symbol_stable_id TEXT NOT NULL,
    snippet_hash TEXT NOT NULL,
    embedding_model_id TEXT NOT NULL,
    embedding_model_version TEXT NOT NULL,
    embedding_dimensions INTEGER NOT NULL,
    path TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    language TEXT NOT NULL,
    chunk_type TEXT,
    snippet_text TEXT NOT NULL,
    vector_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (project_id, "ref", symbol_stable_id, snippet_hash, embedding_model_version)
);
CREATE INDEX IF NOT EXISTS idx_semantic_vectors_query
    ON semantic_vectors(project_id, "ref", embedding_model_version);
CREATE INDEX IF NOT EXISTS idx_semantic_vectors_symbol_ref
    ON semantic_vectors(project_id, "ref", symbol_stable_id);
CREATE INDEX IF NOT EXISTS idx_semantic_vectors_path_ref
    ON semantic_vectors(project_id, "ref", path);
"#;

const MAX_VECTOR_QUERY_CACHE_ENTRIES: usize = 32;
const TOP_K_SNIPPET_FETCH_CHUNK: usize = 256;
const VECTOR_SCHEMA_CACHE_LIMIT: usize = 128;
static JSON_VECTOR_FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VectorCacheKey {
    namespace: String,
    project_id: String,
    ref_name: String,
    embedding_model_version: String,
}

#[derive(Debug, Clone)]
struct CachedVectorRow {
    row_id: i64,
    symbol_stable_id: String,
    path: String,
    line_start: u32,
    line_end: u32,
    language: String,
    chunk_type: Option<String>,
    vector: Vec<f32>,
    vector_norm: f32,
}

#[derive(Debug, Clone)]
struct ScoredVectorRow {
    row_id: i64,
    symbol_stable_id: String,
    path: String,
    line_start: u32,
    line_end: u32,
    language: String,
    chunk_type: Option<String>,
    score: f64,
}

type VectorQueryCache = HashMap<VectorCacheKey, Arc<Vec<CachedVectorRow>>>;
static VECTOR_QUERY_CACHE: OnceLock<Mutex<VectorQueryCache>> = OnceLock::new();
static VECTOR_SCHEMA_CACHE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn vector_query_cache() -> &'static Mutex<VectorQueryCache> {
    VECTOR_QUERY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn schema_cache() -> &'static Mutex<HashSet<String>> {
    VECTOR_SCHEMA_CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorBackend {
    Sqlite,
    LanceDb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendResolution {
    pub backend: VectorBackend,
    pub adapter_unavailable: bool,
}

pub fn resolve_backend(requested: Option<&str>) -> BackendResolution {
    match requested
        .unwrap_or("sqlite")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "lancedb" => {
            #[cfg(feature = "lancedb")]
            {
                BackendResolution {
                    backend: VectorBackend::LanceDb,
                    adapter_unavailable: false,
                }
            }
            #[cfg(not(feature = "lancedb"))]
            {
                warn!(
                    "lancedb backend requested but `lancedb` feature is not enabled; falling back to sqlite"
                );
                BackendResolution {
                    backend: VectorBackend::Sqlite,
                    adapter_unavailable: true,
                }
            }
        }
        _ => BackendResolution {
            backend: VectorBackend::Sqlite,
            adapter_unavailable: false,
        },
    }
}

pub fn ensure_schema_with_backend(
    conn: &Connection,
    requested_backend: Option<&str>,
) -> Result<(), StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => ensure_schema(conn),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::ensure_schema(conn)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn schema_version_with_backend(
    conn: &Connection,
    requested_backend: Option<&str>,
) -> Result<i64, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => schema_version(conn),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::schema_version(conn)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn upsert_vectors_with_backend(
    conn: &Connection,
    vectors: &[VectorRecord],
    requested_backend: Option<&str>,
) -> Result<usize, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => upsert_vectors(conn, vectors),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::upsert_vectors(conn, vectors)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn delete_vectors_for_symbol_with_backend(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_id: &str,
    requested_backend: Option<&str>,
) -> Result<usize, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => {
            delete_vectors_for_symbol(conn, project_id, ref_name, symbol_stable_id)
        }
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::delete_vectors_for_symbol(
                    conn,
                    project_id,
                    ref_name,
                    symbol_stable_id,
                )
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn delete_vectors_for_symbols_with_backend(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_ids: &[String],
    requested_backend: Option<&str>,
) -> Result<usize, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => {
            delete_vectors_for_symbols(conn, project_id, ref_name, symbol_stable_ids)
        }
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::delete_vectors_for_symbols(
                    conn,
                    project_id,
                    ref_name,
                    symbol_stable_ids,
                )
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn delete_vectors_for_path_with_backend(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
    requested_backend: Option<&str>,
) -> Result<usize, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => delete_vectors_for_path(conn, project_id, ref_name, path),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::delete_vectors_for_path(conn, project_id, ref_name, path)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn delete_vectors_for_ref_with_backend(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    requested_backend: Option<&str>,
) -> Result<usize, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => delete_vectors_for_ref(conn, project_id, ref_name),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::delete_vectors_for_ref(conn, project_id, ref_name)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

pub fn query_nearest_with_backend(
    conn: &Connection,
    query: &VectorQuery,
    requested_backend: Option<&str>,
) -> Result<Vec<VectorMatch>, StateError> {
    match resolve_backend(requested_backend).backend {
        VectorBackend::Sqlite => query_nearest(conn, query),
        VectorBackend::LanceDb => {
            #[cfg(feature = "lancedb")]
            {
                lancedb_backend::query_nearest(conn, query)
            }
            #[cfg(not(feature = "lancedb"))]
            {
                unreachable!("resolve_backend should not return lancedb when feature is disabled");
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorRecord {
    pub project_id: String,
    pub ref_name: String,
    pub symbol_stable_id: String,
    pub snippet_hash: String,
    pub embedding_model_id: String,
    pub embedding_model_version: String,
    pub embedding_dimensions: usize,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub language: String,
    pub chunk_type: Option<String>,
    pub snippet_text: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct VectorQuery {
    pub project_id: String,
    pub ref_name: String,
    pub embedding_model_version: String,
    pub query_vector: Vec<f32>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMatch {
    pub symbol_stable_id: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub language: String,
    pub chunk_type: Option<String>,
    pub snippet_text: String,
    pub score: f64,
}

pub fn ensure_schema(conn: &Connection) -> Result<(), StateError> {
    let namespace = cache_namespace(conn);
    if let Ok(cache) = schema_cache().lock()
        && cache.contains(&namespace)
    {
        return Ok(());
    }

    conn.execute_batch(SEMANTIC_VECTOR_DDL)
        .map_err(StateError::sqlite)?;

    if let Ok(mut cache) = schema_cache().lock() {
        if cache.len() >= VECTOR_SCHEMA_CACHE_LIMIT
            && let Some(evict_key) = cache.iter().next().cloned()
        {
            cache.remove(&evict_key);
        }
        cache.insert(namespace);
    }

    Ok(())
}

pub fn schema_version(conn: &Connection) -> Result<i64, StateError> {
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

pub fn upsert_vectors(conn: &Connection, vectors: &[VectorRecord]) -> Result<usize, StateError> {
    if vectors.is_empty() {
        return Ok(0);
    }
    ensure_schema(conn)?;
    conn.execute_batch("SAVEPOINT semantic_vectors_upsert_batch")
        .map_err(StateError::sqlite)?;
    type BulkUpsertResult = (usize, HashSet<(String, String, String)>);
    let result = (|| -> Result<BulkUpsertResult, StateError> {
        let mut stmt = conn
            .prepare(
                "INSERT INTO semantic_vectors (
                    project_id,
                    \"ref\",
                    symbol_stable_id,
                    snippet_hash,
                    embedding_model_id,
                    embedding_model_version,
                    embedding_dimensions,
                    path,
                    line_start,
                    line_end,
                    language,
                    chunk_type,
                    snippet_text,
                    vector_json,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
                ON CONFLICT(project_id, \"ref\", symbol_stable_id, snippet_hash, embedding_model_version)
                DO UPDATE SET
                    embedding_model_id = excluded.embedding_model_id,
                    embedding_dimensions = excluded.embedding_dimensions,
                    path = excluded.path,
                    line_start = excluded.line_start,
                    line_end = excluded.line_end,
                    language = excluded.language,
                    chunk_type = excluded.chunk_type,
                    snippet_text = excluded.snippet_text,
                    vector_json = excluded.vector_json,
                    updated_at = datetime('now')",
            )
            .map_err(StateError::sqlite)?;

        let mut written = 0usize;
        let mut touched_scopes = HashSet::new();
        for record in vectors {
            if record.vector.is_empty() {
                continue;
            }
            let vector_blob = encode_vector_blob(&record.vector);
            stmt.execute(params![
                record.project_id,
                record.ref_name,
                record.symbol_stable_id,
                record.snippet_hash,
                record.embedding_model_id,
                record.embedding_model_version,
                record.embedding_dimensions as i64,
                record.path,
                record.line_start as i64,
                record.line_end as i64,
                record.language,
                record.chunk_type,
                record.snippet_text,
                vector_blob
            ])
            .map_err(StateError::sqlite)?;
            written += 1;
            touched_scopes.insert((
                record.project_id.clone(),
                record.ref_name.clone(),
                record.embedding_model_version.clone(),
            ));
        }
        Ok((written, touched_scopes))
    })();

    match result {
        Ok((written, touched_scopes)) => {
            conn.execute_batch("RELEASE SAVEPOINT semantic_vectors_upsert_batch")
                .map_err(StateError::sqlite)?;
            for (project_id, ref_name, model_version) in touched_scopes {
                invalidate_scope_cache(conn, &project_id, &ref_name, Some(&model_version));
            }
            Ok(written)
        }
        Err(err) => {
            let _ = conn.execute_batch(
                "ROLLBACK TO SAVEPOINT semantic_vectors_upsert_batch;
                 RELEASE SAVEPOINT semantic_vectors_upsert_batch;",
            );
            Err(err)
        }
    }
}

pub fn delete_vectors_for_symbol(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_id: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let deleted = conn
        .execute(
            "DELETE FROM semantic_vectors
         WHERE project_id = ?1 AND \"ref\" = ?2 AND symbol_stable_id = ?3",
            params![project_id, ref_name, symbol_stable_id],
        )
        .map_err(StateError::sqlite)?;
    if deleted > 0 {
        invalidate_scope_cache(conn, project_id, ref_name, None);
    }
    Ok(deleted)
}

pub fn delete_vectors_for_symbols(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    symbol_stable_ids: &[String],
) -> Result<usize, StateError> {
    if symbol_stable_ids.is_empty() {
        return Ok(0);
    }
    ensure_schema(conn)?;

    let mut deleted = 0usize;
    const CHUNK_SIZE: usize = 256;
    for chunk in symbol_stable_ids.chunks(CHUNK_SIZE) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "DELETE FROM semantic_vectors
             WHERE project_id = ?1 AND \"ref\" = ?2
               AND symbol_stable_id IN ({placeholders})"
        );

        let mut params = Vec::with_capacity(2 + chunk.len());
        params.push(Value::from(project_id.to_string()));
        params.push(Value::from(ref_name.to_string()));
        params.extend(chunk.iter().cloned().map(Value::from));

        deleted += conn
            .execute(&sql, params_from_iter(params))
            .map_err(StateError::sqlite)?;
    }
    if deleted > 0 {
        invalidate_scope_cache(conn, project_id, ref_name, None);
    }
    Ok(deleted)
}

pub fn delete_vectors_for_path(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    path: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let deleted = conn
        .execute(
            "DELETE FROM semantic_vectors
         WHERE project_id = ?1 AND \"ref\" = ?2 AND path = ?3",
            params![project_id, ref_name, path],
        )
        .map_err(StateError::sqlite)?;
    if deleted > 0 {
        invalidate_scope_cache(conn, project_id, ref_name, None);
    }
    Ok(deleted)
}

pub fn delete_vectors_for_ref(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<usize, StateError> {
    ensure_schema(conn)?;
    let deleted = conn
        .execute(
            "DELETE FROM semantic_vectors
         WHERE project_id = ?1 AND \"ref\" = ?2",
            params![project_id, ref_name],
        )
        .map_err(StateError::sqlite)?;
    if deleted > 0 {
        invalidate_scope_cache(conn, project_id, ref_name, None);
    }
    Ok(deleted)
}

/// SQLite brute-force cosine similarity search.
///
/// **Scaling note:** This backend loads all matching vectors into memory for
/// pairwise cosine comparison. This is efficient for repos with fewer than
/// ~50k vectors but will degrade for larger corpora. Use the `lancedb` backend
/// (feature-gated) for ANN search at scale.
pub fn query_nearest(
    conn: &Connection,
    query: &VectorQuery,
) -> Result<Vec<VectorMatch>, StateError> {
    ensure_schema(conn)?;
    if query.query_vector.is_empty() || query.limit == 0 {
        return Ok(Vec::new());
    }

    // Enforce project scoping for semantic retrieval to avoid cross-project
    // scans when callers fail to resolve an explicit project id.
    if query.project_id.trim().is_empty() {
        return Ok(Vec::new());
    }

    let cached_rows = load_cached_rows(conn, query)?;
    let query_norm = vector_l2_norm(&query.query_vector);
    if query_norm <= f32::EPSILON {
        return Ok(Vec::new());
    }

    let mut scored = Vec::new();
    for row in cached_rows.iter() {
        if row.vector.len() != query.query_vector.len() {
            continue;
        }
        let similarity = cosine_similarity(
            &query.query_vector,
            query_norm,
            &row.vector,
            row.vector_norm,
        );
        scored.push(ScoredVectorRow {
            row_id: row.row_id,
            symbol_stable_id: row.symbol_stable_id.clone(),
            path: row.path.clone(),
            line_start: row.line_start,
            line_end: row.line_end,
            language: row.language.clone(),
            chunk_type: row.chunk_type.clone(),
            score: similarity,
        });
    }

    scored.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.symbol_stable_id.cmp(&right.symbol_stable_id))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line_start.cmp(&right.line_start))
    });
    scored.truncate(query.limit);

    if scored.is_empty() {
        return Ok(Vec::new());
    }

    let row_ids: Vec<i64> = scored.iter().map(|row| row.row_id).collect();
    let snippet_texts = load_snippet_text_by_row_ids(conn, &row_ids)?;

    Ok(scored
        .into_iter()
        .map(|row| VectorMatch {
            symbol_stable_id: row.symbol_stable_id,
            path: row.path,
            line_start: row.line_start,
            line_end: row.line_end,
            language: row.language,
            chunk_type: row.chunk_type,
            snippet_text: snippet_texts.get(&row.row_id).cloned().unwrap_or_default(),
            score: row.score,
        })
        .collect())
}

fn cache_namespace(conn: &Connection) -> String {
    match conn.path() {
        Some(path) => path.to_string(),
        None => format!(":memory:{:p}", conn),
    }
}

fn make_cache_key(
    namespace: String,
    project_id: &str,
    ref_name: &str,
    embedding_model_version: &str,
) -> VectorCacheKey {
    VectorCacheKey {
        namespace,
        project_id: project_id.to_string(),
        ref_name: ref_name.to_string(),
        embedding_model_version: embedding_model_version.to_string(),
    }
}

fn load_cached_rows(
    conn: &Connection,
    query: &VectorQuery,
) -> Result<Arc<Vec<CachedVectorRow>>, StateError> {
    let namespace = cache_namespace(conn);
    let cache_key = make_cache_key(
        namespace,
        &query.project_id,
        &query.ref_name,
        &query.embedding_model_version,
    );

    if let Ok(cache) = vector_query_cache().lock()
        && let Some(rows) = cache.get(&cache_key)
    {
        return Ok(rows.clone());
    }

    let rows = load_rows_from_db(conn, query)?;
    let shared_rows = Arc::new(rows);

    if let Ok(mut cache) = vector_query_cache().lock() {
        if cache.len() >= MAX_VECTOR_QUERY_CACHE_ENTRIES
            && let Some(evict_key) = cache.keys().next().cloned()
        {
            cache.remove(&evict_key);
        }
        cache.insert(cache_key, shared_rows.clone());
    }

    Ok(shared_rows)
}

fn load_rows_from_db(
    conn: &Connection,
    query: &VectorQuery,
) -> Result<Vec<CachedVectorRow>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT
                rowid,
                symbol_stable_id,
                path,
                line_start,
                line_end,
                language,
                chunk_type,
                vector_json
             FROM semantic_vectors
             WHERE project_id = ?1
               AND \"ref\" = ?2
               AND embedding_model_version = ?3",
        )
        .map_err(StateError::sqlite)?;
    let rows = stmt
        .query_map(
            params![
                query.project_id,
                query.ref_name,
                query.embedding_model_version
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                ))
            },
        )
        .map_err(StateError::sqlite)?;

    let mut decoded_rows = Vec::new();
    for row in rows {
        let (
            row_id,
            symbol_stable_id,
            path,
            line_start,
            line_end,
            language,
            chunk_type,
            vector_blob,
        ) = row.map_err(StateError::sqlite)?;
        let vector = decode_vector_blob(&vector_blob)?;
        decoded_rows.push(CachedVectorRow {
            row_id,
            symbol_stable_id,
            path,
            line_start,
            line_end,
            language,
            chunk_type,
            vector_norm: vector_l2_norm(&vector),
            vector,
        });
    }

    Ok(decoded_rows)
}

fn load_snippet_text_by_row_ids(
    conn: &Connection,
    row_ids: &[i64],
) -> Result<HashMap<i64, String>, StateError> {
    if row_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut snippets = HashMap::with_capacity(row_ids.len());
    for chunk in row_ids.chunks(TOP_K_SNIPPET_FETCH_CHUNK) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!(
            "SELECT rowid, snippet_text
             FROM semantic_vectors
             WHERE rowid IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql).map_err(StateError::sqlite)?;
        let rows = stmt
            .query_map(
                params_from_iter(chunk.iter().copied().map(Value::from)),
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(StateError::sqlite)?;

        for row in rows {
            let (row_id, snippet_text) = row.map_err(StateError::sqlite)?;
            snippets.insert(row_id, snippet_text);
        }
    }

    Ok(snippets)
}

fn invalidate_scope_cache(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    model_version: Option<&str>,
) {
    let namespace = cache_namespace(conn);
    let Ok(mut cache) = vector_query_cache().lock() else {
        return;
    };
    cache.retain(|key, _| {
        if key.namespace != namespace || key.project_id != project_id || key.ref_name != ref_name {
            return true;
        }
        if let Some(version) = model_version {
            key.embedding_model_version != version
        } else {
            false
        }
    });
}

#[cfg(test)]
fn reset_internal_caches_for_tests() {
    if let Some(cache) = VECTOR_QUERY_CACHE.get()
        && let Ok(mut guard) = cache.lock()
    {
        guard.clear();
    }
    if let Some(cache) = VECTOR_SCHEMA_CACHE.get()
        && let Ok(mut guard) = cache.lock()
    {
        guard.clear();
    }
    JSON_VECTOR_FALLBACK_WARNED.store(false, AtomicOrdering::Relaxed);
}

fn vector_l2_norm(vector: &[f32]) -> f32 {
    if vector.is_empty() {
        return 0.0;
    }
    let sum_squares = vector
        .iter()
        .map(|value| {
            let value = *value as f64;
            value * value
        })
        .sum::<f64>();
    sum_squares.sqrt() as f32
}

fn cosine_similarity(left: &[f32], left_norm: f32, right: &[f32], right_norm: f32) -> f64 {
    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return 0.0;
    }

    let mut dot = 0.0_f64;
    for (l, r) in left.iter().zip(right.iter()) {
        let lf = *l as f64;
        let rf = *r as f64;
        dot += lf * rf;
    }
    let norm = left_norm as f64 * right_norm as f64;
    if norm == 0.0 { 0.0 } else { dot / norm }
}

const VECTOR_BLOB_MAGIC_F32: &[u8; 4] = b"F32\0";

fn encode_vector_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(VECTOR_BLOB_MAGIC_F32.len() + vector.len() * 4);
    bytes.extend_from_slice(VECTOR_BLOB_MAGIC_F32);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_vector_blob(bytes: &[u8]) -> Result<Vec<f32>, StateError> {
    if bytes.starts_with(VECTOR_BLOB_MAGIC_F32) {
        return decode_raw_f32_blob(&bytes[VECTOR_BLOB_MAGIC_F32.len()..]);
    }

    let first_non_ws = bytes.iter().copied().find(|b| !b.is_ascii_whitespace());
    if first_non_ws == Some(b'[')
        && let Ok(text) = std::str::from_utf8(bytes)
        && let Ok(parsed) = serde_json::from_str::<Vec<f32>>(text)
    {
        if JSON_VECTOR_FALLBACK_WARNED
            .compare_exchange(false, true, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
            .is_ok()
        {
            warn!("decoded legacy json semantic vector blob; consider re-embedding to migrate");
        }
        return Ok(parsed);
    }
    // Fall through: legacy raw binary blobs can start with '[' by chance.

    decode_raw_f32_blob(bytes)
}

fn decode_raw_f32_blob(bytes: &[u8]) -> Result<Vec<f32>, StateError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(StateError::sqlite(format!(
            "invalid_vector_blob_length:{}",
            bytes.len()
        )));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema};
    use tempfile::tempdir;

    fn setup_conn() -> Connection {
        reset_internal_caches_for_tests();
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn record(
        symbol_stable_id: &str,
        model_version: &str,
        snippet_hash: &str,
        vector: Vec<f32>,
    ) -> VectorRecord {
        VectorRecord {
            project_id: "proj".to_string(),
            ref_name: "main".to_string(),
            symbol_stable_id: symbol_stable_id.to_string(),
            snippet_hash: snippet_hash.to_string(),
            embedding_model_id: "NomicEmbedTextV15Q".to_string(),
            embedding_model_version: model_version.to_string(),
            embedding_dimensions: vector.len(),
            path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            language: "rust".to_string(),
            chunk_type: Some("function_body".to_string()),
            snippet_text: format!("fn {symbol_stable_id}() {{}}"),
            vector,
        }
    }

    #[test]
    fn insert_query_delete_by_stable_symbol_key() {
        let conn = setup_conn();
        let written = upsert_vectors(
            &conn,
            &[
                record("sym-a", "m1", "hash-a", vec![1.0, 0.0, 0.0]),
                record("sym-b", "m1", "hash-b", vec![0.0, 1.0, 0.0]),
            ],
        )
        .unwrap();
        assert_eq!(written, 2);

        let results = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![0.9, 0.1, 0.0],
                limit: 5,
            },
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].symbol_stable_id, "sym-a");

        let deleted = delete_vectors_for_symbol(&conn, "proj", "main", "sym-a").unwrap();
        assert_eq!(deleted, 1);
        let remaining = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0, 0.0],
                limit: 5,
            },
        )
        .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].symbol_stable_id, "sym-b");
    }

    #[test]
    fn bulk_delete_by_stable_symbol_keys() {
        let conn = setup_conn();
        upsert_vectors(
            &conn,
            &[
                record("sym-a", "m1", "hash-a", vec![1.0, 0.0]),
                record("sym-b", "m1", "hash-b", vec![0.0, 1.0]),
                record("sym-c", "m1", "hash-c", vec![0.5, 0.5]),
            ],
        )
        .unwrap();

        let deleted = delete_vectors_for_symbols(
            &conn,
            "proj",
            "main",
            &["sym-a".to_string(), "sym-c".to_string()],
        )
        .unwrap();
        assert_eq!(deleted, 2);

        let remaining = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0],
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].symbol_stable_id, "sym-b");
    }

    #[test]
    fn query_is_partitioned_by_model_version() {
        let conn = setup_conn();
        upsert_vectors(
            &conn,
            &[
                record("sym-a", "m1", "hash-a", vec![1.0, 0.0]),
                record("sym-b", "m2", "hash-b", vec![0.0, 1.0]),
            ],
        )
        .unwrap();

        let only_m1 = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0],
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(only_m1.len(), 1);
        assert_eq!(only_m1[0].symbol_stable_id, "sym-a");
    }

    #[test]
    fn schema_version_is_persisted() {
        let conn = setup_conn();
        assert_eq!(schema_version(&conn).unwrap(), VECTOR_SCHEMA_VERSION);
    }

    #[test]
    fn query_nearest_requires_project_scope() {
        let conn = setup_conn();
        upsert_vectors(
            &conn,
            &[record("sym-a", "m1", "hash-a", vec![1.0, 0.0, 0.0])],
        )
        .unwrap();

        let results = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0, 0.0],
                limit: 10,
            },
        )
        .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn query_cache_invalidates_after_ref_delete() {
        let conn = setup_conn();
        upsert_vectors(
            &conn,
            &[record("sym-a", "m1", "hash-a", vec![1.0, 0.0, 0.0])],
        )
        .unwrap();

        let first = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0, 0.0],
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(first.len(), 1);

        delete_vectors_for_ref(&conn, "proj", "main").unwrap();

        let second = query_nearest(
            &conn,
            &VectorQuery {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                embedding_model_version: "m1".to_string(),
                query_vector: vec![1.0, 0.0, 0.0],
                limit: 10,
            },
        )
        .unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn decode_vector_blob_supports_legacy_json_payloads() {
        let decoded = decode_vector_blob(br#"[1.0, -0.5, 0.25]"#).unwrap();
        assert_eq!(decoded, vec![1.0, -0.5, 0.25]);
    }

    #[test]
    fn decode_vector_blob_falls_back_to_binary_when_json_parse_fails() {
        // Legacy raw binary blobs can have a first byte equal to '['.
        let legacy_blob = vec![b'[', 0, 0, 0];
        let decoded = decode_vector_blob(&legacy_blob).unwrap();
        assert_eq!(decoded.len(), 1);
    }

    #[cfg(not(feature = "lancedb"))]
    #[test]
    fn optional_lancedb_backend_gracefully_falls_back() {
        let resolution = resolve_backend(Some("lancedb"));
        assert_eq!(resolution.backend, VectorBackend::Sqlite);
        assert!(resolution.adapter_unavailable);
    }

    #[cfg(feature = "lancedb")]
    mod lancedb_tests {
        use super::*;

        fn lancedb_record(
            symbol_stable_id: &str,
            model_version: &str,
            snippet_hash: &str,
            vector: Vec<f32>,
        ) -> VectorRecord {
            VectorRecord {
                project_id: "proj".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: symbol_stable_id.to_string(),
                snippet_hash: snippet_hash.to_string(),
                embedding_model_id: "test-model".to_string(),
                embedding_model_version: model_version.to_string(),
                embedding_dimensions: vector.len(),
                path: "src/lib.rs".to_string(),
                line_start: 10,
                line_end: 20,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: format!("fn {symbol_stable_id}() {{}}"),
                vector,
            }
        }

        #[test]
        fn lancedb_resolve_backend_returns_lancedb() {
            let resolution = resolve_backend(Some("lancedb"));
            assert_eq!(resolution.backend, VectorBackend::LanceDb);
            assert!(!resolution.adapter_unavailable);
        }

        #[test]
        fn lancedb_upsert_query_delete_roundtrip() {
            let conn = setup_conn();
            let backend = Some("lancedb");

            let written = upsert_vectors_with_backend(
                &conn,
                &[
                    lancedb_record("sym-a", "m1", "h-a", vec![1.0, 0.0, 0.0]),
                    lancedb_record("sym-b", "m1", "h-b", vec![0.0, 1.0, 0.0]),
                ],
                backend,
            )
            .unwrap();
            assert_eq!(written, 2);

            let results = query_nearest_with_backend(
                &conn,
                &VectorQuery {
                    project_id: "proj".to_string(),
                    ref_name: "main".to_string(),
                    embedding_model_version: "m1".to_string(),
                    query_vector: vec![0.9, 0.1, 0.0],
                    limit: 5,
                },
                backend,
            )
            .unwrap();
            assert_eq!(results.len(), 2);
            // Closest to [0.9, 0.1, 0.0] should be sym-a [1,0,0].
            assert_eq!(results[0].symbol_stable_id, "sym-a");

            let deleted =
                delete_vectors_for_symbol_with_backend(&conn, "proj", "main", "sym-a", backend)
                    .unwrap();
            assert_eq!(deleted, 1);

            let remaining = query_nearest_with_backend(
                &conn,
                &VectorQuery {
                    project_id: "proj".to_string(),
                    ref_name: "main".to_string(),
                    embedding_model_version: "m1".to_string(),
                    query_vector: vec![1.0, 0.0, 0.0],
                    limit: 5,
                },
                backend,
            )
            .unwrap();
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].symbol_stable_id, "sym-b");
        }

        #[test]
        fn lancedb_rejects_dimension_mismatch() {
            let conn = setup_conn();
            let backend = Some("lancedb");
            let result = upsert_vectors_with_backend(
                &conn,
                &[
                    lancedb_record("sym-a", "m1", "h-a", vec![1.0, 0.0, 0.0]),
                    lancedb_record("sym-b", "m1", "h-b", vec![0.0, 1.0]), // wrong dim
                ],
                backend,
            );
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("dimension mismatch"), "got: {err_msg}");
        }

        #[test]
        fn lancedb_results_sorted_by_score_descending() {
            let conn = setup_conn();
            let backend = Some("lancedb");

            upsert_vectors_with_backend(
                &conn,
                &[
                    lancedb_record("far", "m1", "h-far", vec![0.0, 0.0, 1.0]),
                    lancedb_record("close", "m1", "h-close", vec![1.0, 0.0, 0.0]),
                    lancedb_record("mid", "m1", "h-mid", vec![0.7, 0.3, 0.0]),
                ],
                backend,
            )
            .unwrap();

            let results = query_nearest_with_backend(
                &conn,
                &VectorQuery {
                    project_id: "proj".to_string(),
                    ref_name: "main".to_string(),
                    embedding_model_version: "m1".to_string(),
                    query_vector: vec![1.0, 0.0, 0.0],
                    limit: 10,
                },
                backend,
            )
            .unwrap();
            assert!(results.len() >= 2);
            // Verify descending score order.
            for w in results.windows(2) {
                assert!(
                    w[0].score >= w[1].score,
                    "results not sorted: {} < {}",
                    w[0].score,
                    w[1].score
                );
            }
        }

        #[test]
        fn lancedb_delete_vectors_for_symbols_handles_large_batches() {
            let conn = setup_conn();
            let backend = Some("lancedb");

            let records: Vec<VectorRecord> = (0..300)
                .map(|idx| {
                    lancedb_record(
                        &format!("sym-{idx}"),
                        "m1",
                        &format!("h-{idx}"),
                        vec![1.0, 0.0, 0.0],
                    )
                })
                .collect();
            upsert_vectors_with_backend(&conn, &records, backend).unwrap();

            let to_delete: Vec<String> = (0..290).map(|idx| format!("sym-{idx}")).collect();
            let deleted =
                delete_vectors_for_symbols_with_backend(&conn, "proj", "main", &to_delete, backend)
                    .unwrap();
            assert_eq!(deleted, to_delete.len());

            let remaining = query_nearest_with_backend(
                &conn,
                &VectorQuery {
                    project_id: "proj".to_string(),
                    ref_name: "main".to_string(),
                    embedding_model_version: "m1".to_string(),
                    query_vector: vec![1.0, 0.0, 0.0],
                    limit: 400,
                },
                backend,
            )
            .unwrap();
            assert_eq!(remaining.len(), 10);
        }
    }
}
