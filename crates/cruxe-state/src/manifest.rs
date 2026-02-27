use cruxe_core::error::StateError;
use rusqlite::{Connection, params};

/// A file manifest entry for incremental diff.
#[derive(Debug, Clone)]
pub struct ManifestEntry {
    pub repo: String,
    pub r#ref: String,
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub mtime_ns: Option<i64>,
    pub language: Option<String>,
    pub indexed_at: String,
}

/// Upsert a file manifest entry.
pub fn upsert_manifest(conn: &Connection, entry: &ManifestEntry) -> Result<(), StateError> {
    conn.execute(
        "INSERT INTO file_manifest (repo, \"ref\", path, content_hash, size_bytes, mtime_ns, language, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(repo, \"ref\", path) DO UPDATE SET
           content_hash = excluded.content_hash,
           size_bytes = excluded.size_bytes,
           mtime_ns = excluded.mtime_ns,
           language = excluded.language,
           indexed_at = excluded.indexed_at",
        params![
            entry.repo,
            entry.r#ref,
            entry.path,
            entry.content_hash,
            entry.size_bytes,
            entry.mtime_ns,
            entry.language,
            entry.indexed_at,
        ],
    ).map_err(StateError::sqlite)?;
    Ok(())
}

/// Get the content hash for a file. Returns None if not in manifest.
pub fn get_content_hash(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    path: &str,
) -> Result<Option<String>, StateError> {
    let result = conn.query_row(
        "SELECT content_hash FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3",
        params![repo, r#ref, path],
        |row| row.get(0),
    );

    match result {
        Ok(hash) => Ok(Some(hash)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

/// Delete manifest entries for files that no longer exist.
pub fn delete_manifest(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    path: &str,
) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3",
        params![repo, r#ref, path],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

/// Get file count for a repo/ref.
pub fn file_count(conn: &Connection, repo: &str, r#ref: &str) -> Result<u64, StateError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2",
            params![repo, r#ref],
            |row| row.get(0),
        )
        .map_err(StateError::sqlite)?;
    Ok(count as u64)
}

/// Get all manifest entries for a repo/ref.
pub fn get_all_entries(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
) -> Result<Vec<ManifestEntry>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", path, content_hash, size_bytes, mtime_ns, language, indexed_at
         FROM file_manifest WHERE repo = ?1 AND \"ref\" = ?2",
        )
        .map_err(StateError::sqlite)?;

    let entries = stmt
        .query_map(params![repo, r#ref], |row| {
            Ok(ManifestEntry {
                repo: row.get(0)?,
                r#ref: row.get(1)?,
                path: row.get(2)?,
                content_hash: row.get(3)?,
                size_bytes: row.get::<_, i64>(4)? as u64,
                mtime_ns: row.get(5)?,
                language: row.get(6)?,
                indexed_at: row.get(7)?,
            })
        })
        .map_err(StateError::sqlite)?;

    entries
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

    fn sample_entry() -> ManifestEntry {
        ManifestEntry {
            repo: "my-repo".to_string(),
            r#ref: "main".to_string(),
            path: "src/lib.rs".to_string(),
            content_hash: "abc123def456".to_string(),
            size_bytes: 1024,
            mtime_ns: Some(1700000000000000000),
            language: Some("rust".to_string()),
            indexed_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_upsert_and_get_content_hash() {
        let conn = setup_test_db();
        let entry = sample_entry();

        upsert_manifest(&conn, &entry).unwrap();

        let hash = get_content_hash(&conn, &entry.repo, &entry.r#ref, &entry.path).unwrap();
        assert_eq!(hash, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_get_content_hash_returns_none_when_not_found() {
        let conn = setup_test_db();
        let hash = get_content_hash(&conn, "no-repo", "main", "no-file.rs").unwrap();
        assert!(hash.is_none());
    }

    #[test]
    fn test_upsert_updates_existing_entry() {
        let conn = setup_test_db();
        let entry = sample_entry();
        upsert_manifest(&conn, &entry).unwrap();

        // Upsert with a new content hash for the same (repo, ref, path)
        let mut updated = entry.clone();
        updated.content_hash = "new_hash_789".to_string();
        updated.size_bytes = 2048;
        updated.language = Some("python".to_string());
        upsert_manifest(&conn, &updated).unwrap();

        let hash = get_content_hash(&conn, &updated.repo, &updated.r#ref, &updated.path).unwrap();
        assert_eq!(hash, Some("new_hash_789".to_string()));

        // Should still be only 1 entry
        let count = file_count(&conn, &updated.repo, &updated.r#ref).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_delete_manifest() {
        let conn = setup_test_db();
        let entry = sample_entry();
        upsert_manifest(&conn, &entry).unwrap();

        delete_manifest(&conn, &entry.repo, &entry.r#ref, &entry.path).unwrap();

        let hash = get_content_hash(&conn, &entry.repo, &entry.r#ref, &entry.path).unwrap();
        assert!(hash.is_none());
    }

    #[test]
    fn test_delete_manifest_nonexistent_is_ok() {
        let conn = setup_test_db();
        // Deleting a non-existent entry should succeed (0 rows affected)
        let result = delete_manifest(&conn, "no-repo", "main", "no-file.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn test_file_count() {
        let conn = setup_test_db();

        // Initially 0
        let count = file_count(&conn, "my-repo", "main").unwrap();
        assert_eq!(count, 0);

        // Add entries
        let entry1 = sample_entry();
        upsert_manifest(&conn, &entry1).unwrap();

        let mut entry2 = sample_entry();
        entry2.path = "src/main.rs".to_string();
        upsert_manifest(&conn, &entry2).unwrap();

        let mut entry3 = sample_entry();
        entry3.path = "src/utils.rs".to_string();
        upsert_manifest(&conn, &entry3).unwrap();

        let count = file_count(&conn, "my-repo", "main").unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_file_count_scoped_to_repo_and_ref() {
        let conn = setup_test_db();
        let entry = sample_entry();
        upsert_manifest(&conn, &entry).unwrap();

        // Different repo should have 0
        let count = file_count(&conn, "other-repo", "main").unwrap();
        assert_eq!(count, 0);

        // Different ref should have 0
        let count = file_count(&conn, "my-repo", "develop").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_get_all_entries() {
        let conn = setup_test_db();

        let entry1 = sample_entry();
        upsert_manifest(&conn, &entry1).unwrap();

        let mut entry2 = sample_entry();
        entry2.path = "src/main.rs".to_string();
        entry2.content_hash = "hash2".to_string();
        entry2.size_bytes = 512;
        upsert_manifest(&conn, &entry2).unwrap();

        let entries = get_all_entries(&conn, "my-repo", "main").unwrap();
        assert_eq!(entries.len(), 2);

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"src/main.rs"));
    }

    #[test]
    fn test_get_all_entries_empty() {
        let conn = setup_test_db();
        let entries = get_all_entries(&conn, "no-repo", "main").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_entry_with_no_optional_fields() {
        let conn = setup_test_db();
        let entry = ManifestEntry {
            repo: "repo".to_string(),
            r#ref: "main".to_string(),
            path: "file.txt".to_string(),
            content_hash: "hash".to_string(),
            size_bytes: 100,
            mtime_ns: None,
            language: None,
            indexed_at: "2026-01-01T00:00:00Z".to_string(),
        };

        upsert_manifest(&conn, &entry).unwrap();

        let entries = get_all_entries(&conn, "repo", "main").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].mtime_ns.is_none());
        assert!(entries[0].language.is_none());
    }

    #[test]
    fn test_delete_then_file_count() {
        let conn = setup_test_db();
        let entry = sample_entry();
        upsert_manifest(&conn, &entry).unwrap();

        assert_eq!(file_count(&conn, "my-repo", "main").unwrap(), 1);

        delete_manifest(&conn, &entry.repo, &entry.r#ref, &entry.path).unwrap();

        assert_eq!(file_count(&conn, "my-repo", "main").unwrap(), 0);
    }
}
