use codecompass_core::error::StateError;
use rusqlite::Connection;
use tracing::info;

/// Current schema version. Bump this when adding a new migration step.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Create all required SQLite tables per data-model.md and run any pending migrations.
pub fn create_tables(conn: &Connection) -> Result<(), StateError> {
    conn.execute_batch(SCHEMA_SQL).map_err(StateError::sqlite)?;
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
    to_symbol_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    confidence TEXT DEFAULT 'static',
    PRIMARY KEY(repo, "ref", from_symbol_id, to_symbol_id, edge_type)
);

CREATE TABLE IF NOT EXISTS branch_state (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    merge_base_commit TEXT,
    last_indexed_commit TEXT NOT NULL,
    overlay_dir TEXT,
    file_count INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    last_accessed_at TEXT NOT NULL,
    PRIMARY KEY(repo, "ref")
);

CREATE TABLE IF NOT EXISTS branch_tombstones (
    repo TEXT NOT NULL,
    "ref" TEXT NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY(repo, "ref", path)
);

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
CREATE INDEX IF NOT EXISTS idx_symbol_edges_to
    ON symbol_edges(repo, "ref", to_symbol_id);
CREATE INDEX IF NOT EXISTS idx_symbol_edges_from_type
    ON symbol_edges(repo, "ref", from_symbol_id, edge_type);
CREATE INDEX IF NOT EXISTS idx_symbol_edges_to_type
    ON symbol_edges(repo, "ref", to_symbol_id, edge_type);

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
        assert!(tables.contains(&"index_jobs".to_string()));
        assert!(tables.contains(&"known_workspaces".to_string()));
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
