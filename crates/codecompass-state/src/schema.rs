use crate::vector_index::SEMANTIC_VECTOR_DDL;
use codecompass_core::error::StateError;
use rusqlite::Connection;
use tracing::info;

/// Current schema version. Bump this when adding a new migration step.
pub const CURRENT_SCHEMA_VERSION: u32 = 12;

/// Create all required SQLite tables per data-model.md and run any pending migrations.
pub fn create_tables(conn: &Connection) -> Result<(), StateError> {
    conn.execute_batch(SCHEMA_SQL).map_err(StateError::sqlite)?;
    // Semantic vector tables are defined once in vector_index::SEMANTIC_VECTOR_DDL
    // and applied here (baseline) as well as in the V11 migration path.
    conn.execute_batch(SEMANTIC_VECTOR_DDL)
        .map_err(StateError::sqlite)?;
    migrate(conn)?;
    info!("SQLite schema created (version {})", CURRENT_SCHEMA_VERSION);
    Ok(())
}

/// Run incremental schema migrations up to `CURRENT_SCHEMA_VERSION`.
///
/// The `schema_migrations` table tracks which version has been applied.
/// Each migration step is a function that runs the necessary DDL.
/// New migrations should be added to the `MIGRATIONS` array below.
pub fn migrate(conn: &Connection) -> Result<(), StateError> {
    // Ensure the migration tracking table exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(StateError::sqlite)?;

    let current: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;

    type MigrationFn = fn(&Connection) -> Result<(), StateError>;

    // Migration functions indexed by version (1-based: index 0 = V1, etc.)
    // V1 is the baseline schema created by SCHEMA_SQL above, so we just record it.
    let migrations: &[MigrationFn] = &[
        // V1: baseline — no DDL needed, tables already created by SCHEMA_SQL
        |_conn| Ok(()),
        // V2: add progress_token and progress fields to index_jobs.
        // Idempotent: columns already exist in the base DDL for fresh installs.
        |conn| {
            let has_column: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('index_jobs') WHERE name = 'progress_token'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_column {
                conn.execute_batch(
                    "ALTER TABLE index_jobs ADD COLUMN progress_token TEXT;
                     ALTER TABLE index_jobs ADD COLUMN files_scanned INTEGER DEFAULT 0;
                     ALTER TABLE index_jobs ADD COLUMN files_indexed INTEGER DEFAULT 0;
                     ALTER TABLE index_jobs ADD COLUMN symbols_extracted INTEGER DEFAULT 0;",
                )
                .map_err(StateError::sqlite)?;
            }
            Ok(())
        },
        // V3: add symbol_edges composite indexes for forward/reverse graph traversals.
        |conn| {
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_symbol_edges_from_type
                     ON symbol_edges(repo, \"ref\", from_symbol_id, edge_type);
                 CREATE INDEX IF NOT EXISTS idx_symbol_edges_to_type
                     ON symbol_edges(repo, \"ref\", to_symbol_id, edge_type);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V4: extend branch metadata and add worktree lease tracking.
        |conn| {
            let has_symbol_count: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('branch_state') WHERE name = 'symbol_count'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_symbol_count {
                conn.execute_batch(
                    "ALTER TABLE branch_state ADD COLUMN symbol_count INTEGER DEFAULT 0;
                     ALTER TABLE branch_state ADD COLUMN is_default_branch INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE branch_state ADD COLUMN status TEXT NOT NULL DEFAULT 'active';
                     ALTER TABLE branch_state ADD COLUMN eviction_eligible_at TEXT;",
                )
                .map_err(StateError::sqlite)?;
            }

            let has_tombstone_type: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('branch_tombstones') WHERE name = 'tombstone_type'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_tombstone_type {
                conn.execute_batch(
                    "ALTER TABLE branch_tombstones ADD COLUMN tombstone_type TEXT NOT NULL DEFAULT 'deleted';
                     ALTER TABLE branch_tombstones ADD COLUMN created_at TEXT NOT NULL DEFAULT (datetime('now'));",
                )
                .map_err(StateError::sqlite)?;
            }

            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS worktree_leases (
                    repo TEXT NOT NULL,
                    \"ref\" TEXT NOT NULL,
                    worktree_path TEXT NOT NULL,
                    owner_pid INTEGER NOT NULL,
                    refcount INTEGER NOT NULL DEFAULT 0,
                    status TEXT NOT NULL DEFAULT 'active',
                    created_at TEXT NOT NULL,
                    last_used_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(repo, \"ref\")
                );
                CREATE INDEX IF NOT EXISTS idx_worktree_leases_status_updated
                    ON worktree_leases(status, updated_at);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V5: enforce at most one active job per (project, ref) for race-safe sync locking.
        |conn| {
            conn.execute_batch(
                "UPDATE index_jobs
                 SET status = 'interrupted',
                     error_message = COALESCE(error_message, 'interrupted_by_active_job_uniqueness_migration'),
                     updated_at = datetime('now')
                 WHERE status IN ('queued', 'running', 'validating')
                   AND EXISTS (
                     SELECT 1
                     FROM index_jobs newer
                     WHERE newer.project_id = index_jobs.project_id
                       AND newer.\"ref\" = index_jobs.\"ref\"
                       AND newer.status IN ('queued', 'running', 'validating')
                       AND (
                         newer.created_at > index_jobs.created_at
                         OR (newer.created_at = index_jobs.created_at AND newer.rowid > index_jobs.rowid)
                       )
                   );

                 CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_active_project_ref
                     ON index_jobs(project_id, \"ref\")
                     WHERE status IN ('queued', 'running', 'validating');",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V6: add eviction candidate lookup index for branch_state lifecycle operations.
        |conn| {
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_branch_state_eviction
                     ON branch_state(repo, status, last_accessed_at);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V7: align worktree lease lifecycle columns with spec contract.
        |conn| {
            let has_last_used_at: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('worktree_leases') WHERE name = 'last_used_at'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_last_used_at {
                conn.execute_batch("ALTER TABLE worktree_leases ADD COLUMN last_used_at TEXT;")
                    .map_err(StateError::sqlite)?;
            }
            conn.execute_batch(
                "UPDATE worktree_leases
                 SET status = CASE status
                                WHEN 'in_use' THEN 'active'
                                WHEN 'released' THEN 'stale'
                                ELSE status
                              END;
                 UPDATE worktree_leases
                 SET last_used_at = COALESCE(last_used_at, updated_at, created_at, datetime('now'));
                 CREATE INDEX IF NOT EXISTS idx_worktree_leases_status_last_used
                    ON worktree_leases(status, last_used_at);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V8: normalize worktree lease lifecycle states and align index naming with spec.
        |conn| {
            conn.execute_batch(
                "UPDATE worktree_leases
                 SET status = CASE status
                                WHEN 'active' THEN 'active'
                                WHEN 'stale' THEN 'stale'
                                WHEN 'removing' THEN 'removing'
                                ELSE 'stale'
                              END;
                 UPDATE worktree_leases
                 SET last_used_at = COALESCE(last_used_at, updated_at, created_at, datetime('now'));
                 CREATE INDEX IF NOT EXISTS idx_worktree_leases_status
                    ON worktree_leases(status, last_used_at);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V9: extend symbol_edges for call-graph payloads (nullable callee, call-site metadata).
        |conn| {
            let has_to_name: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('symbol_edges') WHERE name = 'to_name'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_to_name {
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS symbol_edges_v9 (
                        repo TEXT NOT NULL,
                        \"ref\" TEXT NOT NULL,
                        from_symbol_id TEXT NOT NULL,
                        to_symbol_id TEXT,
                        to_name TEXT,
                        edge_type TEXT NOT NULL,
                        confidence TEXT DEFAULT 'static',
                        source_file TEXT,
                        source_line INTEGER,
                        CHECK (to_symbol_id IS NOT NULL OR to_name IS NOT NULL)
                    );
                    INSERT INTO symbol_edges_v9
                        (repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence)
                    SELECT repo, \"ref\", from_symbol_id, to_symbol_id, edge_type, confidence
                    FROM symbol_edges;
                    DROP TABLE symbol_edges;
                    ALTER TABLE symbol_edges_v9 RENAME TO symbol_edges;",
                )
                .map_err(StateError::sqlite)?;
            }
            conn.execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_symbol_edges_unique
                    ON symbol_edges(
                        repo,
                        \"ref\",
                        from_symbol_id,
                        edge_type,
                        COALESCE(to_symbol_id, ''),
                        COALESCE(to_name, ''),
                        COALESCE(source_file, ''),
                        COALESCE(source_line, -1)
                    );
                 CREATE INDEX IF NOT EXISTS idx_symbol_edges_to
                    ON symbol_edges(repo, \"ref\", to_symbol_id);
                 CREATE INDEX IF NOT EXISTS idx_symbol_edges_from_type
                    ON symbol_edges(repo, \"ref\", from_symbol_id, edge_type);
                 CREATE INDEX IF NOT EXISTS idx_symbol_edges_to_type
                    ON symbol_edges(repo, \"ref\", to_symbol_id, edge_type);
                 CREATE INDEX IF NOT EXISTS idx_symbol_edges_source_file
                    ON symbol_edges(repo, \"ref\", source_file, edge_type);",
            )
            .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V10: persist symbol content snapshots for cross-ref body diff accuracy.
        |conn| {
            let has_content: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('symbol_relations') WHERE name = 'content'",
                    [],
                    |row| row.get(0),
                )
                .map_err(StateError::sqlite)?;
            if !has_content {
                conn.execute_batch("ALTER TABLE symbol_relations ADD COLUMN content TEXT;")
                    .map_err(StateError::sqlite)?;
            }
            Ok(())
        },
        // V11: semantic vector store metadata + embedded vector rows (hybrid mode).
        // DDL is shared with vector_index::ensure_schema via the canonical constant.
        |conn| {
            conn.execute_batch(SEMANTIC_VECTOR_DDL)
                .map_err(StateError::sqlite)?;
            Ok(())
        },
        // V12: remove duplicate worktree_leases index.
        // `idx_worktree_leases_status` duplicates `idx_worktree_leases_status_last_used`
        // (both cover the same (status, last_used_at) columns).
        |conn| {
            conn.execute_batch("DROP INDEX IF EXISTS idx_worktree_leases_status_last_used;")
                .map_err(StateError::sqlite)?;
            Ok(())
        },
    ];

    for version in (current + 1)..=(CURRENT_SCHEMA_VERSION) {
        let idx = (version - 1) as usize;
        if idx < migrations.len() {
            migrations[idx](conn)?;
        }
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [version],
        )
        .map_err(StateError::sqlite)?;
        info!(version, "Applied schema migration");
    }

    Ok(())
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY,
    repo_root TEXT NOT NULL UNIQUE,
    display_name TEXT,
    default_ref TEXT DEFAULT 'main',
    vcs_mode INTEGER NOT NULL DEFAULT 1,
    schema_version INTEGER NOT NULL DEFAULT 1,
    parser_version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_manifest (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    mtime_ns INTEGER,
    language TEXT,
    indexed_at TEXT NOT NULL,
    PRIMARY KEY(repo, "ref", path)
);

CREATE TABLE IF NOT EXISTS symbol_relations (
    id INTEGER PRIMARY KEY,
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    "commit" TEXT,
    path TEXT NOT NULL,
    symbol_id TEXT NOT NULL,
    symbol_stable_id TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    language TEXT NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    signature TEXT,
    parent_symbol_id TEXT,
    visibility TEXT,
    content TEXT,
    content_hash TEXT NOT NULL,
    UNIQUE(repo, "ref", path, qualified_name, kind, line_start),
    UNIQUE(repo, "ref", symbol_stable_id, kind)
);

CREATE INDEX IF NOT EXISTS idx_symbol_relations_lookup
    ON symbol_relations(repo, "ref", path, line_start);
CREATE INDEX IF NOT EXISTS idx_symbol_relations_name
    ON symbol_relations(repo, "ref", name);
CREATE INDEX IF NOT EXISTS idx_symbol_relations_symbol_id
    ON symbol_relations(repo, "ref", symbol_id);
CREATE INDEX IF NOT EXISTS idx_symbol_relations_symbol_stable_id
    ON symbol_relations(repo, "ref", symbol_stable_id);
CREATE INDEX IF NOT EXISTS idx_symbol_relations_parent_symbol_id
    ON symbol_relations(repo, "ref", parent_symbol_id);

CREATE TABLE IF NOT EXISTS symbol_edges (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    from_symbol_id TEXT NOT NULL,
    to_symbol_id TEXT,
    to_name TEXT,
    edge_type TEXT NOT NULL,
    confidence TEXT DEFAULT 'static',
    source_file TEXT,
    source_line INTEGER,
    CHECK (to_symbol_id IS NOT NULL OR to_name IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS branch_state (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    merge_base_commit TEXT,
    last_indexed_commit TEXT NOT NULL,
    overlay_dir TEXT,
    file_count INTEGER DEFAULT 0,
    symbol_count INTEGER DEFAULT 0,
    is_default_branch INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'active',
    eviction_eligible_at TEXT,
    created_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL,
    PRIMARY KEY(repo, "ref")
);
CREATE INDEX IF NOT EXISTS idx_branch_state_eviction
    ON branch_state(repo, status, last_accessed_at);

CREATE TABLE IF NOT EXISTS branch_tombstones (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    path TEXT NOT NULL,
    tombstone_type TEXT NOT NULL DEFAULT 'deleted',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(repo, "ref", path)
);

CREATE TABLE IF NOT EXISTS worktree_leases (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    owner_pid INTEGER NOT NULL,
    refcount INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(repo, "ref")
);

CREATE INDEX IF NOT EXISTS idx_worktree_leases_status_updated
    ON worktree_leases(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_worktree_leases_status
    ON worktree_leases(status, last_used_at);

CREATE TABLE IF NOT EXISTS index_jobs (
    job_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    "ref" TEXT NOT NULL,
    mode TEXT NOT NULL,
    head_commit TEXT,
    sync_id TEXT,
    status TEXT NOT NULL DEFAULT 'queued',
    changed_files INTEGER DEFAULT 0,
    duration_ms INTEGER,
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,
    progress_token TEXT,
    files_scanned INTEGER DEFAULT 0,
    files_indexed INTEGER DEFAULT 0,
    symbols_extracted INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON index_jobs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_jobs_project_status_created
    ON index_jobs(project_id, status, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_active_project_ref
    ON index_jobs(project_id, "ref")
    WHERE status IN ('queued', 'running', 'validating');
CREATE UNIQUE INDEX IF NOT EXISTS idx_symbol_edges_unique
    ON symbol_edges(
        repo,
        "ref",
        from_symbol_id,
        edge_type,
        COALESCE(to_symbol_id, ''),
        COALESCE(to_name, ''),
        COALESCE(source_file, ''),
        COALESCE(source_line, -1)
    );
CREATE INDEX IF NOT EXISTS idx_symbol_edges_to
    ON symbol_edges(repo, "ref", to_symbol_id);
CREATE INDEX IF NOT EXISTS idx_symbol_edges_from_type
    ON symbol_edges(repo, "ref", from_symbol_id, edge_type);
CREATE INDEX IF NOT EXISTS idx_symbol_edges_to_type
    ON symbol_edges(repo, "ref", to_symbol_id, edge_type);
CREATE INDEX IF NOT EXISTS idx_symbol_edges_source_file
    ON symbol_edges(repo, "ref", source_file, edge_type);

CREATE TABLE IF NOT EXISTS known_workspaces (
    workspace_path TEXT PRIMARY KEY,
    project_id TEXT REFERENCES projects(project_id),
    auto_discovered INTEGER DEFAULT 0,
    last_used_at TEXT NOT NULL,
    index_status TEXT DEFAULT 'unknown'
);

"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::tempdir;

    #[test]
    fn test_create_tables() {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        create_tables(&conn).unwrap();

        // Verify all tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"file_manifest".to_string()));
        assert!(tables.contains(&"symbol_relations".to_string()));
        assert!(tables.contains(&"symbol_edges".to_string()));
        assert!(tables.contains(&"branch_state".to_string()));
        assert!(tables.contains(&"branch_tombstones".to_string()));
        assert!(tables.contains(&"worktree_leases".to_string()));
        assert!(tables.contains(&"index_jobs".to_string()));
        assert!(tables.contains(&"known_workspaces".to_string()));
        assert!(tables.contains(&"semantic_vectors".to_string()));
        assert!(tables.contains(&"semantic_vector_meta".to_string()));
    }

    #[test]
    fn test_schema_contains_vcs_core_columns() {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        create_tables(&conn).unwrap();

        let branch_state_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('branch_state') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(branch_state_cols.contains(&"symbol_count".to_string()));
        assert!(branch_state_cols.contains(&"is_default_branch".to_string()));
        assert!(branch_state_cols.contains(&"status".to_string()));
        assert!(branch_state_cols.contains(&"eviction_eligible_at".to_string()));
        let branch_state_eviction_idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_branch_state_eviction'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(branch_state_eviction_idx, 1);

        let tombstone_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('branch_tombstones') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(tombstone_cols.contains(&"tombstone_type".to_string()));
        assert!(tombstone_cols.contains(&"created_at".to_string()));

        let worktree_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('worktree_leases') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(worktree_cols.contains(&"last_used_at".to_string()));
        let worktree_last_used_idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_worktree_leases_status_last_used'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // V12 migration removes this duplicate index.
        assert_eq!(worktree_last_used_idx, 0);
        let worktree_status_idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_worktree_leases_status'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(worktree_status_idx, 1);

        let active_job_unique_idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_jobs_active_project_ref'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_job_unique_idx, 1);

        let symbol_edge_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('symbol_edges') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(symbol_edge_cols.contains(&"to_name".to_string()));
        assert!(symbol_edge_cols.contains(&"source_file".to_string()));
        assert!(symbol_edge_cols.contains(&"source_line".to_string()));
        let symbol_edge_unique_idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_symbol_edges_unique'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(symbol_edge_unique_idx, 1);

        let symbol_relation_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('symbol_relations') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(symbol_relation_cols.contains(&"content".to_string()));

        let vector_cols: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('semantic_vectors') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(vector_cols.contains(&"embedding_model_version".to_string()));
        assert!(vector_cols.contains(&"symbol_stable_id".to_string()));
        assert!(vector_cols.contains(&"vector_json".to_string()));

        let vector_version: String = conn
            .query_row(
                "SELECT meta_value FROM semantic_vector_meta WHERE meta_key = 'vector_schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(vector_version, "1");
    }

    #[test]
    fn test_create_tables_idempotent() {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        create_tables(&conn).unwrap();
        // Running again should not fail
        create_tables(&conn).unwrap();
    }

    #[test]
    fn test_migration_tracking() {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        create_tables(&conn).unwrap();

        // schema_migrations table should exist with current version recorded
        let version: u32 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);

        // Running migrate again should be a no-op
        migrate(&conn).unwrap();
        let version2: u32 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version2, CURRENT_SCHEMA_VERSION);
    }
}
