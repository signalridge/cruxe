use crate::adapter::VcsAdapter;
use crate::diff::{DiffEntry, FileChangeKind};
use cruxe_core::error::StateError;
use cruxe_core::time::now_iso8601;
use cruxe_state::worktree_leases::{self, WorktreeLease};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tracing::warn;

pub struct WorktreeManager<A>
where
    A: VcsAdapter<FileChange = FileChangeKind, DiffEntry = DiffEntry>,
{
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    adapter: A,
}

impl<A> WorktreeManager<A>
where
    A: VcsAdapter<FileChange = FileChangeKind, DiffEntry = DiffEntry>,
{
    pub fn new(repo_root: impl AsRef<Path>, worktrees_root: impl AsRef<Path>, adapter: A) -> Self {
        Self {
            repo_root: repo_root.as_ref().to_path_buf(),
            worktrees_root: worktrees_root.as_ref().to_path_buf(),
            adapter,
        }
    }

    pub fn worktree_path_for_ref(&self, ref_name: &str) -> PathBuf {
        self.worktrees_root.join(normalize_ref_name(ref_name))
    }

    fn active_checkout_path_for_ref(&self, ref_name: &str) -> PathBuf {
        let checkout_path = self.worktree_path_for_ref(ref_name);
        if checkout_path.join(".git").exists() {
            checkout_path
        } else {
            self.repo_root.clone()
        }
    }

    fn is_repo_root_path(&self, path: &Path) -> bool {
        match (path.canonicalize(), self.repo_root.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => path == self.repo_root.as_path(),
        }
    }

    pub fn ensure_worktree(
        &self,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        owner_pid: i64,
    ) -> Result<WorktreeLease, StateError> {
        let desired_worktree_path = self.worktree_path_for_ref(ref_name);
        if let Some(mut existing) = worktree_leases::get_lease(conn, repo, ref_name)? {
            if existing.status == "active" && existing.owner_pid != owner_pid {
                return Err(StateError::Sqlite(format!(
                    "worktree lease for ({repo}, {ref_name}) already held by pid {}",
                    existing.owner_pid
                )));
            }

            let existing_worktree_path = if existing.worktree_path.trim().is_empty() {
                desired_worktree_path
            } else {
                PathBuf::from(&existing.worktree_path)
            };
            if !existing_worktree_path.join(".git").exists() {
                self.adapter
                    .ensure_worktree(&self.repo_root, ref_name, &existing_worktree_path)
                    .map_err(StateError::vcs)?;
            }

            existing.worktree_path = self
                .active_checkout_path_for_ref(ref_name)
                .to_string_lossy()
                .to_string();
            existing.refcount += 1;
            existing.owner_pid = owner_pid;
            existing.status = "active".to_string();
            existing.last_used_at = now_iso8601();
            worktree_leases::create_lease(conn, &existing)?;
            return Ok(existing);
        }

        let worktree_path = self.worktree_path_for_ref(ref_name);
        self.adapter
            .ensure_worktree(&self.repo_root, ref_name, &worktree_path)
            .map_err(StateError::vcs)?;
        let active_checkout_path = self.active_checkout_path_for_ref(ref_name);

        let now = now_iso8601();
        let lease = WorktreeLease {
            repo: repo.to_string(),
            r#ref: ref_name.to_string(),
            worktree_path: active_checkout_path.to_string_lossy().to_string(),
            owner_pid,
            refcount: 1,
            status: "active".to_string(),
            created_at: now.clone(),
            last_used_at: now,
        };
        worktree_leases::create_lease(conn, &lease)?;
        Ok(lease)
    }

    pub fn release_lease(
        &self,
        conn: &Connection,
        repo: &str,
        ref_name: &str,
        owner_pid: i64,
    ) -> Result<(), StateError> {
        let Some(mut lease) = worktree_leases::get_lease(conn, repo, ref_name)? else {
            return Ok(());
        };

        if lease.owner_pid != owner_pid {
            return Err(StateError::Sqlite(format!(
                "lease owner mismatch for ({repo}, {ref_name}): expected {}, got {owner_pid}",
                lease.owner_pid
            )));
        }

        if lease.refcount <= 1 {
            lease.refcount = 0;
            lease.owner_pid = 0;
            lease.status = "stale".to_string();
            lease.last_used_at = now_iso8601();
            worktree_leases::create_lease(conn, &lease)?;
            return Ok(());
        }

        lease.refcount -= 1;
        lease.last_used_at = now_iso8601();
        worktree_leases::update_refcount(conn, repo, ref_name, lease.refcount, &lease.last_used_at)
    }

    pub fn cleanup_stale(
        &self,
        conn: &Connection,
        stale_before: &str,
    ) -> Result<Vec<String>, StateError> {
        let stale = worktree_leases::list_stale(conn, stale_before)?;
        let mut cleaned_refs = Vec::new();
        for lease in stale {
            let removing_at = now_iso8601();
            worktree_leases::update_status(
                conn,
                &lease.repo,
                &lease.r#ref,
                "removing",
                &removing_at,
            )?;
            let lease_path = PathBuf::from(&lease.worktree_path);
            if self.is_repo_root_path(&lease_path) {
                worktree_leases::delete_lease(conn, &lease.repo, &lease.r#ref)?;
                cleaned_refs.push(lease.r#ref);
                continue;
            }

            match std::fs::remove_dir_all(&lease_path) {
                Ok(()) => {
                    worktree_leases::delete_lease(conn, &lease.repo, &lease.r#ref)?;
                    cleaned_refs.push(lease.r#ref);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    worktree_leases::delete_lease(conn, &lease.repo, &lease.r#ref)?;
                    cleaned_refs.push(lease.r#ref);
                }
                Err(err) => {
                    warn!(
                        repo = %lease.repo,
                        ref_name = %lease.r#ref,
                        worktree_path = %lease.worktree_path,
                        error = %err,
                        "Failed to remove stale worktree directory; keeping lease row"
                    );
                }
            }
        }
        Ok(cleaned_refs)
    }
}

pub fn normalize_ref_name(ref_name: &str) -> String {
    let mut out = String::with_capacity(ref_name.len());
    for ch in ref_name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git2_adapter::Git2VcsAdapter;
    use cruxe_state::{db, schema};
    use git2::Repository;

    fn init_repo(path: &Path) -> Repository {
        let repo = Repository::init(path).unwrap();
        std::fs::write(path.join("lib.rs"), "fn main() {}\n").unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("lib.rs")).unwrap();
        let tree_id = idx.write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("test", "test@example.com").unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn normalize_ref_name_is_deterministic() {
        assert_eq!(normalize_ref_name("feat/auth#1"), "feat-auth-1");
        assert_eq!(normalize_ref_name("main"), "main");
    }

    #[test]
    fn lease_acquire_release_and_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo = init_repo(&repo_root);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/auth", &head, false).unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let worktrees_root = tmp.path().join("worktrees");
        let manager = WorktreeManager::new(&repo_root, &worktrees_root, Git2VcsAdapter);
        let lease = manager
            .ensure_worktree(&conn, "repo-1", "feat/auth", 1234)
            .unwrap();
        assert_eq!(lease.refcount, 1);
        assert!(Path::new(&lease.worktree_path).exists());

        let lease2 = manager
            .ensure_worktree(&conn, "repo-1", "feat/auth", 1234)
            .unwrap();
        assert_eq!(lease2.refcount, 2);

        manager
            .release_lease(&conn, "repo-1", "feat/auth", 1234)
            .unwrap();
        let current = worktree_leases::get_lease(&conn, "repo-1", "feat/auth")
            .unwrap()
            .unwrap();
        assert_eq!(current.refcount, 1);

        worktree_leases::update_status(
            &conn,
            "repo-1",
            "feat/auth",
            "stale",
            "2026-02-24T00:00:00Z",
        )
        .unwrap();
        let cleaned = manager
            .cleanup_stale(&conn, "2026-02-25T00:00:00Z")
            .unwrap();
        assert_eq!(cleaned, vec!["feat/auth".to_string()]);
        assert!(
            worktree_leases::get_lease(&conn, "repo-1", "feat/auth")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ensure_worktree_recreates_missing_path_for_existing_lease() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo = init_repo(&repo_root);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/auth", &head, false).unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let worktrees_root = tmp.path().join("worktrees");
        let manager = WorktreeManager::new(&repo_root, &worktrees_root, Git2VcsAdapter);
        let missing_worktree_path = manager.worktree_path_for_ref("feat/auth");

        worktree_leases::create_lease(
            &conn,
            &WorktreeLease {
                repo: "repo-1".to_string(),
                r#ref: "feat/auth".to_string(),
                worktree_path: missing_worktree_path.to_string_lossy().to_string(),
                owner_pid: 0,
                refcount: 0,
                status: "stale".to_string(),
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_used_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        assert!(
            !missing_worktree_path.exists(),
            "test setup expects missing worktree path"
        );

        let lease = manager
            .ensure_worktree(&conn, "repo-1", "feat/auth", 9001)
            .unwrap();
        assert_eq!(lease.refcount, 1);
        assert_eq!(lease.status, "active");
        assert!(Path::new(&lease.worktree_path).exists());
    }

    #[test]
    fn release_lease_to_zero_keeps_row_as_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo = init_repo(&repo_root);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feat/auth", &head, false).unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let worktrees_root = tmp.path().join("worktrees");
        let manager = WorktreeManager::new(&repo_root, &worktrees_root, Git2VcsAdapter);
        let _lease = manager
            .ensure_worktree(&conn, "repo-1", "feat/auth", 777)
            .unwrap();
        manager
            .release_lease(&conn, "repo-1", "feat/auth", 777)
            .unwrap();

        let lease = worktree_leases::get_lease(&conn, "repo-1", "feat/auth")
            .unwrap()
            .expect("lease should remain for stale cleanup");
        assert_eq!(lease.refcount, 0);
        assert_eq!(lease.status, "stale");
        assert_eq!(lease.owner_pid, 0);
    }

    #[test]
    fn ensure_worktree_uses_repo_root_path_when_ref_already_checked_out() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo = init_repo(&repo_root);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("main", &head, true).ok();
        repo.set_head("refs/heads/main").unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let worktrees_root = tmp.path().join("worktrees");
        let manager = WorktreeManager::new(&repo_root, &worktrees_root, Git2VcsAdapter);
        let lease = manager
            .ensure_worktree(&conn, "repo-1", "main", 42)
            .unwrap();
        assert_eq!(PathBuf::from(&lease.worktree_path), repo_root);
    }

    #[test]
    fn cleanup_stale_never_deletes_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let repo = init_repo(&repo_root);
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("main", &head, true).ok();

        let db_path = tmp.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        worktree_leases::create_lease(
            &conn,
            &WorktreeLease {
                repo: "repo-1".to_string(),
                r#ref: "main".to_string(),
                worktree_path: repo_root.to_string_lossy().to_string(),
                owner_pid: 0,
                refcount: 0,
                status: "stale".to_string(),
                created_at: "2026-02-25T00:00:00Z".to_string(),
                last_used_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let worktrees_root = tmp.path().join("worktrees");
        let manager = WorktreeManager::new(&repo_root, &worktrees_root, Git2VcsAdapter);
        let cleaned = manager
            .cleanup_stale(&conn, "2026-02-26T00:00:00Z")
            .unwrap();
        assert_eq!(cleaned, vec!["main".to_string()]);
        assert!(
            repo_root.exists(),
            "cleanup must not remove repository root"
        );
    }
}
