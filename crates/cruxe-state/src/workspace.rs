use cruxe_core::error::StateError;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

/// A known workspace entry from the `known_workspaces` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownWorkspace {
    pub workspace_path: String,
    pub project_id: Option<String>,
    pub auto_discovered: bool,
    pub last_used_at: String,
    pub index_status: String,
}

/// Register a new workspace or update an existing one.
///
/// `project_id` can be `None` for auto-discovered workspaces where the project
/// entry hasn't been created yet (FK to `projects` allows NULL).
pub fn register_workspace(
    conn: &Connection,
    workspace_path: &str,
    project_id: Option<&str>,
    auto_discovered: bool,
    now: &str,
) -> Result<(), StateError> {
    conn.execute(
        "INSERT INTO known_workspaces (workspace_path, project_id, auto_discovered, last_used_at, index_status)
         VALUES (?1, ?2, ?3, ?4, 'not_indexed')
         ON CONFLICT(workspace_path) DO UPDATE SET
           project_id = COALESCE(excluded.project_id, known_workspaces.project_id),
           last_used_at = excluded.last_used_at",
        params![workspace_path, project_id, auto_discovered as i32, now],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Update the project_id for a known workspace (used after project creation).
pub fn update_workspace_project_id(
    conn: &Connection,
    workspace_path: &str,
    project_id: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE known_workspaces SET project_id = ?1 WHERE workspace_path = ?2",
        params![project_id, workspace_path],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Get a workspace entry by path.
pub fn get_workspace(
    conn: &Connection,
    workspace_path: &str,
) -> Result<Option<KnownWorkspace>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT workspace_path, project_id, auto_discovered, last_used_at, index_status
             FROM known_workspaces WHERE workspace_path = ?1",
        )
        .map_err(StateError::sqlite)?;

    let result = stmt.query_row(params![workspace_path], |row| {
        Ok(KnownWorkspace {
            workspace_path: row.get(0)?,
            project_id: row.get(1)?,
            auto_discovered: row.get::<_, i32>(2)? != 0,
            last_used_at: row.get(3)?,
            index_status: row.get(4)?,
        })
    });

    match result {
        Ok(ws) => Ok(Some(ws)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Update the last_used_at timestamp for a workspace.
pub fn update_last_used(
    conn: &Connection,
    workspace_path: &str,
    now: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE known_workspaces SET last_used_at = ?1 WHERE workspace_path = ?2",
        params![now, workspace_path],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Update the index_status for a workspace.
pub fn update_index_status(
    conn: &Connection,
    workspace_path: &str,
    index_status: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE known_workspaces SET index_status = ?1 WHERE workspace_path = ?2",
        params![index_status, workspace_path],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Attempt to claim bootstrap indexing for a workspace.
///
/// Returns `true` when this caller transitions the workspace into `indexing`
/// state and should launch bootstrap indexing. Returns `false` when another
/// caller already claimed indexing.
pub fn claim_bootstrap_indexing(
    conn: &Connection,
    workspace_path: &str,
    now: &str,
) -> Result<bool, StateError> {
    let updated = conn
        .execute(
            "UPDATE known_workspaces
             SET index_status = 'indexing', last_used_at = ?2
             WHERE workspace_path = ?1
               AND index_status != 'indexing'",
            params![workspace_path, now],
        )
        .map_err(StateError::sqlite)?;
    Ok(updated > 0)
}

/// List all registered workspaces, ordered by last_used_at descending.
pub fn list_workspaces(conn: &Connection) -> Result<Vec<KnownWorkspace>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT workspace_path, project_id, auto_discovered, last_used_at, index_status
             FROM known_workspaces ORDER BY last_used_at DESC",
        )
        .map_err(StateError::sqlite)?;

    let workspaces = stmt
        .query_map([], |row| {
            Ok(KnownWorkspace {
                workspace_path: row.get(0)?,
                project_id: row.get(1)?,
                auto_discovered: row.get::<_, i32>(2)? != 0,
                last_used_at: row.get(3)?,
                index_status: row.get(4)?,
            })
        })
        .map_err(StateError::sqlite)?;

    workspaces
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StateError::Sqlite(e.to_string()))
}

/// Evict the least-recently-used auto-discovered workspaces to stay under `max_count`.
/// Returns the paths of evicted workspaces.
pub fn evict_lru_auto_discovered(
    conn: &Connection,
    max_count: usize,
) -> Result<Vec<String>, StateError> {
    // Count current auto-discovered workspaces
    let count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM known_workspaces WHERE auto_discovered = 1",
            [],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;

    if count <= max_count {
        return Ok(vec![]);
    }

    let to_evict = count - max_count;

    // Find the LRU auto-discovered workspaces
    let mut stmt = conn
        .prepare(
            "SELECT workspace_path FROM known_workspaces
             WHERE auto_discovered = 1
             ORDER BY last_used_at ASC LIMIT ?1",
        )
        .map_err(StateError::sqlite)?;

    let paths: Vec<String> = stmt
        .query_map(params![to_evict], |row| row.get(0))
        .map_err(StateError::sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StateError::Sqlite(e.to_string()))?;

    // Delete them
    for path in &paths {
        conn.execute(
            "DELETE FROM known_workspaces WHERE workspace_path = ?1",
            params![path],
        )
        .map_err(StateError::sqlite)?;
    }

    Ok(paths)
}

/// Delete a workspace entry by path.
pub fn delete_workspace(conn: &Connection, workspace_path: &str) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM known_workspaces WHERE workspace_path = ?1",
        params![workspace_path],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// List workspaces ordered by last_used_at descending, limited to `limit`.
/// Used for warmset prewarm selection.
pub fn list_recent_workspaces(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<KnownWorkspace>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT workspace_path, project_id, auto_discovered, last_used_at, index_status
             FROM known_workspaces ORDER BY last_used_at DESC LIMIT ?1",
        )
        .map_err(StateError::sqlite)?;

    let workspaces = stmt
        .query_map(params![limit], |row| {
            Ok(KnownWorkspace {
                workspace_path: row.get(0)?,
                project_id: row.get(1)?,
                auto_discovered: row.get::<_, i32>(2)? != 0,
                last_used_at: row.get(3)?,
                index_status: row.get(4)?,
            })
        })
        .map_err(StateError::sqlite)?;

    workspaces
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| StateError::Sqlite(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::schema;
    use tempfile::tempdir;

    fn setup_test_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("test.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn insert_test_project(conn: &Connection, project_id: &str, repo_root: &str) {
        let project = cruxe_core::types::Project {
            project_id: project_id.to_string(),
            repo_root: repo_root.to_string(),
            display_name: None,
            default_ref: "main".to_string(),
            vcs_mode: true,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        crate::project::create_project(conn, &project).unwrap();
    }

    #[test]
    fn test_register_and_get_workspace() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");

        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        let ws = get_workspace(&conn, "/home/user/project-a").unwrap();
        assert!(ws.is_some());
        let ws = ws.unwrap();
        assert_eq!(ws.workspace_path, "/home/user/project-a");
        assert_eq!(ws.project_id, Some("proj_1".to_string()));
        assert!(!ws.auto_discovered);
        assert_eq!(ws.index_status, "not_indexed");
    }

    #[test]
    fn test_get_workspace_not_found() {
        let conn = setup_test_db();
        let ws = get_workspace(&conn, "/nonexistent").unwrap();
        assert!(ws.is_none());
    }

    #[test]
    fn test_update_last_used() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        update_last_used(&conn, "/home/user/project-a", "2026-02-01T00:00:00Z").unwrap();

        let ws = get_workspace(&conn, "/home/user/project-a")
            .unwrap()
            .unwrap();
        assert_eq!(ws.last_used_at, "2026-02-01T00:00:00Z");
    }

    #[test]
    fn test_update_index_status() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        update_index_status(&conn, "/home/user/project-a", "ready").unwrap();

        let ws = get_workspace(&conn, "/home/user/project-a")
            .unwrap()
            .unwrap();
        assert_eq!(ws.index_status, "ready");
    }

    #[test]
    fn test_claim_bootstrap_indexing_is_single_winner() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        let now = "2026-01-01T00:00:00Z";

        register_workspace(&conn, "/home/user/project-a", Some("proj_1"), true, now).unwrap();

        let first = claim_bootstrap_indexing(&conn, "/home/user/project-a", now).unwrap();
        let second = claim_bootstrap_indexing(&conn, "/home/user/project-a", now).unwrap();
        assert!(first);
        assert!(!second);

        let ws = get_workspace(&conn, "/home/user/project-a")
            .unwrap()
            .unwrap();
        assert_eq!(ws.index_status, "indexing");
    }

    #[test]
    fn test_list_workspaces() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        insert_test_project(&conn, "proj_2", "/home/user/project-b");

        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        register_workspace(
            &conn,
            "/home/user/project-b",
            Some("proj_2"),
            true,
            "2026-01-02T00:00:00Z",
        )
        .unwrap();

        let all = list_workspaces(&conn).unwrap();
        assert_eq!(all.len(), 2);
        // Most recent first
        assert_eq!(all[0].workspace_path, "/home/user/project-b");
        assert_eq!(all[1].workspace_path, "/home/user/project-a");
    }

    #[test]
    fn test_evict_lru_auto_discovered() {
        let conn = setup_test_db();
        // Register 5 auto-discovered workspaces
        for i in 0..5 {
            let pid = format!("proj_{i}");
            let path = format!("/home/user/project-{i}");
            insert_test_project(&conn, &pid, &path);
            register_workspace(
                &conn,
                &path,
                Some(&pid),
                true,
                &format!("2026-01-0{}T00:00:00Z", i + 1),
            )
            .unwrap();
        }

        // Evict to max 3
        let evicted = evict_lru_auto_discovered(&conn, 3).unwrap();
        assert_eq!(evicted.len(), 2);
        // Should evict the two oldest (project-0 and project-1)
        assert!(evicted.contains(&"/home/user/project-0".to_string()));
        assert!(evicted.contains(&"/home/user/project-1".to_string()));

        // Verify only 3 remain
        let remaining = list_workspaces(&conn).unwrap();
        assert_eq!(remaining.len(), 3);
    }

    #[test]
    fn test_evict_lru_no_eviction_needed() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            true,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        let evicted = evict_lru_auto_discovered(&conn, 10).unwrap();
        assert!(evicted.is_empty());
    }

    #[test]
    fn test_evict_lru_skips_non_auto_discovered() {
        let conn = setup_test_db();
        for i in 0..4 {
            let pid = format!("proj_{i}");
            let path = format!("/home/user/project-{i}");
            insert_test_project(&conn, &pid, &path);
            // First two are manually registered, last two are auto-discovered
            register_workspace(
                &conn,
                &path,
                Some(&pid),
                i >= 2,
                &format!("2026-01-0{}T00:00:00Z", i + 1),
            )
            .unwrap();
        }

        // Evict auto-discovered to max 1
        let evicted = evict_lru_auto_discovered(&conn, 1).unwrap();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0], "/home/user/project-2"); // oldest auto-discovered

        // Manual workspaces untouched
        let all = list_workspaces(&conn).unwrap();
        assert_eq!(all.len(), 3);
        assert!(
            all.iter()
                .any(|ws| ws.workspace_path == "/home/user/project-0")
        );
        assert!(
            all.iter()
                .any(|ws| ws.workspace_path == "/home/user/project-1")
        );
    }

    #[test]
    fn test_delete_workspace() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");
        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        delete_workspace(&conn, "/home/user/project-a").unwrap();

        let ws = get_workspace(&conn, "/home/user/project-a").unwrap();
        assert!(ws.is_none());
    }

    #[test]
    fn test_register_workspace_upsert() {
        let conn = setup_test_db();
        insert_test_project(&conn, "proj_1", "/home/user/project-a");

        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        // Re-register same path with updated timestamp
        register_workspace(
            &conn,
            "/home/user/project-a",
            Some("proj_1"),
            false,
            "2026-02-01T00:00:00Z",
        )
        .unwrap();

        let all = list_workspaces(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].last_used_at, "2026-02-01T00:00:00Z");
    }

    #[test]
    fn test_list_recent_workspaces() {
        let conn = setup_test_db();
        for i in 0..5 {
            let pid = format!("proj_{i}");
            let path = format!("/home/user/project-{i}");
            insert_test_project(&conn, &pid, &path);
            register_workspace(
                &conn,
                &path,
                Some(&pid),
                false,
                &format!("2026-01-0{}T00:00:00Z", i + 1),
            )
            .unwrap();
        }

        let recent = list_recent_workspaces(&conn, 3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].workspace_path, "/home/user/project-4");
        assert_eq!(recent[1].workspace_path, "/home/user/project-3");
        assert_eq!(recent[2].workspace_path, "/home/user/project-2");
    }
}
