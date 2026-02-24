use crate::notifications::{McpProgressNotifier, NullProgressNotifier, ProgressNotifier};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, ProtocolMetadata};
use crate::tools;
use crate::workspace_router::WorkspaceRouter;
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::error::{StateError, WorkspaceError};
use codecompass_core::types::{DetailLevel, SchemaStatus, WorkspaceConfig, generate_project_id};
use codecompass_query::context;
use codecompass_query::detail;
use codecompass_query::freshness::{
    self, FreshnessResult, PolicyAction, apply_freshness_policy, check_freshness_with_scan_params,
    parse_freshness_policy, trigger_async_sync,
};
use codecompass_query::hierarchy;
use codecompass_query::locate;
use codecompass_query::ranking;
use codecompass_query::related;
use codecompass_query::search;
use codecompass_state::tantivy_index::IndexSet;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{error, info};

use self::tool_calls::{ToolCallParams, handle_tool_call};

/// Prewarm status values (stored as AtomicU8).
pub const PREWARM_PENDING: u8 = 0;
pub const PREWARM_IN_PROGRESS: u8 = 1;
pub const PREWARM_COMPLETE: u8 = 2;
pub const PREWARM_FAILED: u8 = 3;
pub const PREWARM_SKIPPED: u8 = 4;
const DEFAULT_WARMSET_CAPACITY: usize = 3;

/// Convert prewarm status byte to string label.
pub fn prewarm_status_label(status: u8) -> &'static str {
    match status {
        PREWARM_PENDING => "pending",
        PREWARM_IN_PROGRESS => "warming",
        PREWARM_COMPLETE => "complete",
        PREWARM_FAILED => "failed",
        PREWARM_SKIPPED => "skipped",
        _ => "unknown",
    }
}

pub(crate) fn warmset_capacity() -> usize {
    std::env::var("CODECOMPASS_WARMSET_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_WARMSET_CAPACITY)
}

pub(crate) fn collect_warmset_project_ids(
    db_path: &Path,
    default_project_id: &str,
    capacity: usize,
) -> Vec<String> {
    let mut project_ids = vec![default_project_id.to_string()];
    let mut seen: HashSet<String> = HashSet::from([default_project_id.to_string()]);
    if let Ok(conn) = codecompass_state::db::open_connection(db_path)
        && let Ok(recent) = codecompass_state::workspace::list_recent_workspaces(&conn, capacity)
    {
        for ws in recent {
            let pid = ws
                .project_id
                .unwrap_or_else(|| generate_project_id(&ws.workspace_path));
            if seen.insert(pid.clone()) {
                project_ids.push(pid);
            }
        }
    }
    if project_ids.len() > capacity {
        project_ids.truncate(capacity);
    }
    project_ids
}

pub(crate) fn collect_warmset_members(
    conn: Option<&rusqlite::Connection>,
    default_workspace: &Path,
    capacity: usize,
) -> Vec<String> {
    let default_member = default_workspace.to_string_lossy().to_string();
    let mut members = vec![default_member.clone()];
    let mut seen: HashSet<String> = HashSet::from([default_member]);
    if let Some(c) = conn
        && let Ok(recent) = codecompass_state::workspace::list_recent_workspaces(c, capacity)
    {
        for ws in recent {
            if seen.insert(ws.workspace_path.clone()) {
                members.push(ws.workspace_path);
            }
        }
    }
    if members.len() > capacity {
        members.truncate(capacity);
    }
    members
}

pub(crate) fn prewarm_projects(status: Arc<AtomicU8>, config: Config, project_ids: Vec<String>) {
    status.store(PREWARM_IN_PROGRESS, Ordering::Release);
    let mut had_index = false;
    for pid in project_ids {
        let data_dir = config.project_data_dir(&pid);
        match IndexSet::open_existing(&data_dir) {
            Ok(index_set) => {
                had_index = true;
                if let Err(e) = codecompass_state::tantivy_index::prewarm_indices(&index_set) {
                    error!(project_id = %pid, "Tantivy index prewarm failed: {}", e);
                    status.store(PREWARM_FAILED, Ordering::Release);
                    return;
                }
                info!(project_id = %pid, "Tantivy index prewarm complete");
            }
            Err(_) => {
                // Skip workspaces that are known but not indexed yet.
            }
        }
    }
    if had_index {
        status.store(PREWARM_COMPLETE, Ordering::Release);
    } else {
        status.store(PREWARM_SKIPPED, Ordering::Release);
    }
}

pub(crate) fn is_status_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "index_repo" | "sync_repo" | "index_status" | "health_check"
    )
}

fn supports_progress_notifications(params: &Value) -> bool {
    let Some(capabilities) = params.get("capabilities") else {
        return false;
    };

    capabilities.get("notifications").is_some()
        || capabilities
            .pointer("/experimental/notifications")
            .is_some()
        || capabilities.pointer("/experimental/progress").is_some()
}

/// Run the MCP server loop on stdin/stdout.
pub fn run_server(
    workspace: &Path,
    config_file: Option<&Path>,
    no_prewarm: bool,
    workspace_config: WorkspaceConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load_with_file(Some(workspace), config_file)?;
    let project_id = generate_project_id(&workspace.to_string_lossy());
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let server_start = Instant::now();

    // T455: Mark any leftover running/queued jobs as interrupted on startup
    if let Ok(conn) = codecompass_state::db::open_connection(&db_path) {
        match codecompass_state::jobs::mark_interrupted_jobs(&conn) {
            Ok(count) if count > 0 => {
                info!(count, "Marked interrupted jobs from previous session");
            }
            _ => {}
        }
    }

    // Create workspace router (validates config at startup — T206/T208)
    let router = WorkspaceRouter::new(workspace_config, workspace.to_path_buf(), db_path.clone())
        .map_err(|e| format!("workspace config error: {}", e))?;

    // Shared prewarm status
    let prewarm_status = Arc::new(AtomicU8::new(PREWARM_PENDING));

    // Start warmset prewarm in background thread (or skip)
    if no_prewarm {
        prewarm_status.store(PREWARM_SKIPPED, Ordering::Release);
    } else {
        let ps = Arc::clone(&prewarm_status);
        let config_clone = config.clone();
        let project_ids = collect_warmset_project_ids(&db_path, &project_id, warmset_capacity());
        std::thread::spawn(move || prewarm_projects(ps, config_clone, project_ids));
    }

    let stdin = io::stdin();
    // T214: Wrap stdout in Arc<Mutex> so it can be shared with the progress notifier.
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(io::stdout())));

    let notifications_enabled = Arc::new(AtomicBool::new(false));

    info!("MCP server started");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("stdin read error: {}", e);
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e));
                write_response(&writer, &resp)?;
                continue;
            }
        };

        if request.method == "initialize" {
            notifications_enabled.store(
                supports_progress_notifications(&request.params),
                Ordering::Release,
            );
        }

        // Resolve workspace for tools/call requests
        let (eff_workspace, eff_project_id, eff_data_dir) = if request.method == "tools/call" {
            let ws_param = request
                .params
                .get("arguments")
                .and_then(|a| a.get("workspace"))
                .and_then(|v| v.as_str());
            match router.resolve_workspace(ws_param) {
                Ok(resolved) => {
                    let eff_data_dir = config.project_data_dir(&resolved.project_id);

                    // T205: On-demand indexing for auto-discovered workspaces
                    if resolved.on_demand_indexing {
                        if resolved.should_bootstrap
                            && let Err(e) = bootstrap_and_index(
                                &resolved.workspace_path,
                                &resolved.project_id,
                                &eff_data_dir,
                            )
                        {
                            error!(
                                workspace = %resolved.workspace_path.display(),
                                "on-demand bootstrap failed: {}", e
                            );
                        }

                        // HIGH-5: For query tools, return graceful "indexing" response
                        // instead of compatibility error (spec resolution logic step 4f)
                        let tool_name = request
                            .params
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if !is_status_tool(tool_name) {
                            let effective_ref =
                                codecompass_core::vcs::detect_head_branch(&resolved.workspace_path)
                                    .unwrap_or_else(|_| constants::REF_LIVE.to_string());
                            let metadata = ProtocolMetadata::syncing(&effective_ref);
                            let resp = tool_calls::tool_text_response(
                                request.id.clone(),
                                json!({
                                    "indexing_status": "indexing",
                                    "result_completeness": "partial",
                                    "workspace": resolved.workspace_path.to_string_lossy(),
                                    "message": "Workspace is being indexed. Results will be available shortly. Use index_status to check progress.",
                                    "suggested_next_actions": ["poll index_status", "retry after indexing completes"],
                                    "metadata": metadata,
                                }),
                            );
                            write_response(&writer, &resp)?;
                            continue;
                        }
                    }

                    (resolved.workspace_path, resolved.project_id, eff_data_dir)
                }
                Err(e) => {
                    let resp = workspace_error_to_response(request.id.clone(), &e);
                    write_response(&writer, &resp)?;
                    continue;
                }
            }
        } else {
            (
                workspace.to_path_buf(),
                project_id.clone(),
                data_dir.clone(),
            )
        };

        let eff_db_path = eff_data_dir.join(constants::STATE_DB_FILE);
        let index_runtime = load_index_runtime(&eff_data_dir);
        let conn = codecompass_state::db::open_connection(&eff_db_path).ok();

        let notifications_enabled_now = notifications_enabled.load(Ordering::Acquire);
        // Optional client-provided progress token (used as notification correlation id).
        // If notifications are enabled but no token is provided, set an empty sentinel so
        // index_repo can still emit progress using server-generated token.
        let progress_token = if notifications_enabled_now {
            request
                .params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(String::new()))
        } else {
            None
        };

        let notifier: Arc<dyn ProgressNotifier> = if notifications_enabled_now {
            Arc::new(McpProgressNotifier::new(Arc::clone(&writer)))
        } else {
            Arc::new(NullProgressNotifier)
        };

        let request_ctx = RequestContext {
            config: &config,
            index_set: index_runtime.index_set.as_ref(),
            schema_status: index_runtime.schema_status,
            compatibility_reason: index_runtime.compatibility_reason.as_deref(),
            conn: conn.as_ref(),
            workspace: &eff_workspace,
            project_id: &eff_project_id,
            prewarm_status: &prewarm_status,
            server_start: &server_start,
            notifier,
            progress_token: progress_token.as_deref(),
        };
        let response = handle_request_with_ctx(&request, &request_ctx);
        write_response(&writer, &response)?;
    }

    Ok(())
}

/// Convert a workspace resolution error into an MCP tool-level error response.
fn workspace_error_to_response(id: Option<Value>, err: &WorkspaceError) -> JsonRpcResponse {
    let (code, message) = match err {
        WorkspaceError::NotRegistered { path } => (
            "workspace_not_registered",
            format!(
                "Workspace not registered: {}. Pass a known workspace or enable --auto-workspace.",
                path
            ),
        ),
        WorkspaceError::NotAllowed { path, reason } => (
            "workspace_not_allowed",
            format!("Workspace not allowed: {} ({})", path, reason),
        ),
        WorkspaceError::AutoDiscoveryDisabled => (
            "workspace_not_registered",
            "Auto-workspace is disabled. Enable with --auto-workspace.".to_string(),
        ),
        WorkspaceError::LimitExceeded { max } => (
            "workspace_limit_exceeded",
            format!("Maximum auto-discovered workspaces ({}) exceeded.", max),
        ),
        WorkspaceError::AllowedRootRequired => (
            "invalid_input",
            "--allowed-root is required when --auto-workspace is enabled.".to_string(),
        ),
    };

    let error_payload = json!({
        "error": {
            "code": code,
            "message": message,
        }
    });

    tool_calls::tool_text_response(id, error_payload)
}

/// Bootstrap a newly auto-discovered workspace: create project entry, DB, indices,
/// and spawn the indexer subprocess. (T205)
pub fn bootstrap_and_index(
    workspace: &Path,
    project_id: &str,
    data_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create data directory
    std::fs::create_dir_all(data_dir)?;

    // Open SQLite and create schema
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = codecompass_state::db::open_connection(&db_path)?;
    codecompass_state::schema::create_tables(&conn)?;

    // Register project if not already present
    let repo_root_str = workspace.to_string_lossy().to_string();
    if codecompass_state::project::get_by_root(&conn, &repo_root_str)?.is_none() {
        let vcs_mode = workspace.join(".git").exists();
        let default_ref = if vcs_mode {
            codecompass_core::vcs::detect_head_branch(workspace)
                .unwrap_or_else(|_| "main".to_string())
        } else {
            constants::REF_LIVE.to_string()
        };

        let now = codecompass_core::time::now_iso8601();
        let project = codecompass_core::types::Project {
            project_id: project_id.to_string(),
            repo_root: repo_root_str.clone(),
            display_name: workspace
                .file_name()
                .map(|n| n.to_string_lossy().to_string()),
            default_ref,
            vcs_mode,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project)?;

        // Update the workspace entry (registered with NULL project_id) with the real project_id
        let _ = codecompass_state::workspace::update_workspace_project_id(
            &conn,
            &repo_root_str,
            project_id,
        );
    }

    // Create Tantivy index directories
    let _ = codecompass_state::tantivy_index::IndexSet::open(data_dir)?;

    // Spawn indexer subprocess
    let exe = std::env::current_exe().unwrap_or_else(|_| "codecompass".into());
    let workspace_str = workspace.to_string_lossy();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("index")
        .arg("--path")
        .arg(workspace_str.as_ref())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Pass config file path if available in config's data dir
    let config_path = workspace.join(constants::PROJECT_CONFIG_FILE);
    if config_path.exists() {
        cmd.arg("--config").arg(&config_path);
    }

    match cmd.spawn() {
        Ok(child) => {
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });
            info!(
                project_id,
                workspace = %workspace.display(),
                "On-demand indexing started for auto-discovered workspace"
            );
        }
        Err(e) => {
            error!(project_id, "Failed to spawn on-demand indexer: {}", e);
        }
    }

    Ok(())
}

/// Write a JSON-RPC response to the shared writer.
fn write_response(
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
    response: &JsonRpcResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized = serde_json::to_string(response)?;
    let mut w = writer
        .lock()
        .map_err(|e| format!("stdout lock poisoned: {}", e))?;
    writeln!(w, "{}", serialized)?;
    w.flush()?;
    Ok(())
}

struct RequestContext<'a> {
    config: &'a Config,
    index_set: Option<&'a IndexSet>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<&'a str>,
    conn: Option<&'a rusqlite::Connection>,
    workspace: &'a Path,
    project_id: &'a str,
    prewarm_status: &'a AtomicU8,
    server_start: &'a Instant,
    notifier: Arc<dyn ProgressNotifier>,
    progress_token: Option<&'a str>,
}

fn handle_request_with_ctx(request: &JsonRpcRequest, ctx: &RequestContext<'_>) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            request.id.clone(),
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "codecompass",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "notifications/initialized" => JsonRpcResponse::success(request.id.clone(), json!({})),
        "tools/list" => {
            let tools = tools::list_tools();
            JsonRpcResponse::success(request.id.clone(), json!({ "tools": tools }))
        }
        "tools/call" => {
            let tool_name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));

            handle_tool_call(ToolCallParams {
                id: request.id.clone(),
                tool_name,
                arguments: &arguments,
                config: ctx.config,
                index_set: ctx.index_set,
                schema_status: ctx.schema_status,
                compatibility_reason: ctx.compatibility_reason,
                conn: ctx.conn,
                workspace: ctx.workspace,
                project_id: ctx.project_id,
                prewarm_status: ctx.prewarm_status,
                server_start: ctx.server_start,
                notifier: ctx.notifier.clone(),
                progress_token: ctx.progress_token.map(|s| s.to_string()),
            })
        }
        _ => JsonRpcResponse::error(
            request.id.clone(),
            -32601,
            format!("Method not found: {}", request.method),
        ),
    }
}

struct IndexRuntime {
    index_set: Option<IndexSet>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<String>,
}

fn load_index_runtime(data_dir: &Path) -> IndexRuntime {
    match IndexSet::open_existing(data_dir) {
        Ok(index_set) => IndexRuntime {
            index_set: Some(index_set),
            schema_status: SchemaStatus::Compatible,
            compatibility_reason: None,
        },
        Err(err) => {
            let (schema_status, compatibility_reason) = classify_index_open_error(&err);
            IndexRuntime {
                index_set: None,
                schema_status,
                compatibility_reason: Some(compatibility_reason),
            }
        }
    }
}

fn classify_index_open_error(err: &StateError) -> (SchemaStatus, String) {
    match err {
        StateError::Io(io_err) if io_err.kind() == std::io::ErrorKind::NotFound => (
            SchemaStatus::NotIndexed,
            "No index found. Run `codecompass index`.".to_string(),
        ),
        StateError::SchemaMigrationRequired { current, required } => (
            SchemaStatus::ReindexRequired,
            format!(
                "Index schema is incompatible (current={}, required={}).",
                current, required
            ),
        ),
        StateError::CorruptManifest(details) => (
            SchemaStatus::CorruptManifest,
            format!("Index appears corrupted: {}", details),
        ),
        StateError::Tantivy(details) => (
            SchemaStatus::ReindexRequired,
            format!(
                "Tantivy index open failed: {}. Run `codecompass index --force` to rebuild.",
                details
            ),
        ),
        other => (
            SchemaStatus::NotIndexed,
            format!("Index unavailable: {}", other),
        ),
    }
}

/// Check if there's an active indexing job.
fn has_active_job(conn: Option<&rusqlite::Connection>, project_id: &str) -> bool {
    conn.and_then(|c| {
        codecompass_state::jobs::get_active_job(c, project_id)
            .ok()
            .flatten()
    })
    .is_some()
}

/// Build protocol metadata aware of current state.
fn build_metadata(
    r#ref: &str,
    schema_status: SchemaStatus,
    config: &Config,
    conn: Option<&rusqlite::Connection>,
    workspace: &Path,
    project_id: &str,
) -> ProtocolMetadata {
    match schema_status {
        SchemaStatus::NotIndexed => ProtocolMetadata::not_indexed(r#ref),
        SchemaStatus::ReindexRequired => ProtocolMetadata::reindex_required(r#ref),
        SchemaStatus::CorruptManifest => ProtocolMetadata::corrupt_manifest(r#ref),
        SchemaStatus::Compatible => {
            let freshness_result = check_freshness_with_scan_params(
                conn,
                workspace,
                project_id,
                r#ref,
                config.index.max_file_size,
                Some(&config.index.languages),
            );
            let mut metadata = ProtocolMetadata::new(r#ref);
            metadata.freshness_status = freshness::freshness_status(&freshness_result);
            if matches!(freshness_result, FreshnessResult::Syncing) {
                metadata.indexing_status = codecompass_core::types::IndexingStatus::Indexing;
                metadata.result_completeness = codecompass_core::types::ResultCompleteness::Partial;
            }
            metadata
        }
    }
}

fn resolve_project_schema_status(
    config: &Config,
    current_project_id: &str,
    target_project_id: &str,
    current_schema_status: SchemaStatus,
) -> SchemaStatus {
    if target_project_id == current_project_id {
        return current_schema_status;
    }

    let data_dir = config.project_data_dir(target_project_id);
    match IndexSet::open_existing(&data_dir) {
        Ok(_) => SchemaStatus::Compatible,
        Err(e) => classify_index_open_error(&e).0,
    }
}

/// Build protocol metadata using an explicit FreshnessResult (for query tools).
fn build_metadata_with_freshness(
    r#ref: &str,
    schema_status: SchemaStatus,
    freshness_result: &FreshnessResult,
) -> ProtocolMetadata {
    match schema_status {
        SchemaStatus::NotIndexed => ProtocolMetadata::not_indexed(r#ref),
        SchemaStatus::ReindexRequired => ProtocolMetadata::reindex_required(r#ref),
        SchemaStatus::CorruptManifest => ProtocolMetadata::corrupt_manifest(r#ref),
        SchemaStatus::Compatible => {
            let mut metadata = ProtocolMetadata::new(r#ref);
            metadata.freshness_status = freshness::freshness_status(freshness_result);
            if matches!(freshness_result, FreshnessResult::Syncing) {
                metadata.indexing_status = codecompass_core::types::IndexingStatus::Indexing;
                metadata.result_completeness = codecompass_core::types::ResultCompleteness::Partial;
            }
            metadata
        }
    }
}

/// Parse the freshness_policy argument, falling back to config default.
fn resolve_freshness_policy(
    arguments: &Value,
    config: &Config,
) -> codecompass_core::types::FreshnessPolicy {
    arguments
        .get("freshness_policy")
        .and_then(|v| v.as_str())
        .map(parse_freshness_policy)
        .unwrap_or_else(|| parse_freshness_policy(&config.search.freshness_policy))
}

fn is_project_registered(conn: Option<&rusqlite::Connection>, workspace: &Path) -> bool {
    conn.and_then(|c| {
        codecompass_state::project::get_by_root(c, &workspace.to_string_lossy())
            .ok()
            .flatten()
    })
    .is_some()
}

/// Resolve the effective ref used by MCP tools.
///
/// Priority:
/// 1. Explicit `ref` argument
/// 2. Current HEAD branch (if available)
/// 3. Project default_ref from SQLite metadata
/// 4. `live` fallback
fn resolve_tool_ref(
    requested_ref: Option<&str>,
    workspace: &Path,
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> String {
    if let Some(r) = requested_ref {
        return r.to_string();
    }
    if let Ok(branch) = codecompass_core::vcs::detect_head_branch(workspace) {
        return branch;
    }
    if let Some(c) = conn
        && let Ok(Some(project)) = codecompass_state::project::get_by_id(c, project_id)
        && !project.default_ref.trim().is_empty()
    {
        return project.default_ref;
    }
    constants::REF_LIVE.to_string()
}

// ---- Public API for HTTP transport (T223) ----

/// Public wrapper for `resolve_tool_ref` used by the HTTP transport.
pub fn resolve_tool_ref_public(
    requested_ref: Option<&str>,
    workspace: &Path,
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> String {
    resolve_tool_ref(requested_ref, workspace, conn, project_id)
}

/// Public wrapper for `workspace_error_to_response` used by the HTTP transport.
pub fn workspace_error_to_response_public(
    id: Option<Value>,
    err: &WorkspaceError,
) -> JsonRpcResponse {
    workspace_error_to_response(id, err)
}

/// Public wrapper for tool text payload responses used by HTTP transport.
pub fn tool_text_response_public(id: Option<Value>, payload: Value) -> JsonRpcResponse {
    tool_calls::tool_text_response(id, payload)
}

/// Public wrapper for index-open error classification used by HTTP transport.
pub fn classify_index_open_error_public(err: &StateError) -> SchemaStatus {
    classify_index_open_error(err).0
}

/// Parameters for calling tool dispatch from the HTTP transport.
pub struct PublicToolCallParams<'a> {
    pub id: Option<Value>,
    pub tool_name: &'a str,
    pub arguments: &'a Value,
    pub config: &'a Config,
    pub index_set: Option<&'a IndexSet>,
    pub schema_status: SchemaStatus,
    pub compatibility_reason: Option<&'a str>,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
    pub prewarm_status: &'a AtomicU8,
    pub server_start: &'a Instant,
    pub notifier: Arc<dyn crate::notifications::ProgressNotifier>,
    pub progress_token: Option<String>,
}

/// Public wrapper for tool dispatch used by the HTTP transport.
pub fn handle_tool_call_public(params: PublicToolCallParams<'_>) -> JsonRpcResponse {
    handle_tool_call(ToolCallParams {
        id: params.id,
        tool_name: params.tool_name,
        arguments: params.arguments,
        config: params.config,
        index_set: params.index_set,
        schema_status: params.schema_status,
        compatibility_reason: params.compatibility_reason,
        conn: params.conn,
        workspace: params.workspace,
        project_id: params.project_id,
        prewarm_status: params.prewarm_status,
        server_start: params.server_start,
        notifier: params.notifier,
        progress_token: params.progress_token,
    })
}

mod tool_calls;

#[cfg(test)]
mod tests;
