use crate::notifications::{McpProgressNotifier, NullProgressNotifier, ProgressNotifier};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, ProtocolMetadata};
use crate::tools;
use crate::workspace_router::WorkspaceRouter;
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::error::{ProtocolErrorCode, StateError, WorkspaceError};
use cruxe_core::types::{DetailLevel, SchemaStatus, WorkspaceConfig, generate_project_id};
use cruxe_query::call_graph;
use cruxe_query::detail;
use cruxe_query::diff_context;
use cruxe_query::explain_ranking;
use cruxe_query::find_references;
use cruxe_query::followup;
use cruxe_query::freshness::{
    self, FreshnessResult, PolicyAction, apply_freshness_policy, check_freshness_with_scan_params,
    parse_freshness_policy, trigger_async_sync,
};
use cruxe_query::hierarchy;
use cruxe_query::locate;
use cruxe_query::ranking;
use cruxe_query::related;
use cruxe_query::search;
use cruxe_query::symbol_compare;
use cruxe_query::tombstone::TombstoneCache;
use cruxe_state::tantivy_index::IndexSet;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use self::tool_calls::{ToolCallParams, handle_tool_call};

/// Prewarm status values (stored as AtomicU8).
pub const PREWARM_PENDING: u8 = 0;
pub const PREWARM_IN_PROGRESS: u8 = 1;
pub const PREWARM_COMPLETE: u8 = 2;
pub const PREWARM_FAILED: u8 = 3;
pub const PREWARM_SKIPPED: u8 = 4;
const DEFAULT_WARMSET_CAPACITY: usize = 3;
const DEFAULT_MAX_OPEN_CONNECTIONS: usize = 32;
const DEFAULT_SESSION_SCOPE: &str = "default";
const SESSION_OVERRIDE_MAX_ENTRIES: usize = 4096;
const SESSION_OVERRIDE_TTL: Duration = Duration::from_secs(12 * 60 * 60);

thread_local! {
    static ACTIVE_SESSION_SCOPE: RefCell<Option<String>> = const { RefCell::new(None) };
}

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
    std::env::var("CRUXE_WARMSET_CAPACITY")
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
    if let Ok(conn) = cruxe_state::db::open_connection(db_path)
        && let Ok(recent) = cruxe_state::workspace::list_recent_workspaces(&conn, capacity)
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
        && let Ok(recent) = cruxe_state::workspace::list_recent_workspaces(c, capacity)
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
                if let Err(e) = cruxe_state::tantivy_index::prewarm_indices(&index_set) {
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

/// Lightweight runtime SQLite connection manager shared across transport handlers.
///
/// Connections are keyed by `db_path` and reused across requests. Callers can
/// invalidate a path entry to force lazy reopen after failures.
pub struct ConnectionManager {
    connections: Mutex<HashMap<std::path::PathBuf, ManagedConnection>>,
    max_open_connections: usize,
}

struct ManagedConnection {
    connection: Arc<Mutex<rusqlite::Connection>>,
    last_accessed_at: Instant,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            max_open_connections: max_open_connections(),
        }
    }

    #[cfg(test)]
    fn with_capacity(max_open_connections: usize) -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            max_open_connections: max_open_connections.max(1),
        }
    }

    pub fn get_or_open(
        &self,
        db_path: &Path,
    ) -> Result<Arc<Mutex<rusqlite::Connection>>, StateError> {
        let mut map = self
            .connections
            .lock()
            .map_err(|e| StateError::sqlite(format!("connection manager lock poisoned: {e}")))?;
        if let Some(existing) = map.get_mut(db_path) {
            existing.last_accessed_at = Instant::now();
            return Ok(Arc::clone(&existing.connection));
        }

        evict_idle_connections(&mut map, self.max_open_connections.saturating_sub(1));
        let conn = cruxe_state::db::open_connection(db_path)?;
        let shared = Arc::new(Mutex::new(conn));
        map.insert(
            db_path.to_path_buf(),
            ManagedConnection {
                connection: Arc::clone(&shared),
                last_accessed_at: Instant::now(),
            },
        );
        Ok(shared)
    }

    pub fn invalidate(&self, db_path: &Path) {
        if let Ok(mut map) = self.connections.lock() {
            map.remove(db_path);
        }
    }

    #[cfg(test)]
    fn cached_connection_count(&self) -> usize {
        self.connections.lock().map(|map| map.len()).unwrap_or(0)
    }

    #[cfg(test)]
    fn contains_connection(&self, db_path: &Path) -> bool {
        self.connections
            .lock()
            .map(|map| map.contains_key(db_path))
            .unwrap_or(false)
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

fn max_open_connections() -> usize {
    std::env::var("CRUXE_MAX_OPEN_CONNECTIONS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_OPEN_CONNECTIONS)
}

fn evict_idle_connections(
    map: &mut HashMap<std::path::PathBuf, ManagedConnection>,
    target_size: usize,
) {
    if map.len() <= target_size {
        return;
    }
    while map.len() > target_size {
        let candidate = map
            .iter()
            .filter(|(_, managed)| {
                // Best-effort "idle" heuristic:
                // when only the manager map holds the Arc, no request currently
                // retains this connection handle.
                // `strong_count` is intentionally used as a soft signal only.
                Arc::strong_count(&managed.connection) == 1
            })
            .min_by_key(|(_, managed)| managed.last_accessed_at)
            .map(|(path, _)| path.clone());
        let Some(path) = candidate else {
            break;
        };
        map.remove(&path);
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
    if let Ok(conn) = cruxe_state::db::open_connection(&db_path) {
        match cruxe_state::jobs::mark_interrupted_jobs(&conn) {
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
    let connection_manager = ConnectionManager::new();

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

        let runtime = DispatchRuntime {
            config: &config,
            router: &router,
            workspace,
            project_id: &project_id,
            data_dir: &data_dir,
            connection_manager: &connection_manager,
            prewarm_status: &prewarm_status,
            server_start: &server_start,
        };
        let transport = TransportExecutionContext {
            notifier,
            progress_token: progress_token.as_deref(),
            // stdio serves a single MCP client per process; keep one shared scope
            // unless transports provide explicit per-session identifiers.
            session_scope: Some("stdio"),
            transport_label: "stdio",
            log_workspace_resolution_failures: true,
            log_degraded_sqlite_open: true,
        };

        let response = execute_transport_request(&request, &runtime, &transport);
        write_response(&writer, &response)?;
    }

    Ok(())
}

/// Convert a workspace resolution error into an MCP tool-level error response.
fn workspace_error_to_response(id: Option<Value>, err: &WorkspaceError) -> JsonRpcResponse {
    let (code, message) = match err {
        WorkspaceError::NotRegistered { path } => (
            ProtocolErrorCode::WorkspaceNotRegistered,
            format!(
                "Workspace not registered: {}. Pass a known workspace or enable --auto-workspace.",
                path
            ),
        ),
        WorkspaceError::NotAllowed { path, reason } => (
            ProtocolErrorCode::WorkspaceNotAllowed,
            format!("Workspace not allowed: {} ({})", path, reason),
        ),
        WorkspaceError::AutoDiscoveryDisabled => (
            ProtocolErrorCode::WorkspaceNotRegistered,
            "Auto-workspace is disabled. Enable with --auto-workspace.".to_string(),
        ),
        WorkspaceError::LimitExceeded { max } => (
            ProtocolErrorCode::WorkspaceLimitExceeded,
            format!("Maximum auto-discovered workspaces ({}) exceeded.", max),
        ),
        WorkspaceError::AllowedRootRequired => (
            ProtocolErrorCode::InvalidInput,
            "--allowed-root is required when --auto-workspace is enabled.".to_string(),
        ),
    };

    let error_payload = json!({
        "error": {
            "code": code.as_str(),
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
    storage_data_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create data directory
    std::fs::create_dir_all(data_dir)?;

    // Open SQLite and create schema
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let conn = cruxe_state::db::open_connection(&db_path)?;
    cruxe_state::schema::create_tables(&conn)?;

    // Register project if not already present
    let repo_root_str = workspace.to_string_lossy().to_string();
    if cruxe_state::project::get_by_root(&conn, &repo_root_str)?.is_none() {
        let vcs_mode = cruxe_core::vcs::is_git_repo(workspace);
        let default_ref = if vcs_mode {
            cruxe_core::vcs::detect_default_ref(workspace, "main")
        } else {
            constants::REF_LIVE.to_string()
        };

        let now = cruxe_core::time::now_iso8601();
        let project = cruxe_core::types::Project {
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
        cruxe_state::project::create_project(&conn, &project)?;

        // Update the workspace entry (registered with NULL project_id) with the real project_id
        let _ =
            cruxe_state::workspace::update_workspace_project_id(&conn, &repo_root_str, project_id);
    }

    // Create Tantivy index directories
    let _ = cruxe_state::tantivy_index::IndexSet::open(data_dir)?;

    // Pass config file path if available in config's data dir
    let config_path = workspace.join(constants::PROJECT_CONFIG_FILE);
    let bootstrap_job_id = crate::index_launcher::generate_job_id();
    let launch_request = crate::index_launcher::IndexLaunchRequest {
        workspace,
        force: false,
        ref_name: None,
        config_path: config_path.exists().then_some(config_path.as_path()),
        project_id: Some(project_id),
        storage_data_dir: Some(storage_data_dir),
        job_id: Some(&bootstrap_job_id),
    };

    match crate::index_launcher::spawn_index_process(&launch_request) {
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

pub struct DispatchRuntime<'a> {
    pub config: &'a Config,
    pub router: &'a WorkspaceRouter,
    pub workspace: &'a Path,
    pub project_id: &'a str,
    pub data_dir: &'a Path,
    pub connection_manager: &'a ConnectionManager,
    pub prewarm_status: &'a AtomicU8,
    pub server_start: &'a Instant,
}

pub struct TransportExecutionContext<'a> {
    pub notifier: Arc<dyn ProgressNotifier>,
    pub progress_token: Option<&'a str>,
    pub session_scope: Option<&'a str>,
    pub transport_label: &'static str,
    pub log_workspace_resolution_failures: bool,
    pub log_degraded_sqlite_open: bool,
}

struct EffectiveWorkspaceContext {
    workspace: std::path::PathBuf,
    project_id: String,
    data_dir: std::path::PathBuf,
}

enum DispatchOutcome {
    Continue(EffectiveWorkspaceContext),
    Response(JsonRpcResponse),
}

fn resolve_tool_call_workspace(
    request: &JsonRpcRequest,
    runtime: &DispatchRuntime<'_>,
    transport: &TransportExecutionContext<'_>,
) -> DispatchOutcome {
    let tool_name = request
        .params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ws_param = request
        .params
        .get("arguments")
        .and_then(|a| a.get("workspace"))
        .and_then(|v| v.as_str());

    match runtime.router.resolve_workspace(ws_param) {
        Ok(resolved) => {
            let eff_data_dir = runtime.config.project_data_dir(&resolved.project_id);
            if resolved.on_demand_indexing {
                if resolved.should_bootstrap
                    && let Err(e) = bootstrap_and_index(
                        &resolved.workspace_path,
                        &resolved.project_id,
                        &eff_data_dir,
                        &runtime.config.storage.data_dir,
                    )
                {
                    error!(
                        workspace = %resolved.workspace_path.display(),
                        "on-demand bootstrap failed: {}", e
                    );
                }

                if !is_status_tool(tool_name) {
                    let effective_ref = cruxe_core::vcs::detect_default_ref(
                        &resolved.workspace_path,
                        constants::REF_LIVE,
                    );
                    let metadata = ProtocolMetadata::syncing(&effective_ref);
                    return DispatchOutcome::Response(tool_calls::tool_text_response(
                        request.id.clone(),
                        json!({
                            "indexing_status": "indexing",
                            "result_completeness": "partial",
                            "workspace": resolved.workspace_path.to_string_lossy(),
                            "message": "Workspace is being indexed. Results will be available shortly. Use index_status to check progress.",
                            "suggested_next_actions": ["poll index_status", "retry after indexing completes"],
                            "metadata": metadata,
                        }),
                    ));
                }
            }

            DispatchOutcome::Continue(EffectiveWorkspaceContext {
                workspace: resolved.workspace_path,
                project_id: resolved.project_id,
                data_dir: eff_data_dir,
            })
        }
        Err(e) => {
            if transport.log_workspace_resolution_failures {
                warn!(
                    transport = transport.transport_label,
                    tool = tool_name,
                    workspace_param = ?ws_param,
                    error = %e,
                    "Workspace resolution failed"
                );
            }
            DispatchOutcome::Response(workspace_error_to_response(request.id.clone(), &e))
        }
    }
}

pub fn execute_transport_request(
    request: &JsonRpcRequest,
    runtime: &DispatchRuntime<'_>,
    transport: &TransportExecutionContext<'_>,
) -> JsonRpcResponse {
    let _session_scope_guard = set_active_session_scope(transport.session_scope);
    let mut effective_workspace = runtime.workspace.to_path_buf();
    let mut effective_project_id = runtime.project_id.to_string();
    let mut effective_data_dir = runtime.data_dir.to_path_buf();

    if request.method == "tools/call" {
        match resolve_tool_call_workspace(request, runtime, transport) {
            DispatchOutcome::Continue(ctx) => {
                effective_workspace = ctx.workspace;
                effective_project_id = ctx.project_id;
                effective_data_dir = ctx.data_dir;
            }
            DispatchOutcome::Response(response) => return response,
        }
    }

    let eff_db_path = effective_data_dir.join(constants::STATE_DB_FILE);
    let index_runtime = load_index_runtime(&effective_data_dir);
    let conn_handle = match runtime.connection_manager.get_or_open(&eff_db_path) {
        Ok(handle) => Some(handle),
        Err(err) => {
            if transport.log_degraded_sqlite_open {
                warn!(
                    transport = transport.transport_label,
                    db_path = %eff_db_path.display(),
                    project_id = %effective_project_id,
                    error = %err,
                    "Failed to open sqlite connection; proceeding with degraded compatibility behavior"
                );
            }
            runtime.connection_manager.invalidate(&eff_db_path);
            None
        }
    };
    let conn_guard = conn_handle
        .as_ref()
        .and_then(|handle| match handle.lock() {
            Ok(guard) => Some(guard),
            Err(err) => {
                if transport.log_degraded_sqlite_open {
                    warn!(
                        transport = transport.transport_label,
                        db_path = %eff_db_path.display(),
                        project_id = %effective_project_id,
                        error = %err,
                        "Failed to lock sqlite connection; proceeding with degraded compatibility behavior"
                    );
                }
                runtime.connection_manager.invalidate(&eff_db_path);
                None
            }
        });

    let request_ctx = RequestContext {
        config: runtime.config,
        index_set: index_runtime.index_set.as_ref(),
        schema_status: index_runtime.schema_status,
        compatibility_reason: index_runtime.compatibility_reason.as_deref(),
        conn: conn_guard.as_deref(),
        workspace: &effective_workspace,
        project_id: &effective_project_id,
        prewarm_status: runtime.prewarm_status,
        server_start: runtime.server_start,
        notifier: transport.notifier.clone(),
        progress_token: transport.progress_token,
    };
    handle_request_with_ctx(request, &request_ctx)
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
                    "name": "cruxe",
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
            "No index found. Run `cruxe index`.".to_string(),
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
                "Tantivy index open failed: {}. Run `cruxe index --force` to rebuild.",
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
        cruxe_state::jobs::get_active_job(c, project_id)
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
                metadata.indexing_status = cruxe_core::types::IndexingStatus::Indexing;
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Partial;
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

pub(crate) fn schema_status_contract(
    schema_status: SchemaStatus,
) -> (&'static str, Option<&'static str>) {
    match schema_status {
        SchemaStatus::Compatible => ("compatible", None),
        SchemaStatus::NotIndexed => ("not_indexed", None),
        SchemaStatus::ReindexRequired => (
            "reindex_required",
            Some("Run `cruxe index --force` to reindex."),
        ),
        SchemaStatus::CorruptManifest => (
            "corrupt_manifest",
            Some("Run `cruxe index --force` to rebuild."),
        ),
    }
}

pub(crate) fn schema_status_current_version(
    schema_status: SchemaStatus,
    stored_schema_version: u32,
) -> u32 {
    match schema_status {
        SchemaStatus::Compatible => constants::SCHEMA_VERSION,
        _ => stored_schema_version,
    }
}

pub(crate) fn build_interrupted_recovery_report(
    conn: Option<&rusqlite::Connection>,
) -> Option<Value> {
    let interrupted_jobs = conn
        .and_then(|c| cruxe_state::jobs::get_interrupted_jobs(c).ok())
        .unwrap_or_default();
    if interrupted_jobs.is_empty() {
        return None;
    }
    let last_interrupted_at = interrupted_jobs
        .iter()
        .map(|j| j.updated_at.as_str())
        .max()
        .unwrap_or_default();
    Some(json!({
        "detected": true,
        "interrupted_jobs": interrupted_jobs.len(),
        "last_interrupted_at": last_interrupted_at,
        "recommended_action": "run sync_repo or index_repo for the affected workspace",
    }))
}

pub(crate) struct HealthProjectStatus {
    pub index_status: &'static str,
    pub is_error: bool,
    pub is_warming: bool,
}

pub(crate) fn health_project_status(
    project_schema_status: SchemaStatus,
    is_primary_project: bool,
    prewarm_status: u8,
    has_active_job: bool,
) -> HealthProjectStatus {
    let has_schema_error = !matches!(project_schema_status, SchemaStatus::Compatible);
    let prewarm_failed_for_project = is_primary_project && prewarm_status == PREWARM_FAILED;
    if has_schema_error || prewarm_failed_for_project {
        return HealthProjectStatus {
            index_status: "error",
            is_error: true,
            is_warming: false,
        };
    }
    if is_primary_project && prewarm_status == PREWARM_IN_PROGRESS {
        return HealthProjectStatus {
            index_status: "warming",
            is_error: false,
            is_warming: true,
        };
    }
    if has_active_job {
        return HealthProjectStatus {
            index_status: "indexing",
            is_error: false,
            is_warming: false,
        };
    }
    HealthProjectStatus {
        index_status: "ready",
        is_error: false,
        is_warming: false,
    }
}

pub(crate) fn health_overall_status(
    any_project_error: bool,
    any_project_warming: bool,
    any_project_indexing: bool,
    schema_status: SchemaStatus,
    prewarm_status: u8,
) -> &'static str {
    if any_project_error
        || prewarm_status == PREWARM_FAILED
        || !matches!(schema_status, SchemaStatus::Compatible)
    {
        "error"
    } else if any_project_warming || prewarm_status == PREWARM_IN_PROGRESS {
        "warming"
    } else if any_project_indexing {
        "indexing"
    } else {
        "ready"
    }
}

pub(crate) struct HealthCoreOptions {
    pub workspace_scoped: bool,
    pub include_freshness_status: bool,
    pub include_extended_active_job_fields: bool,
}

pub(crate) struct HealthCoreRequest<'a> {
    pub config: &'a Config,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
    pub schema_status: SchemaStatus,
    pub prewarm_status: u8,
    pub effective_ref: &'a str,
    pub options: HealthCoreOptions,
}

pub(crate) struct HealthCorePayload {
    pub projects: Vec<Value>,
    pub active_job: Option<Value>,
    pub overall_status: &'static str,
    pub startup_index_status: &'static str,
    pub startup_compat_message: Option<&'static str>,
    pub startup_current_schema_version: u32,
    pub interrupted_recovery_report: Option<Value>,
}

pub(crate) fn build_health_core_payload(request: HealthCoreRequest<'_>) -> HealthCorePayload {
    let HealthCoreRequest {
        config,
        conn,
        workspace,
        project_id,
        schema_status,
        prewarm_status,
        effective_ref,
        options,
    } = request;
    let stored_schema_version = conn.and_then(|c| {
        cruxe_state::project::get_by_id(c, project_id)
            .ok()
            .flatten()
            .map(|p| p.schema_version)
    });
    let startup_current_schema_version =
        schema_status_current_version(schema_status, stored_schema_version.unwrap_or(0));
    let (startup_index_status, startup_compat_message) = schema_status_contract(schema_status);

    let mut any_project_error = false;
    let mut any_project_warming = false;
    let mut any_project_indexing = false;
    let mut active_job_payload: Option<Value> = None;
    let mut project_payloads = Vec::new();

    if let Some(c) = conn {
        let mut projects = if options.workspace_scoped {
            cruxe_state::project::get_by_id(c, project_id)
                .ok()
                .flatten()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            cruxe_state::project::list_projects(c).unwrap_or_default()
        };
        if projects.is_empty()
            && let Some(p) = cruxe_state::project::get_by_id(c, project_id)
                .ok()
                .flatten()
        {
            projects.push(p);
        }

        for p in projects {
            let project_workspace = Path::new(&p.repo_root);
            let project_ref = if p.default_ref.trim().is_empty() {
                constants::REF_LIVE.to_string()
            } else {
                p.default_ref.clone()
            };
            let project_schema_status =
                resolve_project_schema_status(config, project_id, &p.project_id, schema_status);
            let (project_schema_status_str, _) = schema_status_contract(project_schema_status);
            let project_current_schema_version =
                schema_status_current_version(project_schema_status, p.schema_version);

            let active_job = cruxe_state::jobs::get_active_job(c, &p.project_id)
                .ok()
                .flatten();
            if let Some(j) = &active_job {
                any_project_indexing = true;
                if active_job_payload.is_none() {
                    let mut payload = json!({
                        "job_id": j.job_id,
                        "project_id": j.project_id,
                        "mode": j.mode,
                        "status": j.status,
                        "ref": j.r#ref,
                    });
                    if options.include_extended_active_job_fields {
                        payload["changed_files"] = json!(j.changed_files);
                        payload["started_at"] = json!(j.created_at.clone());
                    }
                    active_job_payload = Some(payload);
                }
            }

            let project_status = health_project_status(
                project_schema_status,
                p.project_id == project_id,
                prewarm_status,
                active_job.is_some(),
            );
            any_project_error |= project_status.is_error;
            any_project_warming |= project_status.is_warming;

            let file_count =
                cruxe_state::manifest::file_count(c, &p.project_id, &project_ref).unwrap_or(0);
            let symbol_count =
                cruxe_state::symbols::symbol_count(c, &p.project_id, &project_ref).unwrap_or(0);
            let last_indexed_at = cruxe_state::jobs::get_recent_jobs(c, &p.project_id, 10)
                .ok()
                .and_then(|jobs| {
                    jobs.into_iter()
                        .find(|j| j.status == "published" && j.r#ref == project_ref)
                        .map(|j| j.updated_at)
                });

            let mut project_payload = json!({
                "project_id": p.project_id,
                "repo_root": p.repo_root,
                "index_status": project_status.index_status,
                "last_indexed_at": last_indexed_at,
                "ref": project_ref,
                "file_count": file_count,
                "symbol_count": symbol_count,
                "schema_status": project_schema_status_str,
                "current_schema_version": project_current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
            });
            if options.include_freshness_status {
                let freshness_result = check_freshness_with_scan_params(
                    Some(c),
                    project_workspace,
                    &p.project_id,
                    project_payload["ref"]
                        .as_str()
                        .unwrap_or(constants::REF_LIVE),
                    config.index.max_file_size,
                    Some(&config.index.languages),
                );
                project_payload["freshness_status"] =
                    json!(freshness::freshness_status(&freshness_result));
            }
            project_payloads.push(project_payload);
        }
    }

    if project_payloads.is_empty() {
        let fallback_status = if conn.is_none()
            || !matches!(schema_status, SchemaStatus::Compatible)
            || prewarm_status == PREWARM_FAILED
        {
            any_project_error = true;
            "error"
        } else if prewarm_status == PREWARM_IN_PROGRESS {
            any_project_warming = true;
            "warming"
        } else {
            "ready"
        };
        let (fallback_schema_status, _) = schema_status_contract(schema_status);
        let fallback_current_schema_version =
            schema_status_current_version(schema_status, stored_schema_version.unwrap_or(0));
        let mut fallback_payload = json!({
            "project_id": project_id,
            "repo_root": workspace.to_string_lossy(),
            "index_status": fallback_status,
            "last_indexed_at": Value::Null,
            "ref": effective_ref,
            "file_count": conn
                .and_then(|c| cruxe_state::manifest::file_count(c, project_id, effective_ref).ok())
                .unwrap_or(0),
            "symbol_count": conn
                .and_then(|c| cruxe_state::symbols::symbol_count(c, project_id, effective_ref).ok())
                .unwrap_or(0),
            "schema_status": fallback_schema_status,
            "current_schema_version": fallback_current_schema_version,
            "required_schema_version": constants::SCHEMA_VERSION,
        });
        if options.include_freshness_status {
            fallback_payload["freshness_status"] = json!(
                build_metadata(
                    effective_ref,
                    schema_status,
                    config,
                    conn,
                    workspace,
                    project_id,
                )
                .freshness_status
            );
        }
        project_payloads.push(fallback_payload);
    }

    HealthCorePayload {
        overall_status: health_overall_status(
            any_project_error,
            any_project_warming,
            any_project_indexing,
            schema_status,
            prewarm_status,
        ),
        projects: project_payloads,
        active_job: active_job_payload,
        startup_index_status,
        startup_compat_message,
        startup_current_schema_version,
        interrupted_recovery_report: build_interrupted_recovery_report(conn),
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
                metadata.indexing_status = cruxe_core::types::IndexingStatus::Indexing;
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Partial;
            }
            metadata
        }
    }
}

/// Parse the freshness_policy argument, falling back to config default.
fn resolve_freshness_policy(
    arguments: &Value,
    config: &Config,
) -> cruxe_core::types::FreshnessPolicy {
    arguments
        .get("freshness_policy")
        .and_then(|v| v.as_str())
        .map(parse_freshness_policy)
        .unwrap_or_else(|| config.search.freshness_policy_typed())
}

fn is_project_registered(conn: Option<&rusqlite::Connection>, workspace: &Path) -> bool {
    conn.and_then(|c| {
        cruxe_state::project::get_by_root(c, &workspace.to_string_lossy())
            .ok()
            .flatten()
    })
    .is_some()
}

#[derive(Clone)]
struct SessionRefOverrideEntry {
    ref_name: String,
    last_touched_at: Instant,
}

struct SessionScopeGuard {
    previous: Option<String>,
}

impl Drop for SessionScopeGuard {
    fn drop(&mut self) {
        ACTIVE_SESSION_SCOPE.with(|scope| {
            scope.replace(self.previous.take());
        });
    }
}

fn normalize_session_scope(session_scope: Option<&str>) -> Option<String> {
    session_scope.and_then(|scope| {
        let trimmed = scope.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn set_active_session_scope(session_scope: Option<&str>) -> SessionScopeGuard {
    let normalized = normalize_session_scope(session_scope);
    let previous = ACTIVE_SESSION_SCOPE.with(|scope| scope.replace(normalized));
    SessionScopeGuard { previous }
}

fn current_session_scope() -> Option<String> {
    ACTIVE_SESSION_SCOPE.with(|scope| scope.borrow().clone())
}

fn session_ref_key(workspace: &Path, project_id: &str) -> String {
    let scope = current_session_scope().unwrap_or_else(|| DEFAULT_SESSION_SCOPE.to_string());
    format!("{scope}::{project_id}::{}", workspace.to_string_lossy())
}

fn session_ref_overrides() -> &'static Mutex<HashMap<String, SessionRefOverrideEntry>> {
    static SESSION_REF_OVERRIDES: OnceLock<Mutex<HashMap<String, SessionRefOverrideEntry>>> =
        OnceLock::new();
    SESSION_REF_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_session_ref_override(workspace: &Path, project_id: &str) -> Option<String> {
    let key = session_ref_key(workspace, project_id);
    let mut guard = session_ref_overrides().lock().ok()?;
    let now = Instant::now();
    prune_expired_session_overrides(&mut guard, now);
    let entry = guard.get_mut(&key)?;
    entry.last_touched_at = now;
    Some(entry.ref_name.clone())
}

pub(crate) fn set_session_ref_override(
    workspace: &Path,
    project_id: &str,
    ref_name: &str,
) -> Result<(), StateError> {
    let key = session_ref_key(workspace, project_id);
    let mut guard = session_ref_overrides()
        .lock()
        .map_err(|err| StateError::sqlite(format!("session ref lock poisoned: {err}")))?;
    let now = Instant::now();
    prune_expired_session_overrides(&mut guard, now);
    guard.insert(
        key,
        SessionRefOverrideEntry {
            ref_name: ref_name.to_string(),
            last_touched_at: now,
        },
    );
    enforce_session_override_capacity(&mut guard);
    Ok(())
}

pub(crate) fn clear_session_ref_override(
    workspace: &Path,
    project_id: &str,
) -> Result<(), StateError> {
    let key = session_ref_key(workspace, project_id);
    let mut guard = session_ref_overrides()
        .lock()
        .map_err(|err| StateError::sqlite(format!("session ref lock poisoned: {err}")))?;
    guard.remove(&key);
    Ok(())
}

fn prune_expired_session_overrides(
    entries: &mut HashMap<String, SessionRefOverrideEntry>,
    now: Instant,
) {
    entries.retain(|_, entry| now.duration_since(entry.last_touched_at) <= SESSION_OVERRIDE_TTL);
}

fn enforce_session_override_capacity(entries: &mut HashMap<String, SessionRefOverrideEntry>) {
    while entries.len() > SESSION_OVERRIDE_MAX_ENTRIES {
        let oldest_key = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_touched_at)
            .map(|(key, _)| key.clone());
        let Some(oldest_key) = oldest_key else {
            break;
        };
        entries.remove(&oldest_key);
    }
}

/// Resolve the effective ref used by MCP tools.
///
/// Priority:
/// 1. Explicit `ref` argument
/// 2. Session `switch_ref` override (process-local, non-persistent)
/// 3. Current HEAD branch (if available)
/// 4. Project default_ref from SQLite metadata
/// 5. `live` fallback
fn resolve_tool_ref(
    requested_ref: Option<&str>,
    workspace: &Path,
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
) -> String {
    if let Some(r) = requested_ref {
        return r.to_string();
    }
    if let Some(session_ref) = get_session_ref_override(workspace, project_id) {
        return session_ref;
    }
    if let Ok(branch) = cruxe_core::vcs::detect_head_branch(workspace) {
        return branch;
    }
    if let Some(c) = conn
        && let Ok(Some(project)) = cruxe_state::project::get_by_id(c, project_id)
        && !project.default_ref.trim().is_empty()
    {
        return project.default_ref;
    }
    constants::REF_LIVE.to_string()
}

// ---- Public API for HTTP transport (T223) ----

/// Public runtime compatibility bundle for HTTP transport request routing.
pub struct PublicIndexRuntime {
    pub index_set: Option<IndexSet>,
    pub schema_status: SchemaStatus,
    pub compatibility_reason: Option<String>,
}

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

/// Public wrapper for loading index/runtime compatibility used by HTTP transport.
pub fn load_index_runtime_public(data_dir: &Path) -> PublicIndexRuntime {
    let runtime = load_index_runtime(data_dir);
    PublicIndexRuntime {
        index_set: runtime.index_set,
        schema_status: runtime.schema_status,
        compatibility_reason: runtime.compatibility_reason,
    }
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
