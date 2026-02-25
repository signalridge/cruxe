use codecompass_core::error::StateError;
use rusqlite::{Connection, params};

fn canonical_status(status: &str) -> &str {
    match status {
        "in_use" => "active",
        "released" => "stale",
        _ => status,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeLease {
    pub repo: String,
    pub r#ref: String,
    pub worktree_path: String,
    pub owner_pid: i64,
    pub refcount: i64,
    pub status: String,
    pub created_at: String,
    pub last_used_at: String,
}

pub fn create_lease(conn: &Connection, lease: &WorktreeLease) -> Result<(), StateError> {
    let status = canonical_status(&lease.status);
    conn.execute(
        "INSERT INTO worktree_leases
         (repo, \"ref\", worktree_path, owner_pid, refcount, status, created_at, last_used_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(repo, \"ref\") DO UPDATE SET
            worktree_path = excluded.worktree_path,
            owner_pid = excluded.owner_pid,
            refcount = excluded.refcount,
            status = excluded.status,
            created_at = excluded.created_at,
            last_used_at = excluded.last_used_at,
            updated_at = excluded.last_used_at",
        params![
            lease.repo,
            lease.r#ref,
            lease.worktree_path,
            lease.owner_pid,
            lease.refcount,
            status,
            lease.created_at,
            lease.last_used_at
        ],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn get_lease(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
) -> Result<Option<WorktreeLease>, StateError> {
    let row = conn.query_row(
        "SELECT repo, \"ref\", worktree_path, owner_pid, refcount,
                CASE status
                    WHEN 'in_use' THEN 'active'
                    WHEN 'released' THEN 'stale'
                    ELSE status
                END AS status,
                created_at,
                COALESCE(last_used_at, updated_at, created_at) AS last_used_at
         FROM worktree_leases
         WHERE repo = ?1 AND \"ref\" = ?2",
        params![repo, r#ref],
        |row| {
            Ok(WorktreeLease {
                repo: row.get(0)?,
                r#ref: row.get(1)?,
                worktree_path: row.get(2)?,
                owner_pid: row.get(3)?,
                refcount: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                last_used_at: row.get(7)?,
            })
        },
    );

    match row {
        Ok(lease) => Ok(Some(lease)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(StateError::sqlite(e)),
    }
}

pub fn update_refcount(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    refcount: i64,
    last_used_at: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE worktree_leases
         SET refcount = ?3, last_used_at = ?4, updated_at = ?4
         WHERE repo = ?1 AND \"ref\" = ?2",
        params![repo, r#ref, refcount, last_used_at],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn update_status(
    conn: &Connection,
    repo: &str,
    r#ref: &str,
    status: &str,
    last_used_at: &str,
) -> Result<(), StateError> {
    conn.execute(
        "UPDATE worktree_leases
         SET status = ?3, last_used_at = ?4, updated_at = ?4
         WHERE repo = ?1 AND \"ref\" = ?2",
        params![repo, r#ref, canonical_status(status), last_used_at],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn delete_lease(conn: &Connection, repo: &str, r#ref: &str) -> Result<(), StateError> {
    conn.execute(
        "DELETE FROM worktree_leases WHERE repo = ?1 AND \"ref\" = ?2",
        params![repo, r#ref],
    )
    .map_err(StateError::sqlite)?;
    Ok(())
}

pub fn list_stale(conn: &Connection, stale_before: &str) -> Result<Vec<WorktreeLease>, StateError> {
    let mut stmt = conn
        .prepare(
            "SELECT repo, \"ref\", worktree_path, owner_pid, refcount,
                    CASE status
                        WHEN 'in_use' THEN 'active'
                        WHEN 'released' THEN 'stale'
                        ELSE status
                    END AS status,
                    created_at,
                    COALESCE(last_used_at, updated_at, created_at) AS last_used_at
             FROM worktree_leases
             WHERE
                CASE status
                    WHEN 'in_use' THEN 'active'
                    WHEN 'released' THEN 'stale'
                    ELSE status
                END != 'active'
                OR COALESCE(last_used_at, updated_at, created_at) < ?1
             ORDER BY COALESCE(last_used_at, updated_at, created_at) ASC",
        )
        .map_err(StateError::sqlite)?;

    let rows = stmt
        .query_map(params![stale_before], |row| {
            Ok(WorktreeLease {
                repo: row.get(0)?,
                r#ref: row.get(1)?,
                worktree_path: row.get(2)?,
                owner_pid: row.get(3)?,
                refcount: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                last_used_at: row.get(7)?,
            })
        })
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

    fn sample() -> WorktreeLease {
        WorktreeLease {
            repo: "repo-1".to_string(),
            r#ref: "feat/auth".to_string(),
            worktree_path: "/tmp/wt-feat-auth".to_string(),
            owner_pid: 1000,
            refcount: 1,
            status: "active".to_string(),
            created_at: "2026-02-25T10:00:00Z".to_string(),
            last_used_at: "2026-02-25T10:00:00Z".to_string(),
        }
    }

    #[test]
    fn create_and_get_lease() {
        let conn = setup_db();
        let lease = sample();
        create_lease(&conn, &lease).unwrap();

        let got = get_lease(&conn, &lease.repo, &lease.r#ref)
            .unwrap()
            .unwrap();
        assert_eq!(got, lease);
    }

    #[test]
    fn update_refcount_and_status() {
        let conn = setup_db();
        let lease = sample();
        create_lease(&conn, &lease).unwrap();

        update_refcount(&conn, &lease.repo, &lease.r#ref, 2, "2026-02-25T10:01:00Z").unwrap();
        update_status(
            &conn,
            &lease.repo,
            &lease.r#ref,
            "stale",
            "2026-02-25T10:01:30Z",
        )
        .unwrap();

        let got = get_lease(&conn, &lease.repo, &lease.r#ref)
            .unwrap()
            .unwrap();
        assert_eq!(got.refcount, 2);
        assert_eq!(got.status, "stale");
        assert_eq!(got.last_used_at, "2026-02-25T10:01:30Z");
    }

    #[test]
    fn list_stale_filters_by_time_or_status() {
        let conn = setup_db();
        let mut a = sample();
        a.r#ref = "feat/a".to_string();
        a.last_used_at = "2026-02-25T09:00:00Z".to_string();
        create_lease(&conn, &a).unwrap();

        let mut b = sample();
        b.r#ref = "feat/b".to_string();
        b.last_used_at = "2026-02-25T11:00:00Z".to_string();
        b.status = "stale".to_string();
        create_lease(&conn, &b).unwrap();

        let mut c = sample();
        c.r#ref = "feat/c".to_string();
        c.last_used_at = "2026-02-25T11:00:00Z".to_string();
        c.status = "active".to_string();
        create_lease(&conn, &c).unwrap();

        let stale = list_stale(&conn, "2026-02-25T10:30:00Z").unwrap();
        let refs: Vec<String> = stale.into_iter().map(|it| it.r#ref).collect();
        assert!(refs.contains(&"feat/a".to_string()));
        assert!(refs.contains(&"feat/b".to_string()));
        assert!(!refs.contains(&"feat/c".to_string()));
    }

    #[test]
    fn status_aliases_are_normalized() {
        let conn = setup_db();
        let mut lease = sample();
        lease.status = "in_use".to_string();
        create_lease(&conn, &lease).unwrap();

        let got = get_lease(&conn, &lease.repo, &lease.r#ref)
            .unwrap()
            .unwrap();
        assert_eq!(got.status, "active");

        update_status(
            &conn,
            &lease.repo,
            &lease.r#ref,
            "released",
            "2026-02-25T10:02:00Z",
        )
        .unwrap();
        let got = get_lease(&conn, &lease.repo, &lease.r#ref)
            .unwrap()
            .unwrap();
        assert_eq!(got.status, "stale");
    }

    #[test]
    fn delete_lease_is_idempotent() {
        let conn = setup_db();
        let lease = sample();
        create_lease(&conn, &lease).unwrap();
        delete_lease(&conn, &lease.repo, &lease.r#ref).unwrap();
        delete_lease(&conn, &lease.repo, &lease.r#ref).unwrap();
        assert!(
            get_lease(&conn, &lease.repo, &lease.r#ref)
                .unwrap()
                .is_none()
        );
    }
}
