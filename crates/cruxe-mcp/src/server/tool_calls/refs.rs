use super::*;
use cruxe_core::constants;
use cruxe_core::time::now_iso8601;
use cruxe_vcs::{Git2VcsAdapter, WorktreeManager};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct RefDescriptorPayload {
    #[serde(rename = "ref")]
    ref_name: String,
    is_default: bool,
    last_indexed_commit: Option<String>,
    merge_base_commit: Option<String>,
    file_count: u64,
    symbol_count: u64,
    status: String,
    last_accessed_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListRefsPayload {
    refs: Vec<RefDescriptorPayload>,
    total_refs: usize,
    vcs_mode: bool,
    metadata: ProtocolMetadata,
}

#[derive(Debug, Serialize)]
struct SwitchRefPayload {
    #[serde(rename = "ref")]
    ref_name: String,
    previous_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_path: Option<String>,
    status: String,
    last_indexed_commit: String,
    metadata: ProtocolMetadata,
}

pub(super) fn handle_list_refs(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        config,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
        ..
    } = params;

    let effective_ref = resolve_tool_ref(None, workspace, conn, project_id);
    let metadata = build_metadata(
        &effective_ref,
        schema_status,
        config,
        conn,
        workspace,
        project_id,
    );

    let Some(c) = conn else {
        return tool_compatibility_error(ToolCompatibilityParams {
            id,
            schema_status,
            compatibility_reason,
            config,
            conn,
            workspace,
            project_id,
            ref_name: &effective_ref,
        });
    };
    let project_row = match cruxe_state::project::get_by_id(c, project_id) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::ProjectNotFound,
                "Project is not initialized for this workspace. Run `cruxe init` first.",
                Some(json!({
                    "workspace": workspace.to_string_lossy(),
                    "remediation": "cruxe init --path <workspace>"
                })),
                metadata,
            );
        }
        Err(err) => {
            let (code, message, data) = map_state_error(&err);
            return tool_error_response(id, code, message, data, metadata);
        }
    };

    let refs = if project_row.vcs_mode {
        match cruxe_state::branch_state::list_branch_states(c, project_id) {
            Ok(mut entries) => {
                entries.sort_by(|a, b| a.r#ref.cmp(&b.r#ref));
                entries
                    .into_iter()
                    .map(|entry| RefDescriptorPayload {
                        ref_name: entry.r#ref,
                        is_default: entry.is_default_branch,
                        last_indexed_commit: Some(entry.last_indexed_commit),
                        merge_base_commit: entry.merge_base_commit,
                        file_count: entry.file_count.max(0) as u64,
                        symbol_count: entry.symbol_count.max(0) as u64,
                        status: entry.status,
                        last_accessed_at: Some(entry.last_accessed_at),
                    })
                    .collect::<Vec<_>>()
            }
            Err(err) => {
                let (code, message, data) = map_state_error(&err);
                return tool_error_response(id, code, message, data, metadata);
            }
        }
    } else {
        vec![RefDescriptorPayload {
            ref_name: constants::REF_LIVE.to_string(),
            is_default: true,
            last_indexed_commit: None,
            merge_base_commit: None,
            file_count: cruxe_state::manifest::file_count(c, project_id, constants::REF_LIVE)
                .unwrap_or(0),
            symbol_count: cruxe_state::symbols::symbol_count(c, project_id, constants::REF_LIVE)
                .unwrap_or(0),
            status: "active".to_string(),
            last_accessed_at: None,
        }]
    };

    tool_text_response(
        id,
        serde_json::to_value(ListRefsPayload {
            total_refs: refs.len(),
            refs,
            vcs_mode: project_row.vcs_mode,
            metadata,
        })
        .unwrap_or_else(|_| json!({"error":"failed to serialize list_refs payload"})),
    )
}

pub(super) fn handle_switch_ref(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        arguments,
        config,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
        ..
    } = params;

    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let target_ref = requested_ref.unwrap_or("").trim();
    let previous_ref = resolve_tool_ref(None, workspace, conn, project_id);
    let base_metadata = validation_metadata(&previous_ref, schema_status);
    if target_ref.is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `ref` is required.",
            None,
            base_metadata,
        );
    }

    let Some(c) = conn else {
        return tool_compatibility_error(ToolCompatibilityParams {
            id,
            schema_status,
            compatibility_reason,
            config,
            conn,
            workspace,
            project_id,
            ref_name: &previous_ref,
        });
    };

    let project_row = match cruxe_state::project::get_by_id(c, project_id) {
        Ok(Some(project)) => project,
        Ok(None) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::ProjectNotFound,
                "Project is not initialized for this workspace. Run `cruxe init` first.",
                Some(json!({
                    "workspace": workspace.to_string_lossy(),
                    "remediation": "cruxe init --path <workspace>"
                })),
                base_metadata,
            );
        }
        Err(err) => {
            let (code, message, data) = map_state_error(&err);
            return tool_error_response(id, code, message, data, base_metadata);
        }
    };

    let state = match cruxe_state::branch_state::get_branch_state(c, project_id, target_ref) {
        Ok(Some(state)) => state,
        Ok(None) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::RefNotIndexed,
                "The requested ref has no indexed state yet.",
                Some(json!({
                    "details": format!("ref_not_indexed: project_id={project_id}, ref={target_ref}"),
                    "remediation": format!("Run sync_repo with ref=\"{target_ref}\" before switching."),
                })),
                base_metadata,
            );
        }
        Err(err) => {
            let (code, message, data) = map_state_error(&err);
            return tool_error_response(id, code, message, data, base_metadata);
        }
    };
    if matches!(state.status.as_str(), "syncing" | "rebuilding" | "indexing") {
        return tool_error_response(
            id,
            ProtocolErrorCode::OverlayNotReady,
            "The requested ref overlay is not query-ready yet.",
            Some(json!({
                "details": format!("overlay_not_ready: project_id={project_id}, ref={target_ref}, status={}", state.status),
                "remediation": "Poll index_status until indexing finishes, then retry.",
            })),
            base_metadata,
        );
    }

    let worktree_path = if project_row.vcs_mode && target_ref != project_row.default_ref {
        let worktrees_root =
            default_worktrees_root(&config.project_data_dir(project_id), project_id);
        let manager = WorktreeManager::new(workspace, worktrees_root, Git2VcsAdapter);
        let owner_pid = std::process::id() as i64;
        match manager.ensure_worktree(c, project_id, target_ref, owner_pid) {
            Ok(lease) => {
                let path = lease.worktree_path.clone();
                let _ = manager.release_lease(c, project_id, target_ref, owner_pid);
                Some(path)
            }
            Err(err) => {
                let (code, message, data) = map_state_error(&err);
                return tool_error_response(id, code, message, data, base_metadata);
            }
        }
    } else {
        None
    };

    if let Ok(Some(mut row)) =
        cruxe_state::branch_state::get_branch_state(c, project_id, target_ref)
    {
        row.last_accessed_at = now_iso8601();
        if let Err(err) = cruxe_state::branch_state::upsert_branch_state(c, &row) {
            let (code, message, data) = map_state_error(&err);
            return tool_error_response(id, code, message, data, base_metadata);
        }
    }
    let override_result = if target_ref == project_row.default_ref {
        clear_session_ref_override(workspace, project_id)
    } else {
        set_session_ref_override(workspace, project_id, target_ref)
    };
    if let Err(err) = override_result {
        let (code, message, data) = map_state_error(&err);
        return tool_error_response(id, code, message, data, base_metadata);
    }

    let metadata = build_metadata(
        target_ref,
        schema_status,
        config,
        conn,
        workspace,
        project_id,
    );
    tool_text_response(
        id,
        serde_json::to_value(SwitchRefPayload {
            ref_name: target_ref.to_string(),
            previous_ref,
            worktree_path,
            status: state.status,
            last_indexed_commit: state.last_indexed_commit,
            metadata,
        })
        .unwrap_or_else(|_| json!({"error":"failed to serialize switch_ref payload"})),
    )
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
