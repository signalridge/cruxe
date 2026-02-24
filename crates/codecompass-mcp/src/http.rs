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
use axum::body::Bytes;
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info};

/// Shared state for the HTTP transport.
pub struct HttpState {
    pub config: Config,
    pub workspace: PathBuf,
    pub project_id: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub prewarm_status: Arc<AtomicU8>,
    pub warmset_enabled: bool,
    pub health_cache: Arc<Mutex<Option<(Instant, Value)>>>,
    pub server_start: Instant,
    pub router: WorkspaceRouter,
}

const HEALTH_CACHE_TTL: Duration = Duration::from_secs(1);

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

    // Mark interrupted jobs from previous session (same as stdio transport)
    if let Ok(conn) = codecompass_state::db::open_connection(&db_path) {
        match codecompass_state::jobs::mark_interrupted_jobs(&conn) {
            Ok(count) if count > 0 => {
                info!(count, "Marked interrupted jobs from previous session");
            }
            _ => {}
        }
    }

    // Create workspace router
    let router = WorkspaceRouter::new(workspace_config, workspace.to_path_buf(), db_path.clone())
        .map_err(|e| format!("workspace config error: {}", e))?;

    // Warmset prewarm
    let prewarm_status = Arc::new(AtomicU8::new(crate::server::PREWARM_PENDING));
    if no_prewarm {
        prewarm_status.store(crate::server::PREWARM_SKIPPED, Ordering::Release);
    } else {
        let ps = Arc::clone(&prewarm_status);
        let config_clone = config.clone();
        let project_ids = crate::server::collect_warmset_project_ids(
            &db_path,
            &project_id,
            crate::server::warmset_capacity(),
        );
        std::thread::spawn(move || crate::server::prewarm_projects(ps, config_clone, project_ids));
    }

    let state = Arc::new(HttpState {
        config,
        workspace: workspace.to_path_buf(),
        project_id,
        data_dir,
        db_path,
        prewarm_status,
        warmset_enabled: !no_prewarm,
        health_cache: Arc::new(Mutex::new(None)),
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
async fn jsonrpc_handler(State(state): State<Arc<HttpState>>, body: Bytes) -> impl IntoResponse {
    let request: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            let body = json!({
                "error": {
                    "code": "invalid_input",
                    "message": format!("Invalid JSON request body: {}", e),
                }
            });
            return (StatusCode::BAD_REQUEST, Json(body)).into_response();
        }
    };

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
    if let Ok(cache) = state.health_cache.lock()
        && let Some((cached_at, payload)) = cache.as_ref()
        && cached_at.elapsed() < HEALTH_CACHE_TTL
    {
        return payload.clone();
    }

    let payload = build_health_response_uncached(state);
    if let Ok(mut cache) = state.health_cache.lock() {
        *cache = Some((Instant::now(), payload.clone()));
    }
    payload
}

fn build_health_response_uncached(state: &HttpState) -> Value {
    let conn = codecompass_state::db::open_connection(&state.db_path).ok();
    let effective_ref = crate::server::resolve_tool_ref_public(
        None,
        &state.workspace,
        conn.as_ref(),
        &state.project_id,
    );

    let pw_status = state.prewarm_status.load(Ordering::Acquire);
    let pw_label = crate::server::prewarm_status_label(pw_status);

    // Load index and DB for health checks
    let index_set = IndexSet::open_existing(&state.data_dir).ok();
    let warmset_capacity = crate::server::warmset_capacity();
    let warmset_members =
        crate::server::collect_warmset_members(conn.as_ref(), &state.workspace, warmset_capacity);

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

    let mut any_project_error = false;
    let mut any_project_indexing = false;
    let mut active_job_payload: Option<Value> = None;
    let mut project_payloads = Vec::new();

    if let Some(c) = conn.as_ref() {
        let mut projects = codecompass_state::project::list_projects(c).unwrap_or_default();
        if projects.is_empty()
            && let Some(p) = codecompass_state::project::get_by_id(c, &state.project_id)
                .ok()
                .flatten()
        {
            projects.push(p);
        }

        for p in projects {
            let project_ref = if p.default_ref.trim().is_empty() {
                constants::REF_LIVE.to_string()
            } else {
                p.default_ref.clone()
            };

            let project_data_dir = state.config.project_data_dir(&p.project_id);
            let project_schema_status = match IndexSet::open_existing(&project_data_dir) {
                Ok(_) => SchemaStatus::Compatible,
                Err(err) => crate::server::classify_index_open_error_public(&err),
            };
            let (project_schema_status_str, _project_compat_message) = match project_schema_status {
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
            let project_current_schema_version = match project_schema_status {
                SchemaStatus::Compatible => constants::SCHEMA_VERSION,
                _ => p.schema_version,
            };

            let active_job = codecompass_state::jobs::get_active_job(c, &p.project_id)
                .ok()
                .flatten();
            if let Some(j) = &active_job {
                any_project_indexing = true;
                if active_job_payload.is_none() {
                    active_job_payload = Some(json!({
                        "job_id": j.job_id,
                        "project_id": j.project_id,
                        "mode": j.mode,
                        "status": j.status,
                        "ref": j.r#ref,
                    }));
                }
            }

            let file_count =
                codecompass_state::manifest::file_count(c, &p.project_id, &project_ref)
                    .unwrap_or(0);
            let symbol_count =
                codecompass_state::symbols::symbol_count(c, &p.project_id, &project_ref)
                    .unwrap_or(0);
            let last_indexed_at: Option<String> =
                codecompass_state::jobs::get_recent_jobs(c, &p.project_id, 10)
                    .ok()
                    .and_then(|jobs| {
                        jobs.into_iter()
                            .find(|j| j.status == "published" && j.r#ref == project_ref)
                            .map(|j| j.updated_at)
                    });

            let project_status = if !matches!(
                project_schema_status,
                SchemaStatus::Compatible | SchemaStatus::NotIndexed
            ) || (p.project_id == state.project_id
                && pw_status == crate::server::PREWARM_FAILED)
            {
                "error"
            } else if p.project_id == state.project_id
                && pw_status == crate::server::PREWARM_IN_PROGRESS
            {
                "warming"
            } else if active_job.is_some() {
                "indexing"
            } else {
                "ready"
            };
            any_project_error |= project_status == "error";

            project_payloads.push(json!({
                "project_id": p.project_id,
                "repo_root": p.repo_root,
                "index_status": project_status,
                "ref": project_ref,
                "file_count": file_count,
                "symbol_count": symbol_count,
                "schema_status": project_schema_status_str,
                "current_schema_version": project_current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "last_indexed_at": last_indexed_at,
            }));
        }
    }

    if project_payloads.is_empty() {
        project_payloads.push(json!({
            "project_id": state.project_id,
            "repo_root": state.workspace.to_string_lossy(),
            "index_status": "error",
            "ref": effective_ref,
            "file_count": 0,
            "symbol_count": 0,
            "schema_status": index_compat_status,
            "current_schema_version": current_schema_version,
            "required_schema_version": constants::SCHEMA_VERSION,
            "last_indexed_at": Value::Null,
        }));
        any_project_error = true;
    }

    let interrupted_jobs = conn
        .as_ref()
        .and_then(|c| codecompass_state::jobs::get_interrupted_jobs(c).ok())
        .unwrap_or_default();
    let interrupted_recovery_report = if interrupted_jobs.is_empty() {
        None
    } else {
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
    };

    // Overall status — priority: error > warming > indexing > ready (per spec)
    let overall_status = if any_project_error
        || pw_status == crate::server::PREWARM_FAILED
        || !matches!(
            schema_status,
            SchemaStatus::Compatible | SchemaStatus::NotIndexed
        ) {
        "error"
    } else if pw_status == crate::server::PREWARM_IN_PROGRESS {
        "warming"
    } else if any_project_indexing {
        "indexing"
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
        "active_job": active_job_payload,
        "interrupted_recovery_report": interrupted_recovery_report,
        "startup_checks": {
            "index": {
                "status": index_compat_status,
                "current_schema_version": current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "message": compat_message,
            }
        },
        "projects": project_payloads,
        "workspace_warmset": {
            "enabled": state.warmset_enabled,
            "capacity": warmset_capacity,
            "members": if state.warmset_enabled { warmset_members } else { Vec::<String>::new() },
        },
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
            let tool_name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Resolve workspace
            let ws_param = request
                .params
                .get("arguments")
                .and_then(|a| a.get("workspace"))
                .and_then(|v| v.as_str());

            let (eff_workspace, eff_project_id, eff_data_dir) = match state
                .router
                .resolve_workspace(ws_param)
            {
                Ok(resolved) => {
                    let eff_data_dir = state.config.project_data_dir(&resolved.project_id);

                    if resolved.on_demand_indexing {
                        if resolved.should_bootstrap
                            && let Err(e) = crate::server::bootstrap_and_index(
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

                        if !crate::server::is_status_tool(tool_name) {
                            let effective_ref =
                                codecompass_core::vcs::detect_head_branch(&resolved.workspace_path)
                                    .unwrap_or_else(|_| constants::REF_LIVE.to_string());
                            let metadata =
                                crate::protocol::ProtocolMetadata::syncing(&effective_ref);
                            let payload = json!({
                                "indexing_status": "indexing",
                                "result_completeness": "partial",
                                "workspace": resolved.workspace_path.to_string_lossy(),
                                "message": "Workspace is being indexed. Results will be available shortly. Use index_status to check progress.",
                                "suggested_next_actions": ["poll index_status", "retry after indexing completes"],
                                "metadata": metadata,
                            });
                            return crate::server::tool_text_response_public(
                                request.id.clone(),
                                payload,
                            );
                        }
                    }
                    (resolved.workspace_path, resolved.project_id, eff_data_dir)
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
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let health = build_health_response(&state);
        assert!(health.get("status").is_some());
        assert!(health.get("version").is_some());
        assert!(health.get("uptime_seconds").is_some());
        assert!(health.get("projects").is_some());
        assert!(health.get("startup_checks").is_some());
        assert!(health.get("workspace_warmset").is_some());
        assert!(health.get("interrupted_recovery_report").is_some());

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
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
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

    #[tokio::test]
    async fn jsonrpc_tools_list_without_content_type_header() {
        use axum::body::{Bytes, to_bytes};

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

        let state = Arc::new(HttpState {
            config,
            workspace: workspace.to_path_buf(),
            project_id,
            data_dir,
            db_path,
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        });

        let response = jsonrpc_handler(
            State(state),
            Bytes::from(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("result").is_some());
    }
}
