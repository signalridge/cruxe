use cruxe_core::error::StateError;
use cruxe_core::types::Project;
use rusqlite::{Connection, params};

/// Create a new project entry.
pub fn create_project(conn: &Connection, project: &Project) -> Result<(), StateError> {
    conn.execute(
        "INSERT INTO projects (project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            project.project_id,
            project.repo_root,
            project.display_name,
            project.default_ref,
            project.vcs_mode as i32,
            project.schema_version,
            project.parser_version,
            project.created_at,
            project.updated_at,
        ],
    ).map_err(StateError::sqlite)?;
    Ok(())
}

/// Get a project by its repo root path.
pub fn get_by_root(conn: &Connection, repo_root: &str) -> Result<Option<Project>, StateError> {
    let mut stmt = conn
        .prepare("SELECT project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at FROM projects WHERE repo_root = ?1")
        .map_err(StateError::sqlite)?;

    let result = stmt.query_row(params![repo_root], |row| {
        Ok(Project {
            project_id: row.get(0)?,
            repo_root: row.get(1)?,
            display_name: row.get(2)?,
            default_ref: row.get(3)?,
            vcs_mode: row.get::<_, i32>(4)? != 0,
            schema_version: row.get(5)?,
            parser_version: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    });

    match result {
        Ok(project) => Ok(Some(project)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Get a project by its ID.
pub fn get_by_id(conn: &Connection, project_id: &str) -> Result<Option<Project>, StateError> {
    let mut stmt = conn
        .prepare("SELECT project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at FROM projects WHERE project_id = ?1")
        .map_err(StateError::sqlite)?;

    let result = stmt.query_row(params![project_id], |row| {
        Ok(Project {
            project_id: row.get(0)?,
            repo_root: row.get(1)?,
            display_name: row.get(2)?,
            default_ref: row.get(3)?,
            vcs_mode: row.get::<_, i32>(4)? != 0,
            schema_version: row.get(5)?,
            parser_version: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    });

    match result {
        Ok(project) => Ok(Some(project)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Update a project's updated_at timestamp.
pub fn update_project(conn: &Connection, project: &Project) -> Result<(), StateError> {
    conn.execute(
        "UPDATE projects SET display_name = ?1, default_ref = ?2, schema_version = ?3, parser_version = ?4, updated_at = ?5 WHERE project_id = ?6",
        params![
            project.display_name,
            project.default_ref,
            project.schema_version,
            project.parser_version,
            project.updated_at,
            project.project_id,
        ],
    ).map_err(StateError::sqlite)?;
    Ok(())
}

/// List all registered projects.
pub fn list_projects(conn: &Connection) -> Result<Vec<Project>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT project_id, repo_root, display_name, default_ref, vcs_mode, schema_version, parser_version, created_at, updated_at
             FROM projects
             ORDER BY repo_root",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Project {
                project_id: row.get(0)?,
                repo_root: row.get(1)?,
                display_name: row.get(2)?,
                default_ref: row.get(3)?,
                vcs_mode: row.get::<_, i32>(4)? != 0,
                schema_version: row.get(5)?,
                parser_version: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
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

    fn sample_project() -> Project {
        Project {
            project_id: "proj_abc123".to_string(),
            repo_root: "/home/user/my-project".to_string(),
            display_name: Some("My Project".to_string()),
            default_ref: "main".to_string(),
            vcs_mode: true,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_create_and_get_by_root() {
        let conn = setup_test_db();
        let project = sample_project();

        create_project(&conn, &project).unwrap();

        let found = get_by_root(&conn, &project.repo_root).unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.project_id, project.project_id);
        assert_eq!(found.repo_root, project.repo_root);
        assert_eq!(found.display_name, project.display_name);
        assert_eq!(found.default_ref, project.default_ref);
        assert_eq!(found.vcs_mode, project.vcs_mode);
        assert_eq!(found.schema_version, project.schema_version);
        assert_eq!(found.parser_version, project.parser_version);
        assert_eq!(found.created_at, project.created_at);
        assert_eq!(found.updated_at, project.updated_at);
    }

    #[test]
    fn test_create_and_get_by_id() {
        let conn = setup_test_db();
        let project = sample_project();

        create_project(&conn, &project).unwrap();

        let found = get_by_id(&conn, &project.project_id).unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.project_id, project.project_id);
        assert_eq!(found.repo_root, project.repo_root);
    }

    #[test]
    fn test_get_by_root_returns_none_when_not_found() {
        let conn = setup_test_db();
        let found = get_by_root(&conn, "/nonexistent/path").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_get_by_id_returns_none_when_not_found() {
        let conn = setup_test_db();
        let found = get_by_id(&conn, "nonexistent_id").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_create_duplicate_project_id_fails() {
        let conn = setup_test_db();
        let project = sample_project();
        create_project(&conn, &project).unwrap();

        // Same project_id should fail (PRIMARY KEY constraint)
        let result = create_project(&conn, &project);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_duplicate_repo_root_fails() {
        let conn = setup_test_db();
        let project = sample_project();
        create_project(&conn, &project).unwrap();

        // Different project_id but same repo_root should fail (UNIQUE constraint)
        let mut project2 = sample_project();
        project2.project_id = "proj_different".to_string();
        let result = create_project(&conn, &project2);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_project() {
        let conn = setup_test_db();
        let project = sample_project();
        create_project(&conn, &project).unwrap();

        let mut updated = project.clone();
        updated.display_name = Some("Updated Name".to_string());
        updated.default_ref = "develop".to_string();
        updated.schema_version = 2;
        updated.parser_version = 3;
        updated.updated_at = "2026-06-15T12:00:00Z".to_string();

        update_project(&conn, &updated).unwrap();

        let found = get_by_id(&conn, &updated.project_id).unwrap().unwrap();
        assert_eq!(found.display_name, Some("Updated Name".to_string()));
        assert_eq!(found.default_ref, "develop");
        assert_eq!(found.schema_version, 2);
        assert_eq!(found.parser_version, 3);
        assert_eq!(found.updated_at, "2026-06-15T12:00:00Z");
        // created_at should remain unchanged
        assert_eq!(found.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn test_update_nonexistent_project_succeeds_silently() {
        let conn = setup_test_db();
        let project = sample_project();
        // UPDATE on a nonexistent row is not an error in SQL; it affects 0 rows
        let result = update_project(&conn, &project);
        assert!(result.is_ok());
    }

    #[test]
    fn test_project_with_no_display_name() {
        let conn = setup_test_db();
        let mut project = sample_project();
        project.display_name = None;

        create_project(&conn, &project).unwrap();

        let found = get_by_id(&conn, &project.project_id).unwrap().unwrap();
        assert_eq!(found.display_name, None);
    }

    #[test]
    fn test_vcs_mode_false() {
        let conn = setup_test_db();
        let mut project = sample_project();
        project.vcs_mode = false;

        create_project(&conn, &project).unwrap();

        let found = get_by_id(&conn, &project.project_id).unwrap().unwrap();
        assert!(!found.vcs_mode);
    }

    #[test]
    fn test_list_projects() {
        let conn = setup_test_db();
        let mut p1 = sample_project();
        p1.project_id = "proj_1".to_string();
        p1.repo_root = "/tmp/a".to_string();
        let mut p2 = sample_project();
        p2.project_id = "proj_2".to_string();
        p2.repo_root = "/tmp/b".to_string();

        create_project(&conn, &p1).unwrap();
        create_project(&conn, &p2).unwrap();

        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].repo_root, "/tmp/a");
        assert_eq!(projects[1].repo_root, "/tmp/b");
    }
}
