use crate::protocol::{JsonRpcRequest, JsonRpcResponse, ProtocolMetadata};
use crate::tools;
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::error::StateError;
use codecompass_core::types::{DetailLevel, SchemaStatus, generate_project_id};
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
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;
use tracing::{error, info};

use self::tool_calls::{ToolCallParams, handle_tool_call};

/// Prewarm status values (stored as AtomicU8).
pub const PREWARM_PENDING: u8 = 0;
pub const PREWARM_IN_PROGRESS: u8 = 1;
pub const PREWARM_COMPLETE: u8 = 2;
pub const PREWARM_FAILED: u8 = 3;
pub const PREWARM_SKIPPED: u8 = 4;

/// Convert prewarm status byte to string label.
fn prewarm_status_label(status: u8) -> &'static str {
    match status {
        PREWARM_PENDING => "pending",
        PREWARM_IN_PROGRESS => "warming",
        PREWARM_COMPLETE => "complete",
        PREWARM_FAILED => "failed",
        PREWARM_SKIPPED => "skipped",
        _ => "unknown",
    }
}

/// Run the MCP server loop on stdin/stdout.
pub fn run_server(
    workspace: &Path,
    config_file: Option<&Path>,
    no_prewarm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load_with_file(Some(workspace), config_file)?;
    let project_id = generate_project_id(&workspace.to_string_lossy());
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);
    let server_start = Instant::now();

    // Shared prewarm status
    let prewarm_status = Arc::new(AtomicU8::new(PREWARM_PENDING));

    // Start prewarm in background thread (or skip)
    if no_prewarm {
        prewarm_status.store(PREWARM_SKIPPED, Ordering::Release);
    } else {
        let ps = Arc::clone(&prewarm_status);
        let data_dir_clone = data_dir.clone();
        std::thread::spawn(move || {
            ps.store(PREWARM_IN_PROGRESS, Ordering::Release);
            match IndexSet::open_existing(&data_dir_clone) {
                Ok(index_set) => {
                    match codecompass_state::tantivy_index::prewarm_indices(&index_set) {
                        Ok(()) => {
                            info!("Tantivy index prewarm complete");
                            ps.store(PREWARM_COMPLETE, Ordering::Release);
                        }
                        Err(e) => {
                            error!("Tantivy index prewarm failed: {}", e);
                            ps.store(PREWARM_FAILED, Ordering::Release);
                        }
                    }
                }
                Err(e) => {
                    info!("Skipping prewarm (no indices): {}", e);
                    ps.store(PREWARM_SKIPPED, Ordering::Release);
                }
            }
        });
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

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
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let index_runtime = load_index_runtime(&data_dir);
        let conn = codecompass_state::db::open_connection(&db_path).ok();
        let request_ctx = RequestContext {
            config: &config,
            index_set: index_runtime.index_set.as_ref(),
            schema_status: index_runtime.schema_status,
            compatibility_reason: index_runtime.compatibility_reason.as_deref(),
            conn: conn.as_ref(),
            workspace,
            project_id: &project_id,
            prewarm_status: &prewarm_status,
            server_start: &server_start,
        };
        let response = handle_request_with_ctx(&request, &request_ctx);
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

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

mod tool_calls;

#[cfg(test)]
mod tests;
