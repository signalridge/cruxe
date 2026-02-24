//! HTTP transport for the MCP server (T223-T225).
//!
//! Provides a JSON-RPC over HTTP endpoint that reuses the same tool dispatch
//! as the stdio transport. Routes:
//! - `GET /health` — aggregated health/status
//! - `POST /`      — JSON-RPC MCP handler

use crate::notifications::NullProgressNotifier;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tools;
use crate::workspace_router::WorkspaceRouter;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::types::{SchemaStatus, WorkspaceConfig, generate_project_id};
use codecompass_state::tantivy_index::IndexSet;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

/// Shared state for the HTTP transport.
pub struct HttpState {
    pub config: Config,
    pub workspace: PathBuf,
    pub project_id: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub prewarm_status: Arc<AtomicU8>,
    pub server_start: Instant,
    pub router: WorkspaceRouter,
}

/// Start the HTTP transport server on the given bind address and port.
pub async fn run_http_server(
    workspace: &std::path::Path,
    config_file: Option<&std::path::Path>,
    no_prewarm: bool,
    workspace_config: WorkspaceConfig,
    bind_addr: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load_with_file(Some(workspace), config_file)?;
    let project_id = generate_project_id(&workspace.to_string_lossy());
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);

    // Create workspace router
    let router = WorkspaceRouter::new(workspace_config, workspace.to_path_buf(), db_path.clone())
        .map_err(|e| format!("workspace config error: {}", e))?;

    // Prewarm
    let prewarm_status = Arc::new(AtomicU8::new(crate::server::PREWARM_PENDING));
    if no_prewarm {
        prewarm_status.store(crate::server::PREWARM_SKIPPED, Ordering::Release);
    } else {
        let ps = Arc::clone(&prewarm_status);
        let data_dir_clone = data_dir.clone();
        std::thread::spawn(move || {
            ps.store(crate::server::PREWARM_IN_PROGRESS, Ordering::Release);
            match IndexSet::open_existing(&data_dir_clone) {
                Ok(index_set) => {
                    match codecompass_state::tantivy_index::prewarm_indices(&index_set) {
                        Ok(()) => {
                            info!("Tantivy index prewarm complete");
                            ps.store(crate::server::PREWARM_COMPLETE, Ordering::Release);
                        }
                        Err(e) => {
                            error!("Tantivy index prewarm failed: {}", e);
                            ps.store(crate::server::PREWARM_FAILED, Ordering::Release);
                        }
                    }
                }
                Err(e) => {
                    info!("Skipping prewarm (no indices): {}", e);
                    ps.store(crate::server::PREWARM_SKIPPED, Ordering::Release);
                }
            }
        });
    }

    let state = Arc::new(HttpState {
        config,
        workspace: workspace.to_path_buf(),
        project_id,
        data_dir,
        db_path,
        prewarm_status,
        server_start: Instant::now(),
        router,
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/", post(jsonrpc_handler))
        .with_state(state);

    let addr = format!("{}:{}", bind_addr, port);
    info!("MCP HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// GET /health — aggregated server health (T224).
async fn health_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || build_health_response(&state)
    })
    .await;

    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => {
            let body = json!({"error": format!("internal error: {}", e)});
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

/// POST / — JSON-RPC MCP handler (T225).
async fn jsonrpc_handler(
    State(state): State<Arc<HttpState>>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || handle_http_request(&state, &request)
    })
    .await;

    match result {
        Ok(response) => Json(response).into_response(),
        Err(e) => {
            let resp = JsonRpcResponse::error(None, -32603, format!("Internal error: {}", e));
            Json(resp).into_response()
        }
    }
}

/// Build the /health response.
fn build_health_response(state: &HttpState) -> Value {
    let effective_ref = crate::server::resolve_tool_ref_public(
        None,
        &state.workspace,
        codecompass_state::db::open_connection(&state.db_path)
            .ok()
            .as_ref(),
        &state.project_id,
    );

    let pw_status = state.prewarm_status.load(Ordering::Acquire);
    let pw_label = crate::server::prewarm_status_label(pw_status);

    // Load index and DB for health checks
    let index_set = IndexSet::open_existing(&state.data_dir).ok();
    let conn = codecompass_state::db::open_connection(&state.db_path).ok();

    // Schema status
    let schema_status = match &index_set {
        Some(_) => SchemaStatus::Compatible,
        None => SchemaStatus::NotIndexed,
    };

    let stored_schema_version = conn.as_ref().and_then(|c| {
        codecompass_state::project::get_by_id(c, &state.project_id)
            .ok()
            .flatten()
            .map(|p| p.schema_version)
    });

    let current_schema_version = match schema_status {
        SchemaStatus::Compatible => constants::SCHEMA_VERSION,
        _ => stored_schema_version.unwrap_or(0),
    };

    let (index_compat_status, compat_message) = match schema_status {
        SchemaStatus::Compatible => ("compatible", None),
        SchemaStatus::NotIndexed => ("not_indexed", None),
        SchemaStatus::ReindexRequired => (
            "reindex_required",
            Some("Run `codecompass index --force` to reindex."),
        ),
        SchemaStatus::CorruptManifest => (
            "corrupt_manifest",
            Some("Run `codecompass index --force` to rebuild."),
        ),
    };

    // SQLite health
    let (sqlite_ok, sqlite_error) = conn
        .as_ref()
        .and_then(|c| codecompass_state::db::check_sqlite_health(c).ok())
        .unwrap_or((false, Some("No database connection".into())));

    // Tantivy health
    let tantivy_checks = if let Some(ref idx) = index_set {
        codecompass_state::tantivy_index::check_tantivy_health(idx)
    } else {
        Vec::new()
    };
    let tantivy_ok = !tantivy_checks.is_empty() && tantivy_checks.iter().all(|c| c.ok);

    // Active job
    let active_job = conn.as_ref().and_then(|c| {
        codecompass_state::jobs::get_active_job(c, &state.project_id)
            .ok()
            .flatten()
    });

    // Per-project info
    let (file_count, symbol_count) = conn
        .as_ref()
        .map(|c| {
            let fc = codecompass_state::manifest::file_count(c, &state.project_id, &effective_ref)
                .unwrap_or(0);
            let sc =
                codecompass_state::symbols::symbol_count(c, &state.project_id, &effective_ref)
                    .unwrap_or(0);
            (fc, sc)
        })
        .unwrap_or((0, 0));

    // Overall status
    let overall_status = if active_job.is_some() {
        "indexing"
    } else if pw_status == crate::server::PREWARM_IN_PROGRESS {
        "warming"
    } else if pw_status == crate::server::PREWARM_FAILED
        || !matches!(schema_status, SchemaStatus::Compatible | SchemaStatus::NotIndexed)
    {
        "error"
    } else {
        "ready"
    };

    let uptime_seconds = state.server_start.elapsed().as_secs();

    json!({
        "status": overall_status,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": uptime_seconds,
        "tantivy_ok": tantivy_ok,
        "sqlite_ok": sqlite_ok,
        "sqlite_error": sqlite_error,
        "prewarm_status": pw_label,
        "active_job": active_job.map(|j| json!({
            "job_id": j.job_id,
            "project_id": j.project_id,
            "mode": j.mode,
            "status": j.status,
            "ref": j.r#ref,
        })),
        "startup_checks": {
            "index": {
                "status": index_compat_status,
                "current_schema_version": current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "message": compat_message,
            }
        },
        "projects": [{
            "project_id": state.project_id,
            "repo_root": state.workspace.to_string_lossy(),
            "index_status": overall_status,
            "ref": effective_ref,
            "file_count": file_count,
            "symbol_count": symbol_count,
            "schema_status": index_compat_status,
            "current_schema_version": current_schema_version,
            "required_schema_version": constants::SCHEMA_VERSION,
        }],
    })
}

/// Handle a JSON-RPC request over HTTP by delegating to the same dispatch logic
/// as the stdio transport.
fn handle_http_request(state: &HttpState, request: &JsonRpcRequest) -> JsonRpcResponse {
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
            let tool_list = tools::list_tools();
            JsonRpcResponse::success(request.id.clone(), json!({ "tools": tool_list }))
        }
        "tools/call" => {
            // Resolve workspace
            let ws_param = request
                .params
                .get("arguments")
                .and_then(|a| a.get("workspace"))
                .and_then(|v| v.as_str());

            let (eff_workspace, eff_project_id, eff_data_dir) =
                match state.router.resolve_workspace(ws_param) {
                    Ok(resolved) => {
                        let eff_data_dir = state.config.project_data_dir(&resolved.project_id);
                        (
                            resolved.workspace_path,
                            resolved.project_id,
                            eff_data_dir,
                        )
                    }
                    Err(e) => {
                        return crate::server::workspace_error_to_response_public(
                            request.id.clone(),
                            &e,
                        );
                    }
                };

            let eff_db_path = eff_data_dir.join(constants::STATE_DB_FILE);
            let index_set = IndexSet::open_existing(&eff_data_dir).ok();
            let (schema_status, compatibility_reason) = match &index_set {
                Some(_) => (SchemaStatus::Compatible, None),
                None => (
                    SchemaStatus::NotIndexed,
                    Some("No index found. Run `codecompass index`.".to_string()),
                ),
            };

            let conn = codecompass_state::db::open_connection(&eff_db_path).ok();

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

            // HTTP transport uses NullProgressNotifier (no streaming support)
            let notifier: Arc<dyn crate::notifications::ProgressNotifier> =
                Arc::new(NullProgressNotifier);

            crate::server::handle_tool_call_public(crate::server::PublicToolCallParams {
                id: request.id.clone(),
                tool_name,
                arguments: &arguments,
                config: &state.config,
                index_set: index_set.as_ref(),
                schema_status,
                compatibility_reason: compatibility_reason.as_deref(),
                conn: conn.as_ref(),
                workspace: &eff_workspace,
                project_id: &eff_project_id,
                prewarm_status: &state.prewarm_status,
                server_start: &state.server_start,
                notifier,
                progress_token: None,
            })
        }
        _ => JsonRpcResponse::error(
            request.id.clone(),
            -32601,
            format!("Method not found: {}", request.method),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::types::WorkspaceConfig;

    #[tokio::test]
    async fn health_endpoint_returns_expected_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let config = Config::default();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);

        let router = WorkspaceRouter::new(
            WorkspaceConfig::default(),
            workspace.to_path_buf(),
            db_path.clone(),
        )
        .unwrap();

        let state = HttpState {
            config,
            workspace: workspace.to_path_buf(),
            project_id,
            data_dir,
            db_path,
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            server_start: Instant::now(),
            router,
        };

        let health = build_health_response(&state);
        assert!(health.get("status").is_some());
        assert!(health.get("version").is_some());
        assert!(health.get("uptime_seconds").is_some());
        assert!(health.get("projects").is_some());
        assert!(health.get("startup_checks").is_some());

        // Check per-project compatibility fields
        let projects = health["projects"].as_array().unwrap();
        assert!(!projects.is_empty());
        let proj = &projects[0];
        assert!(proj.get("schema_status").is_some());
        assert!(proj.get("current_schema_version").is_some());
        assert!(proj.get("required_schema_version").is_some());
    }

    #[test]
    fn jsonrpc_tools_list_via_http() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let config = Config::default();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);

        let router = WorkspaceRouter::new(
            WorkspaceConfig::default(),
            workspace.to_path_buf(),
            db_path.clone(),
        )
        .unwrap();

        let state = HttpState {
            config,
            workspace: workspace.to_path_buf(),
            project_id,
            data_dir,
            db_path,
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            server_start: Instant::now(),
            router,
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        };

        let response = handle_http_request(&state, &request);
        let result = response.result.unwrap();
        let tool_array = result["tools"].as_array().unwrap();
        assert!(!tool_array.is_empty());
    }
}
