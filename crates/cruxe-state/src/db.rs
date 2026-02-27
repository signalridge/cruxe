use cruxe_core::error::StateError;
use rusqlite::Connection;
use std::path::Path;
use tracing::info;

/// Open a SQLite connection with default pragmas.
pub fn open_connection(db_path: &Path) -> Result<Connection, StateError> {
    open_connection_with_config(db_path, 5000, -64000)
}

/// Open a SQLite connection with configurable pragmas.
pub fn open_connection_with_config(
    db_path: &Path,
    busy_timeout_ms: u32,
    cache_size: i32,
) -> Result<Connection, StateError> {
    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(StateError::Io)?;
    }

    let conn = Connection::open(db_path).map_err(StateError::sqlite)?;

    apply_pragmas(&conn, busy_timeout_ms, cache_size)?;

    info!(?db_path, "SQLite connection opened");
    Ok(conn)
}

/// Apply required SQLite pragmas per data-model spec.
fn apply_pragmas(
    conn: &Connection,
    busy_timeout_ms: u32,
    cache_size: i32,
) -> Result<(), StateError> {
    conn.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = {};
         PRAGMA cache_size = {};",
        busy_timeout_ms, cache_size
    ))
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Run SQLite quick_check to verify database integrity.
/// Returns Ok(true) if healthy, Ok(false) with error detail otherwise.
pub fn check_sqlite_health(conn: &Connection) -> Result<(bool, Option<String>), StateError> {
    let result: String = conn
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(StateError::sqlite)?;

    if result == "ok" {
        Ok((true, None))
    } else {
        Ok((false, Some(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open_connection() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = open_connection(&db_path).unwrap();

        // Verify WAL mode
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");

        // Verify foreign keys
        let fk: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn test_open_connection_with_custom_config() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_custom.db");
        let conn = open_connection_with_config(&db_path, 3000, -32000).unwrap();

        let timeout: i32 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, 3000);

        let cache: i32 = conn
            .query_row("PRAGMA cache_size", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cache, -32000);
    }
}
