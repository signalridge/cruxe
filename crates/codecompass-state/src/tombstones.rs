use codecompass_core::error::StateError;
use rusqlite::{Connection, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTombstone {
    pub repo: String,
    pub r#ref: String,
    pub path: String,
    pub tombstone_type: String,
    pub created_at: String,
}

pub fn create_tombstone(conn: &Connection, tombstone: &BranchTombstone) -> Result<(), StateError> {
    conn.execute(
        "INSERT INTO branch_tombstones (repo, \"ref\", path, tombstone_type, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(repo, \"ref\", path) DO UPDATE SET
            tombstone_type = excluded.tombstone_type,
            created_at = excluded.created_at",
        params![
            tombstone.repo,
            tombstone.r#ref,
            tombstone.path,
            tombstone.tombstone_type,
            tombstone.created_at
        ],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn bulk_upsert(
    conn: &mut Connection,
    tombstones: &[BranchTombstone],
) -> Result<(), StateError> {
    let tx = conn.transaction().map_err(StateError::sqlite)?;
    for t in tombstones {
        tx.execute(
            "INSERT INTO branch_tombstones (repo, \"ref\", path, tombstone_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(repo, \"ref\", path) DO UPDATE SET
                tombstone_type = excluded.tombstone_type,
                created_at = excluded.created_at",
            params![t.repo, t.r#ref, t.path, t.tombstone_type, t.created_at],
        )
        .map_err(StateError::sqlite)?;
    }
    tx.commit().map_err(StateError::sqlite)?;
    Ok(())
}

pub fn delete_for_ref(conn: &Connection, repo: &str, r#ref: &str) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM branch_tombstones WHERE repo = ?1 AND \"ref\" = ?2",
        params![repo, r#ref],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn list_paths_for_ref(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
) -> Result<Vec<String>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT path
             FROM branch_tombstones
             WHERE repo = ?1 AND \"ref\" = ?2
             ORDER BY path ASC",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![repo, r#ref], |row| row.get::<_, String>(0))
        .map_err(StateError::sqlite)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StateError::sqlite)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema};
    use tempfile::tempdir;

    fn setup_db() -> Connection {
        let dir = tempdir().unwrap();
        let conn = db::open_connection(&dir.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        conn
    }

    fn sample(path: &str) -> BranchTombstone {
        BranchTombstone {
            repo: "repo".to_string(),
            r#ref: "feat/auth".to_string(),
            path: path.to_string(),
            tombstone_type: "deleted".to_string(),
            created_at: "2026-02-25T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn create_and_list_tombstones() {
        let conn = setup_db();
        create_tombstone(&conn, &sample("src/old.rs")).unwrap();
        create_tombstone(&conn, &sample("src/renamed.rs")).unwrap();

        let paths = list_paths_for_ref(&conn, "repo", "feat/auth").unwrap();
        assert_eq!(paths, vec!["src/old.rs", "src/renamed.rs"]);
    }

    #[test]
    fn bulk_upsert_updates_existing_rows() {
        let mut conn = setup_db();
        let mut t = sample("src/old.rs");
        create_tombstone(&conn, &t).unwrap();

        t.tombstone_type = "replaced".to_string();
        t.created_at = "2026-02-25T11:00:00Z".to_string();
        bulk_upsert(&mut conn, &[t.clone()]).unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT tombstone_type, created_at
                 FROM branch_tombstones
                 WHERE repo = ?1 AND \"ref\" = ?2 AND path = ?3",
            )
            .unwrap();
        let (kind, created): (String, String) = stmt
            .query_row(params!["repo", "feat/auth", "src/old.rs"], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(kind, "replaced");
        assert_eq!(created, "2026-02-25T11:00:00Z");
    }

    #[test]
    fn delete_for_ref_clears_only_target_ref() {
        let conn = setup_db();
        create_tombstone(&conn, &sample("src/a.rs")).unwrap();
        let mut other = sample("src/b.rs");
        other.r#ref = "main".to_string();
        create_tombstone(&conn, &other).unwrap();

        delete_for_ref(&conn, "repo", "feat/auth").unwrap();

        assert!(
            list_paths_for_ref(&conn, "repo", "feat/auth")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            list_paths_for_ref(&conn, "repo", "main").unwrap(),
            vec!["src/b.rs"]
        );
    }
}
