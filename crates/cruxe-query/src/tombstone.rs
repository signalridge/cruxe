use cruxe_core::error::StateError;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Query-scoped in-memory tombstone cache.
pub struct TombstoneCache<'a> {
    conn: Option<&'a Connection>,
    cache: HashMap<(String, String), HashSet<String>>,
}

impl<'a> TombstoneCache<'a> {
    pub fn new(conn: Option<&'a Connection>) -> Self {
        Self {
            conn,
            cache: HashMap::new(),
        }
    }

    /// Load tombstoned paths for `(repo, ref)` from SQLite.
    ///
    /// Results are memoized in-process for the cache lifetime.
    pub fn load_paths(
        &mut self,
        repo: &str,
        ref_name: &str,
    ) -> Result<&HashSet<String>, StateError> {
        let key = (repo.to_string(), ref_name.to_string());
        if !self.cache.contains_key(&key) {
            let paths = if let Some(conn) = self.conn {
                cruxe_state::tombstones::list_paths_for_ref(conn, repo, ref_name)?
                    .into_iter()
                    .collect::<HashSet<_>>()
            } else {
                HashSet::new()
            };
            self.cache.insert(key.clone(), paths);
        }

        // Safe due to insertion above.
        Ok(self.cache.get(&key).expect("cache key must exist"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::Project;
    use cruxe_state::{db, project, schema, tombstones};
    use tempfile::tempdir;

    #[test]
    fn load_paths_reads_and_caches_tombstones() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "repo-1".to_string(),
                repo_root: "/tmp/repo".to_string(),
                display_name: None,
                default_ref: "main".to_string(),
                vcs_mode: true,
                schema_version: 1,
                parser_version: 1,
                created_at: "2026-02-25T00:00:00Z".to_string(),
                updated_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        tombstones::create_tombstone(
            &conn,
            &tombstones::BranchTombstone {
                repo: "repo-1".to_string(),
                r#ref: "feat/auth".to_string(),
                path: "src/deleted.rs".to_string(),
                tombstone_type: "deleted".to_string(),
                created_at: "2026-02-25T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let mut cache = TombstoneCache::new(Some(&conn));
        let first = cache.load_paths("repo-1", "feat/auth").unwrap().clone();
        assert!(first.contains("src/deleted.rs"));

        // Second call should be served from cache and still return the same set.
        let second = cache.load_paths("repo-1", "feat/auth").unwrap().clone();
        assert_eq!(first, second);
    }

    #[test]
    fn load_paths_returns_empty_set_without_connection() {
        let mut cache = TombstoneCache::new(None);
        let paths = cache.load_paths("repo", "feat").unwrap();
        assert!(paths.is_empty());
    }
}
