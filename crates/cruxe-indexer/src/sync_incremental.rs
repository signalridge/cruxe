use cruxe_core::config::{Config, SemanticConfig};
use cruxe_core::error::{StateError, VcsError};
use cruxe_core::ids::new_job_id;
use cruxe_core::time::now_iso8601;
use cruxe_core::types::JobStatus;
use cruxe_state::branch_state::{self, BranchState};
use cruxe_state::jobs;
use cruxe_state::tombstones::BranchTombstone;
use cruxe_vcs::{DiffEntry, FileChangeKind, VcsAdapter, WorktreeManager};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::warn;

use crate::{call_extract, embed_writer, parser, prepare, staging, writer};

/// Per-file action derived from `git diff --name-status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    Added { path: String },
    Modified { path: String },
    Deleted { path: String },
}

impl SyncAction {
    pub fn path(&self) -> &str {
        match self {
            Self::Added { path } | Self::Modified { path } | Self::Deleted { path } => path,
        }
    }
}

/// Computed incremental sync plan for a ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub merge_base_commit: String,
    pub head_commit: String,
    pub actions: Vec<SyncAction>,
}

/// Expand VCS diff entries into per-file sync actions.
///
/// Rename entries are expanded into:
/// - `Deleted { old_path }`
/// - `Added { new_path }`
pub fn expand_diff_entries(entries: &[DiffEntry]) -> Vec<SyncAction> {
    let mut actions = Vec::new();
    for entry in entries {
        match &entry.kind {
            FileChangeKind::Added => {
                actions.push(SyncAction::Added {
                    path: entry.path.clone(),
                });
            }
            FileChangeKind::Modified => {
                actions.push(SyncAction::Modified {
                    path: entry.path.clone(),
                });
            }
            FileChangeKind::Deleted => {
                actions.push(SyncAction::Deleted {
                    path: entry.path.clone(),
                });
            }
            FileChangeKind::Renamed { old_path } => {
                actions.push(SyncAction::Deleted {
                    path: old_path.clone(),
                });
                actions.push(SyncAction::Added {
                    path: entry.path.clone(),
                });
            }
        }
    }
    actions
}

/// Build incremental sync plan by computing merge-base and diff name-status.
pub fn build_sync_plan<A>(
    adapter: &A,
    repo_root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<SyncPlan, VcsError>
where
    A: VcsAdapter<FileChange = FileChangeKind, DiffEntry = DiffEntry>,
{
    let merge_base_commit = adapter.merge_base(repo_root, base_ref, head_ref)?;
    let head_commit = adapter.resolve_head(repo_root)?;
    let diff_entries = adapter.diff_name_status(repo_root, &merge_base_commit, head_ref)?;
    let actions = expand_diff_entries(&diff_entries);
    Ok(SyncPlan {
        merge_base_commit,
        head_commit,
        actions,
    })
}

/// Returns `true` when ancestry is broken and overlay rebuild is required.
pub fn should_rebuild_overlay<A>(
    adapter: &A,
    repo_root: &Path,
    last_indexed_commit: &str,
    head_commit: &str,
) -> Result<bool, VcsError>
where
    A: VcsAdapter<FileChange = FileChangeKind, DiffEntry = DiffEntry>,
{
    Ok(!adapter.is_ancestor(repo_root, last_indexed_commit, head_commit)?)
}

/// Convert sync actions into tombstones for deleted/replaced suppression.
pub fn build_tombstones(
    repo: &str,
    ref_name: &str,
    actions: &[SyncAction],
) -> Vec<BranchTombstone> {
    let created_at = now_iso8601();
    actions
        .iter()
        .filter_map(|action| match action {
            SyncAction::Deleted { path } => Some(BranchTombstone {
                repo: repo.to_string(),
                r#ref: ref_name.to_string(),
                path: path.clone(),
                tombstone_type: "deleted".to_string(),
                created_at: created_at.clone(),
            }),
            SyncAction::Modified { path } => Some(BranchTombstone {
                repo: repo.to_string(),
                r#ref: ref_name.to_string(),
                path: path.clone(),
                tombstone_type: "replaced".to_string(),
                created_at: created_at.clone(),
            }),
            SyncAction::Added { .. } => None,
        })
        .collect()
}

/// Persist tombstones derived from sync actions.
pub fn apply_tombstones_for_actions(
    conn: &Connection,
    repo: &str,
    ref_name: &str,
    actions: &[SyncAction],
) -> Result<(), StateError> {
    // `build_sync_plan` computes the full branch delta against merge-base, so tombstones
    // must mirror the current delta exactly. Replacing per-ref tombstones avoids stale
    // suppressions when a file is later reverted back to base.
    cruxe_state::tombstones::delete_for_ref(conn, repo, ref_name)?;

    let tombstones = build_tombstones(repo, ref_name, actions);
    if tombstones.is_empty() {
        return Ok(());
    }
    for tombstone in &tombstones {
        cruxe_state::tombstones::create_tombstone(conn, tombstone)?;
    }
    Ok(())
}

/// Enforce single active sync per `(project_id, ref)`.
pub fn ensure_no_active_sync_for_ref(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
) -> Result<(), StateError> {
    if let Some(active) = jobs::get_active_job_for_ref(conn, project_id, ref_name)? {
        return Err(StateError::sync_in_progress(
            project_id,
            ref_name,
            active.job_id,
        ));
    }
    Ok(())
}

/// Rebuild overlay directory from scratch by discarding current contents.
pub fn rebuild_overlay_directory(data_dir: &Path, ref_name: &str) -> Result<(), StateError> {
    crate::overlay::delete_overlay_dir(data_dir, ref_name)?;
    let _ = crate::overlay::create_overlay_index_set(data_dir, ref_name)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSyncStateUpdate {
    pub repo: String,
    pub ref_name: String,
    pub merge_base_commit: Option<String>,
    pub last_indexed_commit: String,
    pub overlay_dir: Option<String>,
    pub file_count: i64,
    pub symbol_count: i64,
    pub is_default_branch: bool,
}

/// Persist branch state after a successful sync.
pub fn persist_branch_sync_state(
    conn: &Connection,
    update: &BranchSyncStateUpdate,
) -> Result<(), StateError> {
    let now = now_iso8601();
    let entry = BranchState {
        repo: update.repo.clone(),
        r#ref: update.ref_name.clone(),
        merge_base_commit: update.merge_base_commit.clone(),
        last_indexed_commit: update.last_indexed_commit.clone(),
        overlay_dir: update.overlay_dir.clone(),
        file_count: update.file_count,
        symbol_count: update.symbol_count,
        is_default_branch: update.is_default_branch,
        status: "active".to_string(),
        eviction_eligible_at: None,
        created_at: now.clone(),
        last_accessed_at: now,
    };
    branch_state::upsert_branch_state(conn, &entry)
}

/// Create a running sync job with `sync_id`.
pub fn create_sync_job(
    conn: &Connection,
    project_id: &str,
    ref_name: &str,
    mode: &str,
    head_commit: Option<String>,
    sync_id: &str,
) -> Result<String, StateError> {
    let job_id = new_job_id();
    let now = now_iso8601();
    let job = jobs::IndexJob {
        job_id: job_id.clone(),
        project_id: project_id.to_string(),
        r#ref: ref_name.to_string(),
        mode: mode.to_string(),
        head_commit,
        sync_id: Some(sync_id.to_string()),
        status: JobStatus::Running.as_str().to_string(),
        changed_files: 0,
        duration_ms: None,
        error_message: None,
        retry_count: 0,
        progress_token: Some(format!("index-job-{job_id}")),
        files_scanned: 0,
        files_indexed: 0,
        symbols_extracted: 0,
        created_at: now.clone(),
        updated_at: now,
    };
    jobs::create_job(conn, &job)?;
    Ok(job_id)
}

/// Mark sync job as published.
pub fn mark_sync_job_published(
    conn: &Connection,
    job_id: &str,
    changed_files: i64,
    duration_ms: i64,
) -> Result<(), StateError> {
    jobs::update_job_status(
        conn,
        job_id,
        JobStatus::Published,
        Some(changed_files),
        Some(duration_ms),
        None,
        &now_iso8601(),
    )
}

/// Mark sync job as rolled back after failure.
pub fn mark_sync_job_rolled_back(
    conn: &Connection,
    job_id: &str,
    duration_ms: i64,
    error_message: &str,
) -> Result<(), StateError> {
    jobs::update_job_status(
        conn,
        job_id,
        JobStatus::RolledBack,
        None,
        Some(duration_ms),
        Some(error_message),
        &now_iso8601(),
    )
}

pub struct IncrementalSyncRequest<'a> {
    pub repo_root: &'a Path,
    pub data_dir: &'a Path,
    pub project_id: &'a str,
    pub ref_name: &'a str,
    pub base_ref: &'a str,
    pub sync_id: &'a str,
    pub last_indexed_commit: Option<&'a str>,
    pub is_default_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalSyncStats {
    pub changed_files: usize,
    pub processed_files: usize,
    pub symbols_written: usize,
    pub rebuild_triggered: bool,
    pub overlay_dir: PathBuf,
    pub merge_base_commit: String,
    pub head_commit: String,
}

fn file_mtime_ns(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
}

fn default_worktrees_root(data_dir: &Path, project_id: &str) -> PathBuf {
    let storage_root = data_dir
        .file_name()
        .and_then(|name| name.to_str())
        .zip(data_dir.parent())
        .and_then(|(leaf, parent)| {
            if leaf != project_id {
                return None;
            }
            if parent.file_name().and_then(|name| name.to_str()) != Some("data") {
                return None;
            }
            parent.parent().map(Path::to_path_buf)
        })
        .unwrap_or_else(|| data_dir.to_path_buf());
    storage_root.join("worktrees").join(project_id)
}

fn write_actions_to_staging(
    conn: &Connection,
    index_set: &cruxe_state::tantivy_index::IndexSet,
    repo_root: &Path,
    project_id: &str,
    ref_name: &str,
    actions: &[SyncAction],
    semantic: &SemanticConfig,
) -> Result<(usize, usize, Vec<SyncAction>), StateError> {
    write_actions_to_staging_with_parser(
        StagingWriteContext {
            conn,
            index_set,
            repo_root,
            project_id,
            ref_name,
            actions,
            semantic,
        },
        |content, language| parser::parse_file(content, language).map_err(|err| err.to_string()),
    )
}

struct StagingWriteContext<'a> {
    conn: &'a Connection,
    index_set: &'a cruxe_state::tantivy_index::IndexSet,
    repo_root: &'a Path,
    project_id: &'a str,
    ref_name: &'a str,
    actions: &'a [SyncAction],
    semantic: &'a SemanticConfig,
}

fn write_actions_to_staging_with_parser<F>(
    ctx: StagingWriteContext<'_>,
    mut parse_changed_file: F,
) -> Result<(usize, usize, Vec<SyncAction>), StateError>
where
    F: FnMut(&str, &str) -> Result<tree_sitter::Tree, String>,
{
    let StagingWriteContext {
        conn,
        index_set,
        repo_root,
        project_id,
        ref_name,
        actions,
        semantic,
    } = ctx;

    let batch = writer::BatchWriter::new(index_set)?;
    let mut embedding_writer = embed_writer::EmbeddingWriter::new(semantic, project_id, ref_name)?;
    let mut processed_files = 0usize;
    let mut symbols_written = 0usize;
    let mut applied_actions = Vec::with_capacity(actions.len());
    let mut pending_call_edges: Vec<(String, Vec<cruxe_core::types::CallEdge>)> = Vec::new();
    let mut pending_embedding_batches: Vec<(
        Vec<cruxe_core::types::SymbolRecord>,
        Vec<cruxe_core::types::SnippetRecord>,
    )> = Vec::new();
    let embedding_enabled = embedding_writer.enabled();

    for action in actions {
        let path = action.path();
        match action {
            SyncAction::Deleted { .. } => {
                // Keep SQLite side consistent with the staged overlay snapshot:
                // deleted files must not leave stale symbols/manifest/import edges behind.
                let deleted_symbol_ids: Vec<String> =
                    cruxe_state::symbols::list_symbols_in_file(conn, project_id, ref_name, path)?
                        .into_iter()
                        .map(|symbol| symbol.symbol_stable_id)
                        .collect();
                cruxe_state::symbols::delete_symbols_for_file(conn, project_id, ref_name, path)?;
                cruxe_state::manifest::delete_manifest(conn, project_id, ref_name, path)?;
                let source_edge_id = crate::import_extract::source_symbol_id_for_path(path);
                cruxe_state::edges::delete_edges_for_file(
                    conn,
                    project_id,
                    ref_name,
                    vec![source_edge_id.as_str()],
                )?;
                cruxe_state::edges::delete_call_edges_for_file(conn, project_id, ref_name, path)?;
                cruxe_state::edges::delete_call_edges_to_symbols(
                    conn,
                    project_id,
                    ref_name,
                    &deleted_symbol_ids,
                )?;
                embedding_writer.delete_for_file_vectors_with_symbols(
                    conn,
                    path,
                    &deleted_symbol_ids,
                )?;
                applied_actions.push(action.clone());
                continue;
            }
            SyncAction::Added { .. } | SyncAction::Modified { .. } => {
                let is_modified = matches!(action, SyncAction::Modified { .. });
                let full_path = repo_root.join(path);
                let content = match std::fs::read_to_string(&full_path) {
                    Ok(c) => c,
                    Err(err) => {
                        return Err(StateError::Io(std::io::Error::new(
                            err.kind(),
                            format!("failed to read changed file {path}: {err}"),
                        )));
                    }
                };
                let language = match crate::scanner::detect_language(&full_path) {
                    Some(lang) => lang,
                    None => {
                        warn!(path, "Skipping changed file with unsupported language");
                        continue;
                    }
                };
                let artifacts = prepare::build_source_artifacts_with_parser(
                    prepare::ArtifactBuildInput {
                        content: &content,
                        language: &language,
                        source_path: path,
                        project_id,
                        ref_name,
                        source_layer: Some("overlay"),
                        include_imports: false,
                    },
                    &mut parse_changed_file,
                );
                if let Some(err) = artifacts.parse_error.as_deref() {
                    warn!(
                        path,
                        error = %err,
                        "Parse failed for changed file; continuing with metadata-only update"
                    );
                }

                let filename = full_path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let file = prepare::build_file_record(
                    project_id, ref_name, path, &filename, &language, &content,
                );

                if is_modified && embedding_enabled {
                    embedding_writer.delete_for_file_vectors(conn, path)?;
                }

                cruxe_state::symbols::delete_symbols_for_file(conn, project_id, ref_name, path)?;
                batch.add_symbols(&index_set.symbols, &artifacts.symbols)?;
                batch.add_snippets(&index_set.snippets, &artifacts.snippets)?;
                batch.add_file(&index_set.files, &file)?;
                batch.write_sqlite(conn, &artifacts.symbols, &file, file_mtime_ns(&full_path))?;
                if is_modified || !artifacts.call_edges.is_empty() {
                    pending_call_edges.push((path.to_string(), artifacts.call_edges));
                }
                let symbol_len = artifacts.symbols.len();
                if embedding_enabled && !artifacts.snippets.is_empty() {
                    pending_embedding_batches.push((artifacts.symbols, artifacts.snippets));
                }

                processed_files += 1;
                symbols_written += symbol_len;
                applied_actions.push(action.clone());
            }
        }
    }

    if !pending_call_edges.is_empty() {
        if pending_call_edges
            .iter()
            .any(|(_, call_edges)| !call_edges.is_empty())
        {
            let lookup = call_extract::load_symbol_lookup(conn, project_id, ref_name)?;
            for (_, call_edges) in pending_call_edges.iter_mut() {
                if !call_edges.is_empty() {
                    call_extract::resolve_call_targets_with_lookup(&lookup, call_edges);
                }
            }
        }
        writer::replace_call_edges_for_files(conn, project_id, ref_name, pending_call_edges)?;
    }

    if embedding_enabled && !pending_embedding_batches.is_empty() {
        embedding_writer.write_embeddings_for_files(
            conn,
            pending_embedding_batches
                .iter()
                .map(|(symbols, snippets)| (symbols.as_slice(), snippets.as_slice())),
        )?;
    }

    batch.commit()?;
    Ok((processed_files, symbols_written, applied_actions))
}

/// Run incremental overlay sync end-to-end into staging and publish atomically.
pub fn run_incremental_sync<A>(
    adapter: &A,
    conn: &mut Connection,
    request: IncrementalSyncRequest<'_>,
) -> Result<IncrementalSyncStats, StateError>
where
    A: VcsAdapter<FileChange = FileChangeKind, DiffEntry = DiffEntry> + Clone,
{
    let maintenance_op = format!("overlay_sync:{}", request.ref_name);
    let _maintenance_lock =
        cruxe_state::maintenance_lock::acquire_project_lock(request.data_dir, &maintenance_op)?;
    ensure_no_active_sync_for_ref(conn, request.project_id, request.ref_name)?;
    let started = Instant::now();

    let owner_pid = i64::from(std::process::id());
    let worktree_manager = if request.is_default_branch || request.ref_name == request.base_ref {
        None
    } else {
        Some(WorktreeManager::new(
            request.repo_root,
            default_worktrees_root(request.data_dir, request.project_id),
            adapter.clone(),
        ))
    };
    let mut lease_acquired = false;
    let mut execution_root = request.repo_root.to_path_buf();
    if let Some(manager) = worktree_manager.as_ref() {
        let lease =
            manager.ensure_worktree(conn, request.project_id, request.ref_name, owner_pid)?;
        lease_acquired = true;
        let lease_root = PathBuf::from(&lease.worktree_path);
        if lease_root.exists() {
            execution_root = lease_root;
        } else {
            warn!(
                project_id = request.project_id,
                ref_name = request.ref_name,
                worktree_path = %lease.worktree_path,
                "Worktree lease path does not exist; falling back to repo_root"
            );
        }
    }

    let mut job_id: Option<String> = None;
    let semantic_config = Config::load(Some(&execution_root))
        .map(|config| config.search.semantic)
        .unwrap_or_else(|err| {
            warn!(
                project_id = request.project_id,
                ref_name = request.ref_name,
                error = %err,
                "Failed to load semantic config for incremental sync, defaulting to semantic=off"
            );
            SemanticConfig::default()
        });
    let sync_result = (|| -> Result<IncrementalSyncStats, StateError> {
        let head_commit = adapter
            .resolve_head(&execution_root)
            .map_err(StateError::vcs)?;

        let rebuild_triggered = if let Some(last) = request.last_indexed_commit {
            should_rebuild_overlay(adapter, &execution_root, last, &head_commit)
                .map_err(StateError::vcs)?
        } else {
            false
        };

        let mode = if rebuild_triggered {
            "overlay_rebuild"
        } else {
            "overlay_incremental"
        };
        let created_job_id = create_sync_job(
            conn,
            request.project_id,
            request.ref_name,
            mode,
            Some(head_commit.clone()),
            request.sync_id,
        )?;
        job_id = Some(created_job_id);

        if rebuild_triggered {
            rebuild_overlay_directory(request.data_dir, request.ref_name)?;
            if semantic_config.mode.eq_ignore_ascii_case("hybrid") {
                cruxe_state::vector_index::delete_vectors_for_ref_with_backend(
                    conn,
                    request.project_id,
                    request.ref_name,
                    semantic_config.vector_backend_opt(),
                )?;
            }
        }

        let plan = build_sync_plan(adapter, &execution_root, request.base_ref, request.ref_name)
            .map_err(StateError::vcs)?;

        let staging_index_set =
            staging::create_staging_index_set(request.data_dir, request.sync_id)?;
        let tx = conn.transaction().map_err(StateError::sqlite)?;

        let (processed_files, symbols_written, applied_actions) = write_actions_to_staging(
            &tx,
            &staging_index_set,
            &execution_root,
            request.project_id,
            request.ref_name,
            &plan.actions,
            &semantic_config,
        )?;
        apply_tombstones_for_actions(&tx, request.project_id, request.ref_name, &applied_actions)?;
        let total_file_count =
            cruxe_state::manifest::file_count(&tx, request.project_id, request.ref_name)?;
        let total_symbol_count =
            cruxe_state::symbols::symbol_count(&tx, request.project_id, request.ref_name)?;
        let publish = staging::commit_staging_to_overlay(
            request.data_dir,
            request.sync_id,
            request.ref_name,
        )?;
        let overlay_dir = publish.overlay_dir.clone();
        persist_branch_sync_state(
            &tx,
            &BranchSyncStateUpdate {
                repo: request.project_id.to_string(),
                ref_name: request.ref_name.to_string(),
                merge_base_commit: Some(plan.merge_base_commit.clone()),
                last_indexed_commit: head_commit.clone(),
                overlay_dir: Some(format!(
                    "overlay/{}",
                    crate::overlay::normalize_overlay_ref(request.ref_name)
                )),
                file_count: total_file_count as i64,
                symbol_count: total_symbol_count as i64,
                is_default_branch: request.is_default_branch,
            },
        )?;
        if let Err(err) = tx.commit() {
            let _ = staging::rollback_overlay_publish(&publish);
            return Err(StateError::sqlite(err));
        }
        if let Err(err) = staging::finalize_overlay_publish(&publish) {
            warn!(
                error = %err,
                project_id = request.project_id,
                ref_name = request.ref_name,
                "Published overlay but failed to cleanup backup directory"
            );
        }

        Ok(IncrementalSyncStats {
            changed_files: plan.actions.len(),
            processed_files,
            symbols_written,
            rebuild_triggered,
            overlay_dir,
            merge_base_commit: plan.merge_base_commit.clone(),
            head_commit: head_commit.clone(),
        })
    })();

    let outcome = match sync_result {
        Ok(stats) => {
            if let Some(job_id) = job_id.as_deref() {
                match mark_sync_job_published(
                    conn,
                    job_id,
                    stats.changed_files as i64,
                    started.elapsed().as_millis() as i64,
                ) {
                    Ok(()) => Ok(stats),
                    Err(err) => Err(err),
                }
            } else {
                Ok(stats)
            }
        }
        Err(err) => {
            let _ = staging::rollback_staging(request.data_dir, request.sync_id);
            if let Some(job_id) = job_id.as_deref() {
                let _ = mark_sync_job_rolled_back(
                    conn,
                    job_id,
                    started.elapsed().as_millis() as i64,
                    &err.to_string(),
                );
            }
            Err(err)
        }
    };

    if lease_acquired
        && let Some(manager) = worktree_manager.as_ref()
        && let Err(err) =
            manager.release_lease(conn, request.project_id, request.ref_name, owner_pid)
    {
        warn!(
            error = %err,
            project_id = request.project_id,
            ref_name = request.ref_name,
            "Failed to release worktree lease"
        );
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{languages, parser, scanner, snippet_extract, symbol_extract, writer};
    use cruxe_core::types::{CallEdge, FileRecord, Project, SymbolKind, SymbolRecord};
    use cruxe_core::vcs::detect_head_commit;
    use cruxe_state::{db, project, schema};
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct FakeAdapter {
        merge_base: String,
        head: String,
        diff: Vec<DiffEntry>,
        ancestor: bool,
    }

    impl VcsAdapter for FakeAdapter {
        type FileChange = FileChangeKind;
        type DiffEntry = DiffEntry;

        fn detect_repo(&self, _repo_root: &Path) -> Result<(), VcsError> {
            Ok(())
        }

        fn resolve_head(&self, _repo_root: &Path) -> Result<String, VcsError> {
            Ok(self.head.clone())
        }

        fn list_refs(&self, _repo_root: &Path) -> Result<Vec<String>, VcsError> {
            Ok(vec!["main".to_string(), "feat/auth".to_string()])
        }

        fn merge_base(
            &self,
            _repo_root: &Path,
            _base_ref: &str,
            _head_ref: &str,
        ) -> Result<String, VcsError> {
            Ok(self.merge_base.clone())
        }

        fn diff_name_status(
            &self,
            _repo_root: &Path,
            _base_ref: &str,
            _head_ref: &str,
        ) -> Result<Vec<Self::DiffEntry>, VcsError> {
            Ok(self.diff.clone())
        }

        fn is_ancestor(
            &self,
            _repo_root: &Path,
            _ancestor: &str,
            _descendant: &str,
        ) -> Result<bool, VcsError> {
            Ok(self.ancestor)
        }

        fn ensure_worktree(
            &self,
            _repo_root: &Path,
            _ref_name: &str,
            _worktree_path: &Path,
        ) -> Result<(), VcsError> {
            Ok(())
        }
    }

    #[test]
    fn expand_diff_entries_converts_rename_to_delete_and_add() {
        let entries = vec![
            DiffEntry::added("src/new.rs"),
            DiffEntry::modified("src/changed.rs"),
            DiffEntry::deleted("src/removed.rs"),
            DiffEntry::renamed("src/old_name.rs", "src/new_name.rs"),
        ];
        let actions = expand_diff_entries(&entries);
        assert_eq!(
            actions,
            vec![
                SyncAction::Added {
                    path: "src/new.rs".to_string()
                },
                SyncAction::Modified {
                    path: "src/changed.rs".to_string()
                },
                SyncAction::Deleted {
                    path: "src/removed.rs".to_string()
                },
                SyncAction::Deleted {
                    path: "src/old_name.rs".to_string()
                },
                SyncAction::Added {
                    path: "src/new_name.rs".to_string()
                },
            ]
        );
    }

    #[test]
    fn build_sync_plan_uses_merge_base_and_diff() {
        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::modified("src/lib.rs")],
            ancestor: true,
        };
        let tmp = tempdir().unwrap();
        let plan = build_sync_plan(&adapter, tmp.path(), "main", "feat/auth").unwrap();
        assert_eq!(plan.merge_base_commit, "base123");
        assert_eq!(plan.head_commit, "head999");
        assert_eq!(
            plan.actions,
            vec![SyncAction::Modified {
                path: "src/lib.rs".to_string()
            }]
        );
    }

    #[test]
    fn should_rebuild_overlay_returns_inverse_of_ancestor_check() {
        let tmp = tempdir().unwrap();
        let repo_root = PathBuf::from(tmp.path());

        let up_to_date = FakeAdapter {
            merge_base: String::new(),
            head: String::new(),
            diff: vec![],
            ancestor: true,
        };
        assert!(!should_rebuild_overlay(&up_to_date, &repo_root, "a", "b").unwrap());

        let broken = FakeAdapter {
            merge_base: String::new(),
            head: String::new(),
            diff: vec![],
            ancestor: false,
        };
        assert!(should_rebuild_overlay(&broken, &repo_root, "a", "b").unwrap());
    }

    #[test]
    fn build_tombstones_marks_deleted_and_replaced() {
        let actions = vec![
            SyncAction::Added {
                path: "src/new.rs".to_string(),
            },
            SyncAction::Modified {
                path: "src/changed.rs".to_string(),
            },
            SyncAction::Deleted {
                path: "src/removed.rs".to_string(),
            },
        ];
        let tombstones = build_tombstones("repo-1", "feat/auth", &actions);
        assert_eq!(tombstones.len(), 2);
        assert_eq!(tombstones[0].path, "src/changed.rs");
        assert_eq!(tombstones[0].tombstone_type, "replaced");
        assert_eq!(tombstones[1].path, "src/removed.rs");
        assert_eq!(tombstones[1].tombstone_type, "deleted");
    }

    #[test]
    fn default_worktrees_root_uses_storage_root_not_data_subdir() {
        let tmp = tempdir().unwrap();
        let storage_root = tmp.path().join(".cruxe");
        let data_dir = storage_root.join("data").join("proj-1");

        let root = default_worktrees_root(&data_dir, "proj-1");
        assert_eq!(root, storage_root.join("worktrees").join("proj-1"));
    }

    #[test]
    fn default_worktrees_root_falls_back_to_data_dir_when_shape_unknown() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path().join("data");

        let root = default_worktrees_root(&data_dir, "proj-1");
        assert_eq!(root, data_dir.join("worktrees").join("proj-1"));
    }

    #[test]
    fn apply_tombstones_for_actions_upserts_rows() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();

        let actions = vec![
            SyncAction::Modified {
                path: "src/a.rs".to_string(),
            },
            SyncAction::Deleted {
                path: "src/b.rs".to_string(),
            },
        ];
        apply_tombstones_for_actions(&conn, "repo-1", "feat/auth", &actions).unwrap();

        let paths =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "repo-1", "feat/auth").unwrap();
        assert_eq!(paths, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    }

    #[test]
    fn apply_tombstones_for_actions_replaces_previous_ref_snapshot() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();

        apply_tombstones_for_actions(
            &conn,
            "repo-1",
            "feat/auth",
            &[SyncAction::Deleted {
                path: "src/old.rs".to_string(),
            }],
        )
        .unwrap();
        let first =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "repo-1", "feat/auth").unwrap();
        assert_eq!(first, vec!["src/old.rs".to_string()]);

        apply_tombstones_for_actions(
            &conn,
            "repo-1",
            "feat/auth",
            &[SyncAction::Modified {
                path: "src/new.rs".to_string(),
            }],
        )
        .unwrap();
        let second =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "repo-1", "feat/auth").unwrap();
        assert_eq!(second, vec!["src/new.rs".to_string()]);
    }

    #[test]
    fn write_actions_to_staging_parse_failure_continues_and_cleans_stale_symbols() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/lib.rs"), "pub fn broken( {\n").unwrap();

        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        let index_set =
            cruxe_state::tantivy_index::IndexSet::open(&tmp.path().join("index")).unwrap();

        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-1".to_string(),
                r#ref: "feat/auth".to_string(),
                commit: None,
                path: "src/lib.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-stale".to_string(),
                symbol_stable_id: "stable-stale".to_string(),
                name: "stale_symbol".to_string(),
                qualified_name: "stale_symbol".to_string(),
                kind: SymbolKind::Function,
                signature: Some("fn stale_symbol()".to_string()),
                line_start: 1,
                line_end: 1,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some("pub fn stale_symbol() {}".to_string()),
            },
        )
        .unwrap();

        let actions = vec![SyncAction::Modified {
            path: "src/lib.rs".to_string(),
        }];
        let (processed_files, symbols_written, applied_actions) =
            write_actions_to_staging_with_parser(
                StagingWriteContext {
                    conn: &conn,
                    index_set: &index_set,
                    repo_root: &repo_root,
                    project_id: "proj-1",
                    ref_name: "feat/auth",
                    actions: &actions,
                    semantic: &SemanticConfig::default(),
                },
                |_content, _language| Err("synthetic parse failure".to_string()),
            )
            .unwrap();

        assert_eq!(processed_files, 1);
        assert_eq!(symbols_written, 0);
        assert_eq!(applied_actions, actions);

        let symbols =
            cruxe_state::symbols::list_symbols_in_file(&conn, "proj-1", "feat/auth", "src/lib.rs")
                .unwrap();
        assert!(
            symbols.is_empty(),
            "stale symbols should be removed on parse failure"
        );

        let mut manifest_paths: Vec<String> =
            cruxe_state::manifest::get_all_entries(&conn, "proj-1", "feat/auth")
                .unwrap()
                .into_iter()
                .map(|entry| entry.path)
                .collect();
        manifest_paths.sort();
        assert_eq!(manifest_paths, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn ensure_no_active_sync_for_ref_rejects_parallel_runs() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "proj-1".to_string(),
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

        let running = jobs::IndexJob {
            job_id: "job-running".to_string(),
            project_id: "proj-1".to_string(),
            r#ref: "feat/auth".to_string(),
            mode: "overlay_incremental".to_string(),
            head_commit: Some("abc".to_string()),
            sync_id: Some("sync-1".to_string()),
            status: "running".to_string(),
            changed_files: 0,
            duration_ms: None,
            error_message: None,
            retry_count: 0,
            progress_token: None,
            files_scanned: 0,
            files_indexed: 0,
            symbols_extracted: 0,
            created_at: "2026-02-25T00:00:00Z".to_string(),
            updated_at: "2026-02-25T00:00:00Z".to_string(),
        };
        jobs::create_job(&conn, &running).unwrap();

        let err = ensure_no_active_sync_for_ref(&conn, "proj-1", "feat/auth").unwrap_err();
        assert!(
            matches!(err, StateError::SyncInProgress { .. }),
            "unexpected error: {err}"
        );

        // Different ref is allowed.
        ensure_no_active_sync_for_ref(&conn, "proj-1", "main").unwrap();
    }

    #[test]
    fn rebuild_overlay_directory_resets_target_overlay() {
        let tmp = tempdir().unwrap();
        let data_dir = tmp.path();
        let ref_name = "feat/auth";

        let first = crate::overlay::create_overlay_dir(data_dir, ref_name).unwrap();
        std::fs::write(first.join("marker.txt"), "stale").unwrap();
        assert!(first.join("marker.txt").exists());

        rebuild_overlay_directory(data_dir, ref_name).unwrap();
        let refreshed = crate::overlay::overlay_dir_for_ref(data_dir, ref_name);
        assert!(refreshed.exists());
        assert!(!refreshed.join("marker.txt").exists());
        assert!(refreshed.join("symbols").exists());
    }

    #[test]
    fn persist_branch_sync_state_writes_branch_state_row() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();

        persist_branch_sync_state(
            &conn,
            &BranchSyncStateUpdate {
                repo: "repo-1".to_string(),
                ref_name: "feat/auth".to_string(),
                merge_base_commit: Some("base123".to_string()),
                last_indexed_commit: "head999".to_string(),
                overlay_dir: Some("overlay/feat-auth".to_string()),
                file_count: 12,
                symbol_count: 34,
                is_default_branch: false,
            },
        )
        .unwrap();

        let row = cruxe_state::branch_state::get_branch_state(&conn, "repo-1", "feat/auth")
            .unwrap()
            .unwrap();
        assert_eq!(row.merge_base_commit, Some("base123".to_string()));
        assert_eq!(row.last_indexed_commit, "head999");
        assert_eq!(row.file_count, 12);
        assert_eq!(row.symbol_count, 34);
        assert_eq!(row.status, "active");
    }

    #[test]
    fn sync_job_helpers_create_and_transition_job_status() {
        let tmp = tempdir().unwrap();
        let conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        project::create_project(
            &conn,
            &Project {
                project_id: "proj-1".to_string(),
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

        let job_id = create_sync_job(
            &conn,
            "proj-1",
            "feat/auth",
            "overlay_incremental",
            Some("head123".to_string()),
            "sync-abc",
        )
        .unwrap();

        let active = jobs::get_active_job_for_ref(&conn, "proj-1", "feat/auth")
            .unwrap()
            .unwrap();
        assert_eq!(active.job_id, job_id);
        assert_eq!(active.status, "running");
        assert_eq!(active.sync_id, Some("sync-abc".to_string()));

        mark_sync_job_published(&conn, &job_id, 7, 1234).unwrap();
        assert!(
            jobs::get_active_job_for_ref(&conn, "proj-1", "feat/auth")
                .unwrap()
                .is_none()
        );

        let job_id2 = create_sync_job(
            &conn,
            "proj-1",
            "feat/auth",
            "overlay_incremental",
            Some("head124".to_string()),
            "sync-def",
        )
        .unwrap();
        mark_sync_job_rolled_back(&conn, &job_id2, 321, "parse failure").unwrap();
        assert!(
            jobs::get_active_job_for_ref(&conn, "proj-1", "feat/auth")
                .unwrap()
                .is_none()
        );
    }

    fn insert_project(conn: &Connection, project_id: &str, repo_root: &Path) {
        project::create_project(
            conn,
            &Project {
                project_id: project_id.to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
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
    }

    #[test]
    fn run_incremental_sync_publishes_overlay_and_branch_state() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/new.rs"), "pub fn new_fn() {}\n").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::added("src/new.rs")],
            ancestor: true,
        };
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-1",
                last_indexed_commit: Some("head998"),
                is_default_branch: false,
            },
        )
        .unwrap();

        assert_eq!(stats.changed_files, 1);
        assert_eq!(stats.processed_files, 1);
        assert!(!stats.rebuild_triggered);
        assert!(stats.overlay_dir.join("symbols").exists());
        assert!(stats.overlay_dir.join("snippets").exists());
        assert!(stats.overlay_dir.join("files").exists());

        let branch = cruxe_state::branch_state::get_branch_state(&conn, "proj-1", "feat/auth")
            .unwrap()
            .unwrap();
        assert_eq!(branch.merge_base_commit, Some("base123".to_string()));
        assert_eq!(branch.file_count, 1);
        assert_eq!(branch.overlay_dir, Some("overlay/feat-auth".to_string()));
    }

    #[test]
    fn run_incremental_sync_rename_records_tombstone_for_old_path() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/new_name.rs"), "pub fn renamed() {}\n").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::renamed("src/old_name.rs", "src/new_name.rs")],
            ancestor: true,
        };
        run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-2",
                last_indexed_commit: Some("head998"),
                is_default_branch: false,
            },
        )
        .unwrap();

        let tombstones =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "proj-1", "feat/auth").unwrap();
        assert_eq!(tombstones, vec!["src/old_name.rs".to_string()]);
    }

    #[test]
    fn run_incremental_sync_delete_file_cleans_incoming_edges_to_deleted_symbols() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/a.rs"), "pub fn a() {}\n").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let a = SymbolRecord {
            repo: "proj-1".to_string(),
            r#ref: "feat/auth".to_string(),
            commit: None,
            path: "src/a.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym::a".to_string(),
            symbol_stable_id: "stable-a".to_string(),
            name: "a".to_string(),
            qualified_name: "a".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn a()".to_string()),
            line_start: 1,
            line_end: 1,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        };
        let b = SymbolRecord {
            repo: "proj-1".to_string(),
            r#ref: "feat/auth".to_string(),
            commit: None,
            path: "src/b.rs".to_string(),
            language: "rust".to_string(),
            symbol_id: "sym::b".to_string(),
            symbol_stable_id: "stable-b".to_string(),
            name: "b".to_string(),
            qualified_name: "b".to_string(),
            kind: SymbolKind::Function,
            signature: Some("fn b()".to_string()),
            line_start: 1,
            line_end: 1,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: None,
        };
        cruxe_state::symbols::insert_symbol(&conn, &a).unwrap();
        cruxe_state::symbols::insert_symbol(&conn, &b).unwrap();
        cruxe_state::edges::insert_call_edges(
            &conn,
            "proj-1",
            "feat/auth",
            &[CallEdge {
                repo: "proj-1".to_string(),
                ref_name: "feat/auth".to_string(),
                from_symbol_id: "stable-a".to_string(),
                to_symbol_id: Some("stable-b".to_string()),
                to_name: None,
                edge_type: "calls".to_string(),
                confidence: "static".to_string(),
                source_file: "src/a.rs".to_string(),
                source_line: 1,
            }],
        )
        .unwrap();

        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::deleted("src/b.rs")],
            ancestor: true,
        };
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-delete-cleanup",
                last_indexed_commit: Some("head998"),
                is_default_branch: false,
            },
        )
        .unwrap();
        assert_eq!(stats.changed_files, 1);

        let callees =
            cruxe_state::edges::get_callees(&conn, "proj-1", "feat/auth", "stable-a").unwrap();
        assert!(
            callees
                .iter()
                .all(|edge| edge.to_symbol_id.as_deref() != Some("stable-b")),
            "incoming edges to deleted symbol should be removed"
        );
    }

    #[test]
    fn run_incremental_sync_fails_for_unreadable_modified_file_without_committing_state() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/ok.rs"), "pub fn ok() {}\n").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::modified("src/missing.rs")],
            ancestor: true,
        };
        let result = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-skip-modified",
                last_indexed_commit: Some("head998"),
                is_default_branch: false,
            },
        );

        assert!(result.is_err());
        let tombstones =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "proj-1", "feat/auth").unwrap();
        assert!(
            tombstones.is_empty(),
            "failed sync must not persist tombstones"
        );
        assert!(
            cruxe_state::branch_state::get_branch_state(&conn, "proj-1", "feat/auth")
                .unwrap()
                .is_none(),
            "failed sync must not persist branch state"
        );
    }

    #[test]
    fn run_incremental_sync_noop_keeps_total_branch_state_counts() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/new.rs"), "pub fn new_fn() {}\n").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let first_adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::added("src/new.rs")],
            ancestor: true,
        };
        run_incremental_sync(
            &first_adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-first",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();

        let first_branch =
            cruxe_state::branch_state::get_branch_state(&conn, "proj-1", "feat/auth")
                .unwrap()
                .unwrap();
        assert_eq!(first_branch.file_count, 1);
        assert!(first_branch.symbol_count > 0);

        let second_adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head1000".to_string(),
            diff: Vec::new(),
            ancestor: true,
        };
        let second_stats = run_incremental_sync(
            &second_adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-second",
                last_indexed_commit: Some("head999"),
                is_default_branch: false,
            },
        )
        .unwrap();
        assert_eq!(second_stats.changed_files, 0);
        assert_eq!(second_stats.processed_files, 0);

        let second_branch =
            cruxe_state::branch_state::get_branch_state(&conn, "proj-1", "feat/auth")
                .unwrap()
                .unwrap();
        assert_eq!(second_branch.file_count, first_branch.file_count);
        assert_eq!(second_branch.symbol_count, first_branch.symbol_count);
    }

    #[test]
    fn run_incremental_sync_ancestry_break_triggers_overlay_rebuild() {
        let tmp = tempdir().unwrap();
        let repo_root = tmp.path().join("repo");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        std::fs::write(repo_root.join("src/new.rs"), "pub fn new_fn() {}\n").unwrap();

        let stale_overlay = crate::overlay::create_overlay_dir(&data_dir, "feat/auth").unwrap();
        std::fs::write(stale_overlay.join("stale.marker"), "stale").unwrap();

        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-1", &repo_root);

        let adapter = FakeAdapter {
            merge_base: "base123".to_string(),
            head: "head999".to_string(),
            diff: vec![DiffEntry::added("src/new.rs")],
            ancestor: false,
        };
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-1",
                ref_name: "feat/auth",
                base_ref: "main",
                sync_id: "sync-3",
                last_indexed_commit: Some("old_commit"),
                is_default_branch: false,
            },
        )
        .unwrap();

        assert!(stats.rebuild_triggered);
        assert!(!stats.overlay_dir.join("stale.marker").exists());
    }

    #[cfg(unix)]
    fn fixture_setup_script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/vcs-sample/setup.sh")
    }

    #[cfg(unix)]
    fn setup_vcs_fixture_repo() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempdir().unwrap();
        let script = fixture_setup_script();
        assert!(
            script.exists(),
            "missing fixture script: {}",
            script.display()
        );
        let repo_path = tmp.path().join("vcs-sample");

        let output = std::process::Command::new("bash")
            .arg(&script)
            .arg(tmp.path())
            .output()
            .expect("run vcs fixture setup script");
        assert!(
            output.status.success(),
            "fixture setup failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        (tmp, repo_path)
    }

    #[cfg(unix)]
    fn git(repo_root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(unix)]
    fn git_output(repo_root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[cfg(unix)]
    fn index_current_checkout_as_base(
        repo_root: &Path,
        data_dir: &Path,
        conn: &Connection,
        project_id: &str,
    ) {
        let base_index_set = cruxe_state::tantivy_index::IndexSet::open(data_dir).unwrap();
        let files = scanner::scan_directory_filtered(repo_root, 1_048_576, &["rust".to_string()]);
        for file in files {
            let content = std::fs::read_to_string(&file.path).unwrap();
            let tree = parser::parse_file(&content, &file.language).unwrap();
            let extracted = languages::extract_symbols(&tree, &content, &file.language);
            let symbols = symbol_extract::build_symbol_records(
                &extracted,
                project_id,
                "main",
                &file.relative_path,
                None,
            );
            let snippets = snippet_extract::build_snippet_records(
                &extracted,
                project_id,
                "main",
                &file.relative_path,
                None,
            );
            let record = FileRecord {
                repo: project_id.to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: file.relative_path.clone(),
                filename: file
                    .path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                language: file.language.clone(),
                content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                size_bytes: content.len() as u64,
                updated_at: now_iso8601(),
                content_head: Some(content.lines().take(20).collect::<Vec<_>>().join("\n")),
            };
            writer::write_file_records(&base_index_set, conn, &symbols, &snippets, &record)
                .unwrap();
        }

        persist_branch_sync_state(
            conn,
            &BranchSyncStateUpdate {
                repo: project_id.to_string(),
                ref_name: "main".to_string(),
                merge_base_commit: None,
                last_indexed_commit: detect_head_commit(repo_root).unwrap_or_default(),
                overlay_dir: None,
                file_count: cruxe_state::manifest::file_count(conn, project_id, "main").unwrap()
                    as i64,
                symbol_count: 0,
                is_default_branch: true,
            },
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn manifest_paths(conn: &Connection, repo: &str, ref_name: &str) -> Vec<String> {
        let mut paths: Vec<String> = cruxe_state::manifest::get_all_entries(conn, repo, ref_name)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        paths.sort();
        paths
    }

    #[cfg(unix)]
    #[test]
    fn t270_overlay_sync_add_file_keeps_base_immutable() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");
        let base_before = manifest_paths(&conn, "proj-vcs", "main");
        assert!(base_before.contains(&"src/lib.rs".to_string()));

        git(&repo_root, &["checkout", "feat/add-file"]);
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/add-file",
                base_ref: "main",
                sync_id: "sync-add-file",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();

        assert_eq!(stats.changed_files, 1);
        let overlay_paths = manifest_paths(&conn, "proj-vcs", "feat/add-file");
        assert_eq!(overlay_paths, vec!["src/add_file.rs".to_string()]);
        assert_eq!(manifest_paths(&conn, "proj-vcs", "main"), base_before);
    }

    #[cfg(unix)]
    #[test]
    fn t297_overlay_sync_uses_target_ref_worktree_without_checkout() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

        // Intentionally keep the primary workspace on `main` and sync another ref.
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/add-file",
                base_ref: "main",
                sync_id: "sync-worktree-ref",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();

        assert_eq!(stats.changed_files, 1);
        let overlay_paths = manifest_paths(&conn, "proj-vcs", "feat/add-file");
        assert_eq!(overlay_paths, vec!["src/add_file.rs".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn run_incremental_sync_releases_lease_and_rolls_back_job_on_plan_error() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let result = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/add-file",
                base_ref: "refs/heads/does-not-exist",
                sync_id: "sync-plan-error",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        );
        assert!(result.is_err(), "invalid base ref should fail planning");
        assert!(
            jobs::get_active_job_for_ref(&conn, "proj-vcs", "feat/add-file")
                .unwrap()
                .is_none(),
            "sync job should not remain active after failure"
        );

        let recent = jobs::get_recent_jobs(&conn, "proj-vcs", 1).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(
            recent[0].status,
            JobStatus::RolledBack.as_str(),
            "failed sync should be marked rolled_back"
        );

        let lease = cruxe_state::worktree_leases::get_lease(&conn, "proj-vcs", "feat/add-file")
            .unwrap()
            .expect("lease row should remain for stale cleanup");
        assert_eq!(lease.status, "stale");
        assert_eq!(lease.refcount, 0);
        assert_eq!(lease.owner_pid, 0);
    }

    #[cfg(unix)]
    #[test]
    fn t271_overlay_sync_delete_file_creates_tombstone() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

        git(&repo_root, &["checkout", "feat/delete-file"]);
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let _ = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/delete-file",
                base_ref: "main",
                sync_id: "sync-delete-file",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();

        let tombstones =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "proj-vcs", "feat/delete-file")
                .unwrap();
        assert_eq!(tombstones, vec!["src/lib.rs".to_string()]);
        assert!(manifest_paths(&conn, "proj-vcs", "main").contains(&"src/lib.rs".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn t272_overlay_sync_rename_tombstones_old_path_and_indexes_new_path() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        index_current_checkout_as_base(&repo_root, &data_dir, &conn, "proj-vcs");

        git(&repo_root, &["checkout", "feat/rename-file"]);
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/rename-file",
                base_ref: "main",
                sync_id: "sync-rename-file",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();

        assert_eq!(
            stats.changed_files, 2,
            "rename should expand to delete + add"
        );
        let tombstones =
            cruxe_state::tombstones::list_paths_for_ref(&conn, "proj-vcs", "feat/rename-file")
                .unwrap();
        assert_eq!(tombstones, vec!["src/lib.rs".to_string()]);
        let overlay_paths = manifest_paths(&conn, "proj-vcs", "feat/rename-file");
        assert_eq!(overlay_paths, vec!["src/core.rs".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn t273_rebase_triggers_overlay_rebuild() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "feat/rebase-target"]);
        let adapter = cruxe_vcs::Git2VcsAdapter;
        let first = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/rebase-target",
                base_ref: "main",
                sync_id: "sync-rebase-1",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();
        assert!(!first.rebuild_triggered);

        let old_head = git_output(&repo_root, &["rev-parse", "HEAD"]);
        git(&repo_root, &["rebase", "main"]);
        let new_head = git_output(&repo_root, &["rev-parse", "HEAD"]);
        assert_ne!(old_head, new_head, "rebase should rewrite branch head");

        let overlay_dir = crate::overlay::overlay_dir_for_ref(&data_dir, "feat/rebase-target");
        std::fs::write(overlay_dir.join("stale.marker"), "stale").unwrap();

        let second = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/rebase-target",
                base_ref: "main",
                sync_id: "sync-rebase-2",
                last_indexed_commit: Some(&old_head),
                is_default_branch: false,
            },
        )
        .unwrap();

        assert!(second.rebuild_triggered);
        assert!(
            !crate::overlay::overlay_dir_for_ref(&data_dir, "feat/rebase-target")
                .join("stale.marker")
                .exists(),
            "overlay rebuild should replace stale directory contents"
        );
    }

    #[cfg(unix)]
    #[test]
    fn t274_incremental_sync_ten_file_smoke_under_five_seconds() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        git(&repo_root, &["checkout", "-b", "feat/perf-10"]);
        std::fs::create_dir_all(repo_root.join("src/perf")).unwrap();
        for i in 0..10 {
            std::fs::write(
                repo_root.join(format!("src/perf/file_{i}.rs")),
                format!("pub fn perf_{i}() -> usize {{ {i} }}\n"),
            )
            .unwrap();
        }
        git(&repo_root, &["add", "."]);
        git(&repo_root, &["commit", "-m", "feat/perf-10: add ten files"]);

        let adapter = cruxe_vcs::Git2VcsAdapter;
        let started = Instant::now();
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/perf-10",
                base_ref: "main",
                sync_id: "sync-perf-10",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();
        let elapsed = started.elapsed();

        assert!(
            stats.changed_files >= 10,
            "expected at least 10 changed actions, got {}",
            stats.changed_files
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "incremental sync should remain below 5s for 10 files, got {:?}",
            elapsed
        );
    }

    #[cfg(unix)]
    #[test]
    fn t294_overlay_bootstrap_fifty_file_smoke_under_fifteen_seconds() {
        let (tmp, repo_root) = setup_vcs_fixture_repo();
        let data_dir = tmp.path().join("data");
        let mut conn = db::open_connection(&tmp.path().join("state.db")).unwrap();
        schema::create_tables(&conn).unwrap();
        insert_project(&conn, "proj-vcs", &repo_root);

        git(&repo_root, &["checkout", "main"]);
        git(&repo_root, &["checkout", "-b", "feat/perf-50"]);
        std::fs::create_dir_all(repo_root.join("src/perf50")).unwrap();
        for i in 0..50 {
            std::fs::write(
                repo_root.join(format!("src/perf50/file_{i}.rs")),
                format!("pub fn perf_bootstrap_{i}() -> usize {{ {i} }}\n"),
            )
            .unwrap();
        }
        git(&repo_root, &["add", "."]);
        git(
            &repo_root,
            &["commit", "-m", "feat/perf-50: add fifty files"],
        );

        let adapter = cruxe_vcs::Git2VcsAdapter;
        let started = Instant::now();
        let stats = run_incremental_sync(
            &adapter,
            &mut conn,
            IncrementalSyncRequest {
                repo_root: &repo_root,
                data_dir: &data_dir,
                project_id: "proj-vcs",
                ref_name: "feat/perf-50",
                base_ref: "main",
                sync_id: "sync-perf-50",
                last_indexed_commit: None,
                is_default_branch: false,
            },
        )
        .unwrap();
        let elapsed = started.elapsed();

        assert!(
            stats.changed_files >= 50,
            "expected at least 50 changed actions, got {}",
            stats.changed_files
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "overlay bootstrap should remain below 15s for 50 files, got {:?}",
            elapsed
        );
    }
}
