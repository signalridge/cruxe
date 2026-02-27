use cruxe_core::types::{FreshnessPolicy, FreshnessStatus};
use cruxe_indexer::scanner;
use rusqlite::Connection;
use std::path::Path;
use tracing::debug;

/// Detailed result of a freshness check.
#[derive(Debug, Clone)]
pub enum FreshnessResult {
    /// Index is up to date with HEAD.
    Fresh,
    /// Index is behind HEAD.
    Stale {
        last_indexed_commit: String,
        current_head: String,
    },
    /// A sync job is currently running.
    Syncing,
}

/// Action to take after applying freshness policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction {
    /// Proceed normally — index is fresh or status unknown.
    Proceed,
    /// Return results with a stale indicator in metadata.
    ProceedWithStaleIndicator,
    /// Return results with stale indicator AND trigger async sync.
    ProceedWithStaleIndicatorAndSync,
    /// Block the query and return an error.
    BlockWithError {
        last_indexed_commit: String,
        current_head: String,
    },
}

/// Check freshness of the index for a given project and ref.
///
/// Compares `branch_state.last_indexed_commit` to the current HEAD commit.
/// Returns `Fresh` when commits match, `Stale` when they diverge,
/// or `Syncing` when an indexing job is active.
pub fn check_freshness(
    conn: Option<&Connection>,
    workspace: &Path,
    project_id: &str,
    r#ref: &str,
) -> FreshnessResult {
    check_freshness_with_scan_params(
        conn,
        workspace,
        project_id,
        r#ref,
        cruxe_core::constants::MAX_FILE_SIZE,
        None,
    )
}

pub fn check_freshness_with_scan_params(
    conn: Option<&Connection>,
    workspace: &Path,
    project_id: &str,
    r#ref: &str,
    max_file_size: u64,
    languages: Option<&[String]>,
) -> FreshnessResult {
    let Some(conn) = conn else {
        // No DB connection — assume fresh (can't check)
        return FreshnessResult::Fresh;
    };

    // Check for active job first
    if let Ok(Some(_)) = cruxe_state::jobs::get_active_job(conn, project_id) {
        return FreshnessResult::Syncing;
    }

    let Ok(Some(branch_state)) =
        cruxe_state::branch_state::get_branch_state(conn, project_id, r#ref)
    else {
        // No branch state — never indexed, treat as fresh (not stale)
        return FreshnessResult::Fresh;
    };

    // Single-version mode: derive freshness from file_manifest snapshot cursor.
    if !cruxe_core::vcs::is_git_repo(workspace) {
        return check_single_version_freshness(
            conn,
            workspace,
            project_id,
            r#ref,
            &branch_state.last_indexed_commit,
            max_file_size,
            languages,
        );
    }

    let Ok(head_branch) = cruxe_core::vcs::detect_head_branch(workspace) else {
        return FreshnessResult::Fresh;
    };

    // Only compare when we're on the same branch
    if head_branch != r#ref {
        return FreshnessResult::Fresh;
    }

    let Ok(head_commit) = cruxe_core::vcs::detect_head_commit(workspace) else {
        return FreshnessResult::Fresh;
    };

    if branch_state.last_indexed_commit == head_commit {
        debug!(r#ref, head_commit, "freshness: fresh");
        FreshnessResult::Fresh
    } else {
        debug!(
            r#ref,
            last_indexed = branch_state.last_indexed_commit,
            head_commit,
            "freshness: stale"
        );
        FreshnessResult::Stale {
            last_indexed_commit: branch_state.last_indexed_commit,
            current_head: head_commit,
        }
    }
}

/// Lightweight single-version freshness check using only filesystem metadata.
///
/// Per spec: "The freshness check uses lightweight signals … manifest hash
/// cursor for single-version. It does not scan files."
///
/// Strategy:
/// 1. For each manifest entry, compare `size_bytes` via `fs::metadata()`.
///    If size differs or file is missing → stale.
/// 2. Re-scan indexable file paths (same scanner rules, language-filtered)
///    and compare the path set to `file_manifest`.
///
/// No file contents are read (no `fs::read`). Total cost: O(manifest_entries)
/// metadata syscalls + one path-only workspace scan.
fn check_single_version_freshness(
    conn: &Connection,
    workspace: &Path,
    project_id: &str,
    r#ref: &str,
    fallback_last_indexed: &str,
    max_file_size: u64,
    languages: Option<&[String]>,
) -> FreshnessResult {
    if !workspace.exists() {
        return FreshnessResult::Fresh;
    }

    let Ok(entries) = cruxe_state::manifest::get_all_entries(conn, project_id, r#ref) else {
        return FreshnessResult::Fresh;
    };
    if entries.is_empty() {
        return FreshnessResult::Fresh;
    }

    // Phase 1: Check existing manifest entries via metadata only (no fs::read).
    let mut manifest_paths = std::collections::HashSet::new();
    let mut indexed_languages = std::collections::HashSet::new();

    for entry in &entries {
        manifest_paths.insert(entry.path.clone());
        if let Some(lang) = &entry.language {
            indexed_languages.insert(lang.clone());
        }

        let full_path = workspace.join(&entry.path);
        let Ok(metadata) = std::fs::metadata(&full_path) else {
            // File deleted since indexing → stale.
            debug!(path = %entry.path, "freshness: file missing");
            return FreshnessResult::Stale {
                last_indexed_commit: fallback_last_indexed.to_string(),
                current_head: "metadata_changed".to_string(),
            };
        };

        if metadata.len() != entry.size_bytes {
            debug!(
                path = %entry.path,
                indexed_size = entry.size_bytes,
                current_size = metadata.len(),
                "freshness: size mismatch"
            );
            return FreshnessResult::Stale {
                last_indexed_commit: fallback_last_indexed.to_string(),
                current_head: "metadata_changed".to_string(),
            };
        }

        // Check mtime if stored (currently unused by indexer, but future-proof).
        if let Some(expected_mtime) = entry.mtime_ns {
            let current_mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64);
            if current_mtime != Some(expected_mtime) {
                debug!(path = %entry.path, "freshness: mtime mismatch");
                return FreshnessResult::Stale {
                    last_indexed_commit: fallback_last_indexed.to_string(),
                    current_head: "metadata_changed".to_string(),
                };
            }
        }
    }

    // Phase 2: Detect indexable-file set drift.
    // Use the same scanner rules as indexing (gitignore/.cruxeignore + language detection)
    // while filtering to languages that were actually indexed for this ref.
    let mut language_filter: Vec<String> = match languages {
        Some(configured) if !configured.is_empty() => configured.to_vec(),
        _ => indexed_languages.into_iter().collect(),
    };
    language_filter.sort();
    let scanned = if language_filter.is_empty() {
        scanner::scan_directory(workspace, max_file_size)
    } else {
        scanner::scan_directory_filtered(workspace, max_file_size, &language_filter)
    };
    let scanned_paths: std::collections::HashSet<String> =
        scanned.into_iter().map(|f| f.relative_path).collect();

    let new_indexable_paths: Vec<&str> = scanned_paths
        .iter()
        .filter(|p| !manifest_paths.contains(*p))
        .map(|p| p.as_str())
        .collect();
    if !new_indexable_paths.is_empty() {
        debug!(
            new_paths = new_indexable_paths.len(),
            "freshness: new indexable files detected"
        );
        return FreshnessResult::Stale {
            last_indexed_commit: fallback_last_indexed.to_string(),
            current_head: "manifest_drift".to_string(),
        };
    }

    FreshnessResult::Fresh
}

/// Map a `FreshnessResult` to a `FreshnessStatus` enum for protocol metadata.
pub fn freshness_status(result: &FreshnessResult) -> FreshnessStatus {
    match result {
        FreshnessResult::Fresh => FreshnessStatus::Fresh,
        FreshnessResult::Stale { .. } => FreshnessStatus::Stale,
        FreshnessResult::Syncing => FreshnessStatus::Syncing,
    }
}

/// Apply freshness policy to determine the action to take.
pub fn apply_freshness_policy(
    policy: FreshnessPolicy,
    freshness: &FreshnessResult,
) -> PolicyAction {
    match (policy, freshness) {
        // Fresh — always proceed normally
        (_, FreshnessResult::Fresh) => PolicyAction::Proceed,

        // Syncing — proceed with stale indicator regardless of policy
        (_, FreshnessResult::Syncing) => PolicyAction::ProceedWithStaleIndicator,

        // Strict + Stale — block
        (
            FreshnessPolicy::Strict,
            FreshnessResult::Stale {
                last_indexed_commit,
                current_head,
            },
        ) => PolicyAction::BlockWithError {
            last_indexed_commit: last_indexed_commit.clone(),
            current_head: current_head.clone(),
        },

        // Balanced + Stale — proceed with stale indicator + trigger async sync
        (FreshnessPolicy::Balanced, FreshnessResult::Stale { .. }) => {
            PolicyAction::ProceedWithStaleIndicatorAndSync
        }

        // BestEffort + Stale — proceed with stale indicator, no sync
        (FreshnessPolicy::BestEffort, FreshnessResult::Stale { .. }) => {
            PolicyAction::ProceedWithStaleIndicator
        }
    }
}

/// Trigger an asynchronous sync by spawning a background indexer process.
///
/// Used by the `balanced` policy when staleness is detected.
/// The sync runs in a separate process so it doesn't block the current query.
pub fn trigger_async_sync(workspace: &Path, r#ref: &str) {
    let exe = std::env::current_exe().unwrap_or_else(|_| "cruxe".into());
    let workspace_str = workspace.to_string_lossy().to_string();
    let ref_str = r#ref.to_string();

    std::thread::spawn(move || {
        let result = std::process::Command::new(exe)
            .arg("index")
            .arg("--path")
            .arg(&workspace_str)
            .arg("--ref")
            .arg(&ref_str)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => {
                tracing::warn!("Failed to spawn async sync: {}", e);
            }
        }
    });
}

/// Parse a freshness policy string into the enum, with fallback to Balanced.
pub fn parse_freshness_policy(s: &str) -> FreshnessPolicy {
    match s {
        "strict" => FreshnessPolicy::Strict,
        "balanced" => FreshnessPolicy::Balanced,
        "best_effort" => FreshnessPolicy::BestEffort,
        _ => FreshnessPolicy::Balanced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::types::Project;

    #[test]
    fn test_apply_freshness_policy_strict_stale_blocks() {
        let stale = FreshnessResult::Stale {
            last_indexed_commit: "abc123".into(),
            current_head: "def456".into(),
        };
        let action = apply_freshness_policy(FreshnessPolicy::Strict, &stale);
        assert_eq!(
            action,
            PolicyAction::BlockWithError {
                last_indexed_commit: "abc123".into(),
                current_head: "def456".into(),
            }
        );
    }

    #[test]
    fn test_apply_freshness_policy_balanced_stale_proceeds_with_sync() {
        let stale = FreshnessResult::Stale {
            last_indexed_commit: "abc123".into(),
            current_head: "def456".into(),
        };
        let action = apply_freshness_policy(FreshnessPolicy::Balanced, &stale);
        assert_eq!(action, PolicyAction::ProceedWithStaleIndicatorAndSync);
    }

    #[test]
    fn test_apply_freshness_policy_best_effort_stale_proceeds() {
        let stale = FreshnessResult::Stale {
            last_indexed_commit: "abc123".into(),
            current_head: "def456".into(),
        };
        let action = apply_freshness_policy(FreshnessPolicy::BestEffort, &stale);
        assert_eq!(action, PolicyAction::ProceedWithStaleIndicator);
    }

    #[test]
    fn test_apply_freshness_policy_fresh_always_proceeds() {
        for policy in [
            FreshnessPolicy::Strict,
            FreshnessPolicy::Balanced,
            FreshnessPolicy::BestEffort,
        ] {
            let action = apply_freshness_policy(policy, &FreshnessResult::Fresh);
            assert_eq!(action, PolicyAction::Proceed);
        }
    }

    #[test]
    fn test_apply_freshness_policy_syncing_proceeds_with_indicator() {
        for policy in [
            FreshnessPolicy::Strict,
            FreshnessPolicy::Balanced,
            FreshnessPolicy::BestEffort,
        ] {
            let action = apply_freshness_policy(policy, &FreshnessResult::Syncing);
            assert_eq!(action, PolicyAction::ProceedWithStaleIndicator);
        }
    }

    #[test]
    fn test_freshness_status_mapping() {
        assert_eq!(
            freshness_status(&FreshnessResult::Fresh),
            FreshnessStatus::Fresh
        );
        assert_eq!(
            freshness_status(&FreshnessResult::Stale {
                last_indexed_commit: "a".into(),
                current_head: "b".into(),
            }),
            FreshnessStatus::Stale
        );
        assert_eq!(
            freshness_status(&FreshnessResult::Syncing),
            FreshnessStatus::Syncing
        );
    }

    #[test]
    fn test_parse_freshness_policy() {
        assert_eq!(parse_freshness_policy("strict"), FreshnessPolicy::Strict);
        assert_eq!(
            parse_freshness_policy("balanced"),
            FreshnessPolicy::Balanced
        );
        assert_eq!(
            parse_freshness_policy("best_effort"),
            FreshnessPolicy::BestEffort
        );
        assert_eq!(parse_freshness_policy("unknown"), FreshnessPolicy::Balanced);
    }

    #[test]
    fn test_check_freshness_single_version_detects_file_change() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let src = workspace.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file_path = src.join("lib.rs");
        std::fs::write(&file_path, "pub fn v() -> i32 { 1 }\n").unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = cruxe_state::db::open_connection(&db_path).unwrap();
        cruxe_state::schema::create_tables(&conn).unwrap();

        let project_id = "proj_single";
        let project = Project {
            project_id: project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("single".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        cruxe_state::project::create_project(&conn, &project).unwrap();

        let content_hash = blake3::hash(std::fs::read(&file_path).unwrap().as_slice())
            .to_hex()
            .to_string();
        cruxe_state::manifest::upsert_manifest(
            &conn,
            &cruxe_state::manifest::ManifestEntry {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                path: "src/lib.rs".to_string(),
                content_hash,
                size_bytes: std::fs::metadata(&file_path).unwrap().len(),
                mtime_ns: std::fs::metadata(&file_path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i64),
                language: Some("rust".to_string()),
                indexed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        cruxe_state::branch_state::upsert_branch_state(
            &conn,
            &cruxe_state::branch_state::BranchState {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "live".to_string(),
                overlay_dir: None,
                file_count: 1,
                symbol_count: 0,
                is_default_branch: true,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                last_accessed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        // Fresh before mutation.
        let fresh = check_freshness(Some(&conn), &workspace, project_id, "live");
        assert!(matches!(fresh, FreshnessResult::Fresh));

        // Mutate file without changing size and ensure mtime moves.
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::fs::write(&file_path, "pub fn v() -> i32 { 2 }\n").unwrap();
        let stale = check_freshness(Some(&conn), &workspace, project_id, "live");
        assert!(matches!(stale, FreshnessResult::Stale { .. }));
    }

    #[test]
    fn test_check_freshness_single_version_detects_new_source_file() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file_path = workspace.join("src/lib.rs");
        std::fs::write(&file_path, "pub fn v() -> i32 { 1 }\n").unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = cruxe_state::db::open_connection(&db_path).unwrap();
        cruxe_state::schema::create_tables(&conn).unwrap();

        let project_id = "proj_single_new_file";
        let project = Project {
            project_id: project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("single".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        cruxe_state::project::create_project(&conn, &project).unwrap();

        cruxe_state::manifest::upsert_manifest(
            &conn,
            &cruxe_state::manifest::ManifestEntry {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                path: "src/lib.rs".to_string(),
                content_hash: blake3::hash(std::fs::read(&file_path).unwrap().as_slice())
                    .to_hex()
                    .to_string(),
                size_bytes: std::fs::metadata(&file_path).unwrap().len(),
                mtime_ns: None,
                language: Some("rust".to_string()),
                indexed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        cruxe_state::branch_state::upsert_branch_state(
            &conn,
            &cruxe_state::branch_state::BranchState {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "live".to_string(),
                overlay_dir: None,
                file_count: 1,
                symbol_count: 0,
                is_default_branch: true,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                last_accessed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let fresh = check_freshness(Some(&conn), &workspace, project_id, "live");
        assert!(matches!(fresh, FreshnessResult::Fresh));

        std::fs::write(workspace.join("src/new_file.rs"), "pub fn n() {}\n").unwrap();
        let stale = check_freshness(Some(&conn), &workspace, project_id, "live");
        assert!(matches!(stale, FreshnessResult::Stale { .. }));
    }

    #[test]
    fn test_check_freshness_single_version_ignores_non_indexed_file_types() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file_path = workspace.join("src/lib.rs");
        std::fs::write(&file_path, "pub fn v() -> i32 { 1 }\n").unwrap();

        let db_path = tmp.path().join("state.db");
        let conn = cruxe_state::db::open_connection(&db_path).unwrap();
        cruxe_state::schema::create_tables(&conn).unwrap();

        let project_id = "proj_single_ignore_docs";
        let project = Project {
            project_id: project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("single".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        cruxe_state::project::create_project(&conn, &project).unwrap();

        cruxe_state::manifest::upsert_manifest(
            &conn,
            &cruxe_state::manifest::ManifestEntry {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                path: "src/lib.rs".to_string(),
                content_hash: blake3::hash(std::fs::read(&file_path).unwrap().as_slice())
                    .to_hex()
                    .to_string(),
                size_bytes: std::fs::metadata(&file_path).unwrap().len(),
                mtime_ns: None,
                language: Some("rust".to_string()),
                indexed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        cruxe_state::branch_state::upsert_branch_state(
            &conn,
            &cruxe_state::branch_state::BranchState {
                repo: project_id.to_string(),
                r#ref: "live".to_string(),
                merge_base_commit: None,
                last_indexed_commit: "live".to_string(),
                overlay_dir: None,
                file_count: 1,
                symbol_count: 0,
                is_default_branch: true,
                status: "active".to_string(),
                eviction_eligible_at: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                last_accessed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        std::fs::create_dir_all(workspace.join("docs")).unwrap();
        std::fs::write(workspace.join("docs/README.md"), "# doc only\n").unwrap();

        let freshness = check_freshness(Some(&conn), &workspace, project_id, "live");
        assert!(
            matches!(freshness, FreshnessResult::Fresh),
            "non-indexed file types should not mark index stale"
        );
    }
}
