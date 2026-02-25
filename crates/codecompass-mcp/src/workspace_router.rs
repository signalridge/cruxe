use codecompass_core::error::WorkspaceError;
use codecompass_core::types::{WorkspaceConfig, generate_project_id};
use std::path::{Path, PathBuf};

/// Workspace resolution result containing the resolved project context.
#[derive(Debug, Clone)]
pub struct ResolvedWorkspace {
    pub workspace_path: PathBuf,
    pub project_id: String,
    /// Whether query/status tools should surface "indexing" partial semantics.
    pub on_demand_indexing: bool,
    /// Whether this request should launch bootstrap indexing.
    pub should_bootstrap: bool,
}

/// Router that resolves workspace parameters to project contexts.
#[derive(Debug)]
pub struct WorkspaceRouter {
    config: WorkspaceConfig,
    default_workspace: PathBuf,
    default_project_id: String,
    db_path: PathBuf,
    data_root: PathBuf,
}

impl WorkspaceRouter {
    /// Create a new workspace router.
    ///
    /// # Errors
    /// Returns `AllowedRootRequired` if auto_workspace is enabled without allowed roots.
    pub fn new(
        config: WorkspaceConfig,
        default_workspace: PathBuf,
        db_path: PathBuf,
    ) -> Result<Self, WorkspaceError> {
        // T206: startup validation
        if config.auto_workspace && config.allowed_roots.is_empty() {
            return Err(WorkspaceError::AllowedRootRequired);
        }

        let default_project_id = generate_project_id(&default_workspace.to_string_lossy());
        let data_root = db_path
            .parent()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .ok_or_else(|| WorkspaceError::NotAllowed {
                path: db_path.display().to_string(),
                reason: "invalid state.db path; expected <data_root>/data/<project_id>/state.db"
                    .to_string(),
            })?;

        Ok(Self {
            config,
            default_workspace,
            default_project_id,
            db_path,
            data_root,
        })
    }

    /// Resolve a workspace parameter to a project context.
    ///
    /// Logic:
    /// 1. None → use default workspace
    /// 2. Known workspace → load project
    /// 3. Unknown + auto-workspace enabled → validate, register, trigger on-demand index
    /// 4. Unknown + auto-workspace disabled → error
    pub fn resolve_workspace(
        &self,
        workspace_param: Option<&str>,
    ) -> Result<ResolvedWorkspace, WorkspaceError> {
        let workspace_param = match workspace_param {
            Some(p) if !p.trim().is_empty() => p.trim(),
            _ => {
                // Case 1: Use default workspace
                return Ok(ResolvedWorkspace {
                    workspace_path: self.default_workspace.clone(),
                    project_id: self.default_project_id.clone(),
                    on_demand_indexing: false,
                    should_bootstrap: false,
                });
            }
        };

        let path = Path::new(workspace_param);

        // Canonicalize the path
        let canonical = std::fs::canonicalize(path).map_err(|e| WorkspaceError::NotAllowed {
            path: workspace_param.to_string(),
            reason: format!("path resolution failed: {e}"),
        })?;

        // If this is the default workspace (after canonicalization), return it directly
        if canonical == self.default_workspace {
            return Ok(ResolvedWorkspace {
                workspace_path: self.default_workspace.clone(),
                project_id: self.default_project_id.clone(),
                on_demand_indexing: false,
                should_bootstrap: false,
            });
        }

        // Check if workspace is known in DB
        let conn = codecompass_state::db::open_connection(&self.db_path).map_err(|e| {
            WorkspaceError::NotAllowed {
                path: canonical.display().to_string(),
                reason: format!("database error: {e}"),
            }
        })?;

        let canonical_str = canonical.to_string_lossy().to_string();

        if let Some(ws) = codecompass_state::workspace::get_workspace(&conn, &canonical_str)
            .map_err(|e| WorkspaceError::NotAllowed {
                path: canonical_str.clone(),
                reason: format!("database error: {e}"),
            })?
        {
            // Case 2: Known workspace
            // If a known workspace has no bound project yet, it is in bootstrap discovery
            // and callers should receive on-demand semantics even before the indexing claim
            // flips `index_status` to `indexing`.
            let on_demand_indexing = ws.project_id.is_none();
            let project_id = ws
                .project_id
                .unwrap_or_else(|| generate_project_id(&canonical_str));

            // Update last_used_at
            let now = codecompass_core::time::now_iso8601();
            let _ = codecompass_state::workspace::update_last_used(&conn, &canonical_str, &now);

            return Ok(ResolvedWorkspace {
                workspace_path: canonical,
                project_id,
                on_demand_indexing,
                should_bootstrap: false,
            });
        }

        // Case 3/4: Unknown workspace
        if !self.config.auto_workspace {
            return Err(WorkspaceError::NotRegistered {
                path: canonical_str,
            });
        }

        // Validate against allowed roots
        if !self.config.allowed_roots.contains(&canonical) {
            return Err(WorkspaceError::NotAllowed {
                path: canonical_str,
                reason: "path is outside all --allowed-root prefixes".to_string(),
            });
        }

        // T236: Evict LRU auto-discovered workspaces if at capacity
        match codecompass_state::workspace::evict_lru_auto_discovered(
            &conn,
            self.config.max_auto_workspaces.saturating_sub(1), // Make room for the new one
        ) {
            Ok(evicted) => {
                for path in &evicted {
                    let evicted_project_id = generate_project_id(path);
                    let evicted_data_dir = self.data_root.join(&evicted_project_id);
                    let mut cleaned_index_data = false;

                    if !evicted_data_dir.starts_with(&self.data_root) {
                        tracing::warn!(
                            evicted_workspace = %path,
                            project_id = %evicted_project_id,
                            data_dir = %evicted_data_dir.display(),
                            "Skipping index cleanup for evicted workspace: resolved path escapes data root"
                        );
                    } else if evicted_data_dir.exists() {
                        match std::fs::remove_dir_all(&evicted_data_dir) {
                            Ok(()) => {
                                cleaned_index_data = true;
                            }
                            Err(err) => tracing::warn!(
                                evicted_workspace = %path,
                                project_id = %evicted_project_id,
                                data_dir = %evicted_data_dir.display(),
                                "Evicted workspace entry but failed to clean index data: {}",
                                err
                            ),
                        }
                    }

                    tracing::info!(
                        evicted_workspace = %path,
                        project_id = %evicted_project_id,
                        cleaned_index_data,
                        "Evicted LRU auto-discovered workspace entry"
                    );
                }
            }
            Err(err) => {
                tracing::warn!("Failed to evict LRU auto-discovered workspaces: {}", err);
            }
        }

        // Register new workspace (UPSERT — safe for concurrent requests)
        let project_id = generate_project_id(&canonical_str);
        let now = codecompass_core::time::now_iso8601();

        codecompass_state::workspace::register_workspace(
            &conn,
            &canonical_str,
            None, // project_id is NULL until bootstrap_and_index creates the project
            true,
            &now,
        )
        .map_err(|e| WorkspaceError::NotAllowed {
            path: canonical_str.clone(),
            reason: format!("failed to register workspace: {e}"),
        })?;

        // HIGH-6: Claim bootstrap launch in DB so concurrent requests do not
        // spawn duplicate indexers for the same workspace.
        let should_bootstrap =
            codecompass_state::workspace::claim_bootstrap_indexing(&conn, &canonical_str, &now)
                .map_err(|e| WorkspaceError::NotAllowed {
                    path: canonical_str.clone(),
                    reason: format!("failed to claim workspace bootstrap: {e}"),
                })?;

        // If another request already bootstrapped this workspace, still return
        // indexing semantics for query tools (partial + retry guidance).
        if let Ok(Some(ws)) = codecompass_state::workspace::get_workspace(&conn, &canonical_str)
            && ws.project_id.is_some()
        {
            return Ok(ResolvedWorkspace {
                workspace_path: canonical,
                project_id: ws.project_id.unwrap(),
                on_demand_indexing: !should_bootstrap,
                should_bootstrap: false,
            });
        }

        // Case 3: Auto-discovered — signal that on-demand indexing should start
        Ok(ResolvedWorkspace {
            workspace_path: canonical,
            project_id,
            on_demand_indexing: true,
            should_bootstrap,
        })
    }

    pub fn default_workspace(&self) -> &Path {
        &self.default_workspace
    }

    pub fn default_project_id(&self) -> &str {
        &self.default_project_id
    }

    pub fn config(&self) -> &WorkspaceConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::types::AllowedRoots;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// Helper: set up a DB with schema at the given path.
    fn setup_db(db_path: &Path) {
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = codecompass_state::db::open_connection(db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();
    }

    /// Helper: create a project entry in the DB (satisfies FK constraints).
    fn create_project_in_db(db_path: &Path, project_id: &str, repo_root: &str) {
        let conn = codecompass_state::db::open_connection(db_path).unwrap();
        let now = codecompass_core::time::now_iso8601();
        let project = codecompass_core::types::Project {
            project_id: project_id.to_string(),
            repo_root: repo_root.to_string(),
            display_name: Some("test".to_string()),
            default_ref: "main".to_string(),
            vcs_mode: false,
            schema_version: 1,
            parser_version: 1,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();
    }

    /// Helper: register a workspace in the DB (project entry must exist first).
    fn register_workspace_in_db(db_path: &Path, workspace_path: &str, project_id: &str) {
        let conn = codecompass_state::db::open_connection(db_path).unwrap();
        let now = codecompass_core::time::now_iso8601();
        codecompass_state::workspace::register_workspace(
            &conn,
            workspace_path,
            Some(project_id),
            false,
            &now,
        )
        .unwrap();
    }

    // T206: startup validation — auto_workspace without allowed_roots fails
    #[test]
    fn startup_rejects_auto_workspace_without_allowed_roots() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::default(),
            max_auto_workspaces: 10,
        };
        let result = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorkspaceError::AllowedRootRequired
        ));
    }

    // T206: auto_workspace=false without allowed_roots is fine
    #[test]
    fn startup_allows_disabled_auto_workspace_without_roots() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let result = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path);
        assert!(result.is_ok());
    }

    // T209 (simplified): None workspace resolves to default
    #[test]
    fn resolve_none_returns_default_workspace() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let resolved = router.resolve_workspace(None).unwrap();
        assert_eq!(resolved.workspace_path, dir.path());
        assert!(!resolved.on_demand_indexing);
    }

    // T209: empty string workspace resolves to default
    #[test]
    fn resolve_empty_string_returns_default_workspace() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let resolved = router.resolve_workspace(Some("  ")).unwrap();
        assert_eq!(resolved.workspace_path, dir.path());
    }

    // T209: known workspace resolves to correct project_id
    #[test]
    fn resolve_known_workspace_returns_registered_project() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        // Create a second workspace directory
        let ws2 = dir.path().join("project_b");
        std::fs::create_dir_all(&ws2).unwrap();
        let ws2_canonical = std::fs::canonicalize(&ws2).unwrap();
        let ws2_str = ws2_canonical.to_string_lossy().to_string();

        // Register project + workspace in the DB
        create_project_in_db(&db_path, "proj-b", &ws2_str);
        register_workspace_in_db(&db_path, &ws2_str, "proj-b");

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let resolved = router.resolve_workspace(Some(&ws2_str)).unwrap();
        assert_eq!(resolved.workspace_path, ws2_canonical);
        assert_eq!(resolved.project_id, "proj-b");
        assert!(!resolved.on_demand_indexing);
    }

    // T210: auto-workspace discovers unknown workspace under allowed root
    #[test]
    fn resolve_auto_discovers_workspace_under_allowed_root() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        // Create workspace under allowed root
        let ws = dir.path().join("allowed/repo_x");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let allowed_root = std::fs::canonicalize(dir.path().join("allowed")).unwrap();
        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router =
            WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path.clone()).unwrap();

        let resolved = router.resolve_workspace(Some(&ws_str)).unwrap();
        assert_eq!(resolved.workspace_path, ws_canonical);
        assert!(resolved.on_demand_indexing);

        // Verify workspace was registered in DB
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        let ws_record = codecompass_state::workspace::get_workspace(&conn, &ws_str).unwrap();
        assert!(ws_record.is_some());
    }

    // T211: workspace outside allowed root returns error
    #[test]
    fn resolve_rejects_workspace_outside_allowed_root() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        // Create workspace outside allowed root
        let ws = dir.path().join("forbidden/repo_y");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let allowed_root = std::fs::canonicalize(dir.path().join("data")).unwrap();
        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let result = router.resolve_workspace(Some(&ws_str));
        assert!(result.is_err());
        match result.unwrap_err() {
            WorkspaceError::NotAllowed { path, reason } => {
                assert_eq!(path, ws_str);
                assert!(reason.contains("outside"));
            }
            other => panic!("expected NotAllowed, got: {:?}", other),
        }
    }

    // T212: unknown workspace with auto-workspace disabled returns NotRegistered
    #[test]
    fn resolve_rejects_unknown_workspace_when_auto_disabled() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let ws = dir.path().join("some_repo");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let config = WorkspaceConfig {
            auto_workspace: false,
            allowed_roots: AllowedRoots::default(),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let result = router.resolve_workspace(Some(&ws_str));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            WorkspaceError::NotRegistered { .. }
        ));
    }

    // T209: resolving default workspace via explicit path returns same result
    #[test]
    fn resolve_explicit_default_path_returns_default() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let default_ws = std::fs::canonicalize(dir.path()).unwrap();
        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, default_ws.clone(), db_path).unwrap();

        let default_str = default_ws.to_string_lossy().to_string();
        let resolved = router.resolve_workspace(Some(&default_str)).unwrap();
        assert_eq!(resolved.workspace_path, default_ws);
        assert_eq!(resolved.project_id, router.default_project_id());
        assert!(!resolved.on_demand_indexing);
    }

    // T210: auto-discovered workspace gets correct project_id
    #[test]
    fn auto_discovered_workspace_gets_deterministic_project_id() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let ws = dir.path().join("auto/myrepo");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let allowed_root = std::fs::canonicalize(dir.path().join("auto")).unwrap();
        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let resolved = router.resolve_workspace(Some(&ws_str)).unwrap();
        let expected_id = generate_project_id(&ws_str);
        assert_eq!(resolved.project_id, expected_id);
    }

    // Second resolve of same auto-discovered workspace reuses in-progress indexing
    // without launching bootstrap again.
    #[test]
    fn second_resolve_of_auto_discovered_reuses_indexing_without_bootstrap() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let ws = dir.path().join("auto2/repo_z");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let allowed_root = std::fs::canonicalize(dir.path().join("auto2")).unwrap();
        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        // First resolve: on-demand
        let r1 = router.resolve_workspace(Some(&ws_str)).unwrap();
        assert!(r1.on_demand_indexing);
        assert!(r1.should_bootstrap);

        // Second resolve: reuse in-progress indexing semantics
        let r2 = router.resolve_workspace(Some(&ws_str)).unwrap();
        assert!(r2.on_demand_indexing);
        assert!(!r2.should_bootstrap);
        assert_eq!(r1.project_id, r2.project_id);
    }

    // T239: Auto-discover 3 workspaces and verify known_workspaces entries + last_used updates.
    #[test]
    fn t239_auto_discover_three_workspaces_updates_known_workspaces() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let root = dir.path().join("auto-discover");
        let ws1 = root.join("repo-a");
        let ws2 = root.join("repo-b");
        let ws3 = root.join("repo-c");
        std::fs::create_dir_all(&ws1).unwrap();
        std::fs::create_dir_all(&ws2).unwrap();
        std::fs::create_dir_all(&ws3).unwrap();

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![std::fs::canonicalize(&root).unwrap()]),
            max_auto_workspaces: 10,
        };
        let router =
            WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path.clone()).unwrap();

        let ws1s = std::fs::canonicalize(&ws1)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let ws2s = std::fs::canonicalize(&ws2)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let ws3s = std::fs::canonicalize(&ws3)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let _ = router.resolve_workspace(Some(&ws1s)).unwrap();
        let _ = router.resolve_workspace(Some(&ws2s)).unwrap();
        let _ = router.resolve_workspace(Some(&ws3s)).unwrap();

        // Querying again should update last_used for the selected workspace.
        let _ = router.resolve_workspace(Some(&ws2s)).unwrap();

        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        let workspaces = codecompass_state::workspace::list_workspaces(&conn).unwrap();
        assert_eq!(workspaces.len(), 3, "should register 3 known workspaces");
        let mut paths: Vec<_> = workspaces
            .iter()
            .map(|w| w.workspace_path.clone())
            .collect();
        paths.sort();
        assert_eq!(paths, vec![ws1s.clone(), ws2s.clone(), ws3s.clone()]);

        let ws2_entry = workspaces
            .iter()
            .find(|w| w.workspace_path == ws2s)
            .expect("workspace-b should be present");
        assert!(
            !ws2_entry.last_used_at.is_empty(),
            "last_used_at should be populated for re-queried workspace"
        );
    }

    #[test]
    fn t460_lru_eviction_cleans_evicted_workspace_index_data() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let root = dir.path().join("auto-evict");
        let ws1 = root.join("repo-a");
        let ws2 = root.join("repo-b");
        let ws3 = root.join("repo-c");
        std::fs::create_dir_all(&ws1).unwrap();
        std::fs::create_dir_all(&ws2).unwrap();
        std::fs::create_dir_all(&ws3).unwrap();

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![std::fs::canonicalize(&root).unwrap()]),
            max_auto_workspaces: 2,
        };
        let router =
            WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path.clone()).unwrap();

        let ws1s = std::fs::canonicalize(&ws1)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let ws2s = std::fs::canonicalize(&ws2)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let ws3s = std::fs::canonicalize(&ws3)
            .unwrap()
            .to_string_lossy()
            .to_string();

        let _ = router.resolve_workspace(Some(&ws1s)).unwrap();
        let _ = router.resolve_workspace(Some(&ws2s)).unwrap();

        let ws1_pid = generate_project_id(&ws1s);
        let ws2_pid = generate_project_id(&ws2s);
        let ws1_data_dir = dir.path().join(&ws1_pid);
        let ws2_data_dir = dir.path().join(&ws2_pid);
        std::fs::create_dir_all(&ws1_data_dir).unwrap();
        std::fs::create_dir_all(&ws2_data_dir).unwrap();
        std::fs::write(ws1_data_dir.join("marker"), "evict-me").unwrap();
        std::fs::write(ws2_data_dir.join("marker"), "keep-me").unwrap();

        // Force deterministic LRU order: ws1 older than ws2
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::workspace::update_last_used(&conn, &ws1s, "2026-01-01T00:00:00Z")
            .unwrap();
        codecompass_state::workspace::update_last_used(&conn, &ws2s, "2026-01-02T00:00:00Z")
            .unwrap();

        let _ = router.resolve_workspace(Some(&ws3s)).unwrap();

        let workspaces = codecompass_state::workspace::list_workspaces(&conn).unwrap();
        let paths: Vec<_> = workspaces
            .iter()
            .map(|w| w.workspace_path.as_str())
            .collect();
        assert!(
            !paths.contains(&ws1s.as_str()),
            "LRU workspace should be evicted from known_workspaces"
        );
        assert!(paths.contains(&ws2s.as_str()));
        assert!(paths.contains(&ws3s.as_str()));

        assert!(
            !ws1_data_dir.exists(),
            "evicted workspace index data should be cleaned"
        );
        assert!(
            ws2_data_dir.exists(),
            "non-evicted workspace index data should remain"
        );
    }

    #[test]
    fn t461_concurrent_workspace_discovery_claims_single_bootstrap_launcher() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let root = dir.path().join("auto-concurrent");
        let ws = root.join("repo");
        std::fs::create_dir_all(&ws).unwrap();
        let ws_canonical = std::fs::canonicalize(&ws).unwrap();
        let ws_str = ws_canonical.to_string_lossy().to_string();

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![std::fs::canonicalize(&root).unwrap()]),
            max_auto_workspaces: 10,
        };
        let router = Arc::new(
            WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path.clone()).unwrap(),
        );

        let mut handles = Vec::new();
        for _ in 0..8 {
            let router = Arc::clone(&router);
            let ws_str = ws_str.clone();
            handles.push(std::thread::spawn(move || {
                router
                    .resolve_workspace(Some(&ws_str))
                    .expect("concurrent resolve should succeed")
            }));
        }

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let bootstrap_count = results.iter().filter(|r| r.should_bootstrap).count();
        assert_eq!(
            bootstrap_count, 1,
            "exactly one concurrent request should claim bootstrap launcher rights"
        );
        assert!(
            results.iter().all(|r| r.on_demand_indexing),
            "all concurrent requests should surface on-demand indexing semantics"
        );
        assert!(
            results
                .iter()
                .all(|r| r.project_id == results[0].project_id),
            "all concurrent requests should resolve to the same project id"
        );
    }

    // T457: Workspace routing smoke guard (strict p95 thresholds live in benchmark harness).
    #[test]
    fn t457_workspace_routing_overhead_smoke_guard() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();
        let mut samples = Vec::new();
        for _ in 0..200 {
            let started = std::time::Instant::now();
            let resolved = router.resolve_workspace(None).unwrap();
            assert_eq!(resolved.workspace_path, dir.path());
            samples.push(started.elapsed());
        }
        samples.sort();
        let p95 = samples[190];
        assert!(
            p95.as_millis() < 500,
            "workspace routing smoke budget should remain < 500ms, got {}ms",
            p95.as_millis()
        );
    }

    #[test]
    #[ignore = "benchmark harness"]
    fn benchmark_t457_workspace_routing_overhead_p95_under_5ms() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();
        let mut samples = Vec::new();
        for _ in 0..200 {
            let started = std::time::Instant::now();
            let resolved = router.resolve_workspace(None).unwrap();
            assert_eq!(resolved.workspace_path, dir.path());
            samples.push(started.elapsed());
        }
        samples.sort();
        let p95 = samples[190];
        assert!(
            p95.as_millis() < 5,
            "workspace routing benchmark p95 should be < 5ms, got {}ms",
            p95.as_millis()
        );
    }

    // Nonexistent path returns error
    #[test]
    fn resolve_nonexistent_path_returns_error() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig::default();
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let result = router.resolve_workspace(Some("/nonexistent/path/xyz"));
        assert!(result.is_err());
    }

    // ------ T233: Security tests for workspace path validation ------

    /// T233: Path traversal attack should be rejected (outside allowed roots)
    #[test]
    fn security_path_traversal_rejected() {
        let dir = tempdir().unwrap();
        let allowed_root = dir.path().join("allowed");
        std::fs::create_dir_all(&allowed_root).unwrap();

        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        // Traversal attempt: ../../../etc/passwd (non-existent as dir, canonicalize fails)
        let result = router.resolve_workspace(Some("../../../etc/passwd"));
        assert!(result.is_err());
    }

    /// T233: Relative path that resolves outside allowed root is rejected
    #[test]
    fn security_relative_path_outside_root_rejected() {
        let dir = tempdir().unwrap();
        let allowed_root = dir.path().join("projects");
        let outside = dir.path().join("secrets");
        std::fs::create_dir_all(&allowed_root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![allowed_root]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        // outside/ is a real directory but not under allowed_root
        let result = router.resolve_workspace(Some(&outside.to_string_lossy()));
        assert!(result.is_err());
    }

    /// T233: Null bytes in path should fail
    #[test]
    fn security_null_bytes_in_path_rejected() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![dir.path().to_path_buf()]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let result = router.resolve_workspace(Some("/tmp/foo\0bar"));
        assert!(result.is_err());
    }

    /// T233: Extremely long path should fail gracefully
    #[test]
    fn security_extremely_long_path_rejected() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![dir.path().to_path_buf()]),
            max_auto_workspaces: 10,
        };
        let router = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path).unwrap();

        let long_path = format!("/tmp/{}", "a".repeat(4096));
        let result = router.resolve_workspace(Some(&long_path));
        assert!(result.is_err());
    }

    /// T234: auto_workspace without allowed_root fails at startup
    #[test]
    fn security_auto_workspace_requires_allowed_root() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("data/state.db");
        setup_db(&db_path);

        let config = WorkspaceConfig {
            auto_workspace: true,
            allowed_roots: AllowedRoots::new(vec![]),
            max_auto_workspaces: 10,
        };
        let result = WorkspaceRouter::new(config, dir.path().to_path_buf(), db_path);
        assert!(result.is_err());
    }
}
