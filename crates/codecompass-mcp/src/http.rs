//! HTTP transport for the MCP server (T223-T225).
//!
//! Provides a JSON-RPC over HTTP endpoint that reuses the same tool dispatch
//! as the stdio transport. Routes:
//! - `GET /health` — aggregated health/status
//! - `POST /`      — JSON-RPC MCP handler

use crate::notifications::{NullProgressNotifier, ProgressNotifier};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::workspace_router::WorkspaceRouter;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::error::ProtocolErrorCode;
use codecompass_core::types::{SchemaStatus, WorkspaceConfig, generate_project_id};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Shared state for the HTTP transport.
pub struct HttpState {
    pub config: Config,
    pub workspace: PathBuf,
    pub project_id: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub connection_manager: Arc<crate::server::ConnectionManager>,
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
        connection_manager: Arc::new(crate::server::ConnectionManager::new()),
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
async fn jsonrpc_handler(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let request: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            let body = json!({
                "error": {
                    "code": ProtocolErrorCode::InvalidInput.as_str(),
                    "message": format!("Invalid JSON request body: {}", e),
                }
            });
            return (StatusCode::BAD_REQUEST, Json(body)).into_response();
        }
    };
    let session_scope = session_scope_from_headers(&headers);

    let result = tokio::task::spawn_blocking({
        let state = Arc::clone(&state);
        move || handle_http_request(&state, &request, session_scope.as_deref())
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

fn session_scope_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .or_else(|| headers.get("x-codecompass-session"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
    let conn_handle = match state.connection_manager.get_or_open(&state.db_path) {
        Ok(handle) => Some(handle),
        Err(err) => {
            warn!(
                db_path = %state.db_path.display(),
                error = %err,
                "Failed to open sqlite connection for /health; serving degraded payload"
            );
            state.connection_manager.invalidate(&state.db_path);
            None
        }
    };
    let conn_guard = conn_handle.as_ref().and_then(|handle| match handle.lock() {
        Ok(guard) => Some(guard),
        Err(err) => {
            warn!(
                db_path = %state.db_path.display(),
                error = %err,
                "Failed to lock sqlite connection for /health; serving degraded payload"
            );
            state.connection_manager.invalidate(&state.db_path);
            None
        }
    });
    let conn = conn_guard.as_deref();
    let effective_ref =
        crate::server::resolve_tool_ref_public(None, &state.workspace, conn, &state.project_id);

    let pw_status = state.prewarm_status.load(Ordering::Acquire);
    let pw_label = crate::server::prewarm_status_label(pw_status);

    // Load index/runtime compatibility for health checks.
    let runtime = crate::server::load_index_runtime_public(&state.data_dir);
    let index_set = runtime.index_set;
    let schema_status = runtime.schema_status;
    if !matches!(schema_status, SchemaStatus::Compatible) {
        warn!(
            data_dir = %state.data_dir.display(),
            schema_status = ?schema_status,
            compatibility_reason = ?runtime.compatibility_reason,
            "Index runtime is not fully compatible for /health"
        );
    }
    let warmset_capacity = crate::server::warmset_capacity();
    let warmset_members =
        crate::server::collect_warmset_members(conn, &state.workspace, warmset_capacity);

    // SQLite health
    let (sqlite_ok, sqlite_error) = conn
        .and_then(|c| codecompass_state::db::check_sqlite_health(c).ok())
        .unwrap_or((false, Some("No database connection".into())));

    // Tantivy health
    let tantivy_checks = if let Some(ref idx) = index_set {
        codecompass_state::tantivy_index::check_tantivy_health(idx)
    } else {
        Vec::new()
    };
    let tantivy_ok = !tantivy_checks.is_empty() && tantivy_checks.iter().all(|c| c.ok);

    let health_core = crate::server::build_health_core_payload(crate::server::HealthCoreRequest {
        config: &state.config,
        conn,
        workspace: &state.workspace,
        project_id: &state.project_id,
        schema_status,
        prewarm_status: pw_status,
        effective_ref: &effective_ref,
        options: crate::server::HealthCoreOptions {
            workspace_scoped: false,
            include_freshness_status: false,
            include_extended_active_job_fields: false,
        },
    });

    let uptime_seconds = state.server_start.elapsed().as_secs();

    json!({
        "status": health_core.overall_status,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": uptime_seconds,
        "tantivy_ok": tantivy_ok,
        "sqlite_ok": sqlite_ok,
        "sqlite_error": sqlite_error,
        "prewarm_status": pw_label,
        "active_job": health_core.active_job,
        "interrupted_recovery_report": health_core.interrupted_recovery_report,
        "startup_checks": {
            "index": {
                "status": health_core.startup_index_status,
                "current_schema_version": health_core.startup_current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "message": health_core.startup_compat_message,
            }
        },
        "projects": health_core.projects,
        "workspace_warmset": {
            "enabled": state.warmset_enabled,
            "capacity": warmset_capacity,
            "members": if state.warmset_enabled { warmset_members } else { Vec::<String>::new() },
        },
    })
}

/// Handle a JSON-RPC request over HTTP by delegating to the same dispatch logic
/// as the stdio transport.
fn handle_http_request(
    state: &HttpState,
    request: &JsonRpcRequest,
    session_scope: Option<&str>,
) -> JsonRpcResponse {
    let runtime = crate::server::DispatchRuntime {
        config: &state.config,
        router: &state.router,
        workspace: &state.workspace,
        project_id: &state.project_id,
        data_dir: &state.data_dir,
        connection_manager: &state.connection_manager,
        prewarm_status: &state.prewarm_status,
        server_start: &state.server_start,
    };
    let transport = crate::server::TransportExecutionContext {
        notifier: Arc::new(NullProgressNotifier) as Arc<dyn ProgressNotifier>,
        progress_token: None,
        session_scope,
        transport_label: "http",
        log_workspace_resolution_failures: true,
        log_degraded_sqlite_open: true,
    };
    crate::server::execute_transport_request(request, &runtime, &transport)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::types::{AllowedRoots, Project, WorkspaceConfig};
    use std::time::Duration;

    fn build_fixture_index_at(data_dir: &std::path::Path) {
        use codecompass_indexer::{
            import_extract, languages, parser, scanner, snippet_extract, symbol_extract, writer,
        };

        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/rust-sample");
        assert!(
            fixture_dir.exists(),
            "fixture directory missing: {}",
            fixture_dir.display()
        );
        std::fs::create_dir_all(data_dir).unwrap();
        let index_set = codecompass_state::tantivy_index::IndexSet::open(data_dir).unwrap();

        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();

        let scanned = scanner::scan_directory(&fixture_dir, 1_048_576);
        assert!(!scanned.is_empty(), "scanner found no fixture files");

        let repo = "test-repo";
        let r#ref = "live";
        let mut pending_imports = Vec::new();

        for file in &scanned {
            let source = std::fs::read_to_string(&file.path).unwrap();
            let tree = match parser::parse_file(&source, &file.language) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let extracted = languages::extract_symbols(&tree, &source, &file.language);
            let raw_imports = import_extract::extract_imports(
                &tree,
                &source,
                &file.language,
                &file.relative_path,
            );
            let symbols = symbol_extract::build_symbol_records(
                &extracted,
                repo,
                r#ref,
                &file.relative_path,
                None,
            );
            let snippets = snippet_extract::build_snippet_records(
                &extracted,
                repo,
                r#ref,
                &file.relative_path,
                None,
            );
            let content_hash = blake3::hash(source.as_bytes()).to_hex().to_string();
            let filename = file.path.file_name().unwrap().to_string_lossy().to_string();
            let file_record = codecompass_core::types::FileRecord {
                repo: repo.to_string(),
                r#ref: r#ref.to_string(),
                commit: None,
                path: file.relative_path.clone(),
                filename,
                language: file.language.clone(),
                content_hash,
                size_bytes: source.len() as u64,
                updated_at: "2026-01-01T00:00:00Z".to_string(),
                content_head: source
                    .lines()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into(),
            };
            writer::write_file_records(&index_set, &conn, &symbols, &snippets, &file_record)
                .unwrap();
            pending_imports.push((file.relative_path.clone(), raw_imports));
        }

        for (path, raw_imports) in pending_imports {
            writer::replace_import_edges_for_file(&conn, repo, r#ref, &path, raw_imports).unwrap();
        }
    }

    fn extract_payload(response: &JsonRpcResponse) -> Value {
        let result = response.result.as_ref().expect("result should be present");
        let content = result
            .get("content")
            .expect("result should contain content")
            .as_array()
            .expect("content should be array");
        let text = content[0]["text"].as_str().expect("tool text payload");
        serde_json::from_str(text).expect("payload should be valid json")
    }

    fn build_test_state(workspace: &std::path::Path, config: Config) -> HttpState {
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let router = WorkspaceRouter::new(
            WorkspaceConfig::default(),
            workspace.to_path_buf(),
            db_path.clone(),
        )
        .unwrap();
        HttpState {
            config,
            workspace: workspace.to_path_buf(),
            project_id,
            data_dir,
            db_path,
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        }
    }

    fn dispatch_stdio_equivalent(state: &HttpState, request: &JsonRpcRequest) -> JsonRpcResponse {
        let runtime = crate::server::DispatchRuntime {
            config: &state.config,
            router: &state.router,
            workspace: &state.workspace,
            project_id: &state.project_id,
            data_dir: &state.data_dir,
            connection_manager: &state.connection_manager,
            prewarm_status: &state.prewarm_status,
            server_start: &state.server_start,
        };
        let transport = crate::server::TransportExecutionContext {
            notifier: Arc::new(NullProgressNotifier),
            progress_token: None,
            session_scope: Some("stdio-test"),
            transport_label: "stdio-test",
            log_workspace_resolution_failures: false,
            log_degraded_sqlite_open: false,
        };
        crate::server::execute_transport_request(request, &runtime, &transport)
    }

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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let health = build_health_response(&state);
        assert!(health.get("status").is_some());
        assert_eq!(
            health.get("status").and_then(Value::as_str),
            Some("error"),
            "unindexed workspace should surface error status in health"
        );
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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
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

        let response = handle_http_request(&state, &request, None);
        let result = response.result.unwrap();
        let tool_array = result["tools"].as_array().unwrap();
        assert!(!tool_array.is_empty());
    }

    #[test]
    fn session_scope_prefers_mcp_session_id_header() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert("mcp-session-id", HeaderValue::from_static("session-abc"));
        headers.insert(
            "x-codecompass-session",
            HeaderValue::from_static("fallback"),
        );
        assert_eq!(
            session_scope_from_headers(&headers).as_deref(),
            Some("session-abc")
        );
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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        });

        let response = jsonrpc_handler(
            State(state),
            HeaderMap::new(),
            Bytes::from(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed.get("result").is_some());
    }

    #[tokio::test]
    async fn jsonrpc_invalid_json_returns_bad_request() {
        use axum::body::{Bytes, to_bytes};

        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(build_test_state(tmp.path(), Config::default()));

        let response =
            jsonrpc_handler(State(state), HeaderMap::new(), Bytes::from("{invalid-json"))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        let err = parsed.get("error").expect("error payload should exist");
        assert_eq!(
            err.get("code").and_then(Value::as_str),
            Some("invalid_input"),
            "invalid JSON must map to canonical invalid_input code"
        );
    }

    #[test]
    fn jsonrpc_unknown_method_returns_method_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_test_state(tmp.path(), Config::default());

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(42)),
            method: "unknown/method".into(),
            params: json!({}),
        };
        let response = handle_http_request(&state, &request, None);
        let error = response.error.expect("unknown methods should return error");
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("Method not found"));
    }

    #[test]
    fn jsonrpc_workspace_resolution_error_is_reported_canonically() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let config = Config::default();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let router = WorkspaceRouter::new(
            WorkspaceConfig {
                auto_workspace: false,
                allowed_roots: AllowedRoots::default(),
                max_auto_workspaces: 10,
            },
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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(7)),
            method: "tools/call".into(),
            params: json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "foo",
                    "workspace": "/definitely/not/registered"
                }
            }),
        };
        let response = handle_http_request(&state, &request, None);
        assert!(
            response.error.is_none(),
            "workspace routing failures are reported as tool-level payload errors"
        );
        let payload = extract_payload(&response);
        let error_code = payload
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            matches!(
                error_code,
                "workspace_not_registered" | "workspace_not_allowed"
            ),
            "expected canonical workspace routing error code, got: {error_code}"
        );
    }

    #[test]
    fn health_cache_hit_prefers_cached_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_test_state(tmp.path(), Config::default());

        let cached = json!({
            "status": "cached",
            "marker": "cache-hit",
        });
        {
            let mut cache = state.health_cache.lock().unwrap();
            *cache = Some((Instant::now(), cached.clone()));
        }

        let response = build_health_response(&state);
        assert_eq!(response, cached);
    }

    #[test]
    fn health_cache_expiry_recomputes_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_test_state(tmp.path(), Config::default());

        {
            let mut cache = state.health_cache.lock().unwrap();
            *cache = Some((
                Instant::now() - HEALTH_CACHE_TTL - Duration::from_millis(5),
                json!({
                    "status": "cached",
                    "marker": "expired-value",
                }),
            ));
        }

        let response = build_health_response(&state);
        assert_ne!(
            response.get("marker"),
            Some(&json!("expired-value")),
            "expired cache entries must not be returned"
        );
    }

    #[test]
    fn health_surfaces_share_core_fields_with_intentional_differences() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("cc-data").to_string_lossy().to_string();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();

        let now = "2026-02-24T00:00:00Z".to_string();
        let project = Project {
            project_id: project_id.clone(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("health-parity".to_string()),
            default_ref: constants::REF_LIVE.to_string(),
            vcs_mode: false,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

        let router = WorkspaceRouter::new(
            WorkspaceConfig::default(),
            workspace.to_path_buf(),
            db_path.clone(),
        )
        .unwrap();
        let state = HttpState {
            config: config.clone(),
            workspace: workspace.to_path_buf(),
            project_id: project_id.clone(),
            data_dir: data_dir.clone(),
            db_path: db_path.clone(),
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let http_health = build_health_response(&state);

        let runtime = crate::server::load_index_runtime_public(&data_dir);
        let health_tool_response =
            crate::server::handle_tool_call_public(crate::server::PublicToolCallParams {
                id: Some(json!(1)),
                tool_name: "health_check",
                arguments: &json!({}),
                config: &config,
                index_set: runtime.index_set.as_ref(),
                schema_status: runtime.schema_status,
                compatibility_reason: runtime.compatibility_reason.as_deref(),
                conn: Some(&conn),
                workspace: &workspace,
                project_id: &project_id,
                prewarm_status: &state.prewarm_status,
                server_start: &state.server_start,
                notifier: Arc::new(NullProgressNotifier),
                progress_token: None,
            });
        let health_tool_payload = extract_payload(&health_tool_response);

        assert_eq!(
            http_health.get("status"),
            health_tool_payload.get("status"),
            "shared health fields should remain semantically aligned"
        );
        assert!(http_health.get("projects").is_some());
        assert!(health_tool_payload.get("projects").is_some());

        // Intentional contract differences:
        assert!(http_health.get("metadata").is_none());
        assert!(health_tool_payload.get("metadata").is_some());
        assert!(http_health.get("grammars").is_none());
        assert!(health_tool_payload.get("grammars").is_some());
    }

    #[test]
    fn t230_locate_symbol_http_matches_stdio_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("cc-data").to_string_lossy().to_string();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        let db_path = data_dir.join(constants::STATE_DB_FILE);

        build_fixture_index_at(&data_dir);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        let now = "2026-02-24T00:00:00Z".to_string();
        let project = Project {
            project_id: project_id.clone(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("http-locate".to_string()),
            default_ref: constants::REF_LIVE.to_string(),
            vcs_mode: false,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

        let router = WorkspaceRouter::new(
            WorkspaceConfig::default(),
            workspace.to_path_buf(),
            db_path.clone(),
        )
        .unwrap();
        let state = HttpState {
            config: config.clone(),
            workspace: workspace.to_path_buf(),
            project_id: project_id.clone(),
            data_dir: data_dir.clone(),
            db_path: db_path.clone(),
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let http_request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "locate_symbol",
                "arguments": { "name": "validate_token" }
            }),
        };
        let http_response = handle_http_request(&state, &http_request, None);
        assert!(
            http_response.error.is_none(),
            "http locate_symbol should succeed"
        );

        let stdio_response = dispatch_stdio_equivalent(&state, &http_request);
        assert!(
            stdio_response.error.is_none(),
            "stdio locate_symbol should succeed"
        );

        let http_payload = extract_payload(&http_response);
        let stdio_payload = extract_payload(&stdio_response);
        assert!(http_payload.get("results").is_some());
        assert!(http_payload.get("metadata").is_some());
        assert_eq!(
            http_payload
                .get("metadata")
                .and_then(|m| m.get("codecompass_protocol_version")),
            stdio_payload
                .get("metadata")
                .and_then(|m| m.get("codecompass_protocol_version"))
        );
        assert!(
            http_payload["results"].as_array().unwrap().len()
                == stdio_payload["results"].as_array().unwrap().len(),
            "HTTP and stdio locate_symbol should produce same result count for same inputs"
        );
    }

    #[test]
    fn t233_transport_parity_for_validation_error() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let config = Config::default();
        let state = build_test_state(&workspace, config);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(233)),
            method: "tools/call".into(),
            params: json!({
                "name": "locate_symbol",
                "arguments": {}
            }),
        };

        let http_response = handle_http_request(&state, &request, None);
        let stdio_response = dispatch_stdio_equivalent(&state, &request);

        assert_eq!(
            extract_payload(&http_response),
            extract_payload(&stdio_response),
            "validation failure semantics must remain equivalent across transports"
        );
    }

    #[test]
    fn t234_transport_parity_for_compatibility_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let config = Config::default();
        let state = build_test_state(&workspace, config);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(234)),
            method: "tools/call".into(),
            params: json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token"
                }
            }),
        };

        let http_response = handle_http_request(&state, &request, None);
        let stdio_response = dispatch_stdio_equivalent(&state, &request);

        assert_eq!(
            extract_payload(&http_response),
            extract_payload(&stdio_response),
            "compatibility failure semantics must remain equivalent across transports"
        );
    }

    #[test]
    fn t231_health_reports_indexing_when_active_job_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("cc-data").to_string_lossy().to_string();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        std::fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();
        let _ = codecompass_state::tantivy_index::IndexSet::open(&data_dir).unwrap();

        let now = "2026-02-24T00:00:00Z".to_string();
        let project = Project {
            project_id: project_id.clone(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("http-indexing".to_string()),
            default_ref: constants::REF_LIVE.to_string(),
            vcs_mode: false,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();
        let active_job = codecompass_state::jobs::IndexJob {
            job_id: "job-http-active".to_string(),
            project_id: project_id.clone(),
            r#ref: constants::REF_LIVE.to_string(),
            mode: "incremental".to_string(),
            head_commit: None,
            sync_id: None,
            status: "running".to_string(),
            changed_files: 0,
            duration_ms: None,
            error_message: None,
            retry_count: 0,
            progress_token: Some("index-job-job-http-active".to_string()),
            files_scanned: 50,
            files_indexed: 20,
            symbols_extracted: 100,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::jobs::create_job(&conn, &active_job).unwrap();

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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let health = build_health_response(&state);
        assert_eq!(
            health.get("status").and_then(Value::as_str),
            Some("indexing"),
            "health status should surface active indexing jobs"
        );
    }

    #[tokio::test]
    async fn t232_http_server_reports_port_conflict() {
        use tokio::time::timeout;

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let result = timeout(
            Duration::from_secs(5),
            run_http_server(
                &workspace,
                None,
                true,
                WorkspaceConfig::default(),
                "127.0.0.1",
                port,
            ),
        )
        .await;
        assert!(
            result.is_ok(),
            "run_http_server should fail quickly on bound ports"
        );
        let err = result.unwrap().expect_err("expected bind conflict error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("address already in use")
                || msg.contains("addrinuse")
                || msg.contains("os error"),
            "error should clearly indicate bind/port conflict, got: {msg}"
        );
        drop(listener);
    }

    #[test]
    fn t457_health_endpoint_smoke_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("cc-data").to_string_lossy().to_string();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        std::fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();
        let _ = codecompass_state::tantivy_index::IndexSet::open(&data_dir).unwrap();

        let now = "2026-02-24T00:00:00Z".to_string();
        let project = Project {
            project_id: project_id.clone(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("http-perf".to_string()),
            default_ref: constants::REF_LIVE.to_string(),
            vcs_mode: false,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let mut samples = Vec::new();
        for _ in 0..20 {
            let started = Instant::now();
            let _ = build_health_response_uncached(&state);
            samples.push(started.elapsed());
        }
        samples.sort();
        let p95 = samples[18];
        assert!(
            p95.as_millis() < 5_000,
            "/health smoke budget should remain < 5000ms, got {}ms",
            p95.as_millis()
        );
    }

    #[test]
    #[ignore = "benchmark harness"]
    fn benchmark_t457_health_endpoint_p95_under_50ms() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("cc-data").to_string_lossy().to_string();
        let project_id = generate_project_id(&workspace.to_string_lossy());
        let data_dir = config.project_data_dir(&project_id);
        std::fs::create_dir_all(&data_dir).unwrap();
        let db_path = data_dir.join(constants::STATE_DB_FILE);
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();
        let _ = codecompass_state::tantivy_index::IndexSet::open(&data_dir).unwrap();

        let now = "2026-02-24T00:00:00Z".to_string();
        let project = Project {
            project_id: project_id.clone(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("http-perf-bench".to_string()),
            default_ref: constants::REF_LIVE.to_string(),
            vcs_mode: false,
            schema_version: constants::SCHEMA_VERSION,
            parser_version: constants::PARSER_VERSION,
            created_at: now.clone(),
            updated_at: now,
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

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
            connection_manager: Arc::new(crate::server::ConnectionManager::new()),
            prewarm_status: Arc::new(AtomicU8::new(crate::server::PREWARM_COMPLETE)),
            warmset_enabled: true,
            health_cache: Arc::new(Mutex::new(None)),
            server_start: Instant::now(),
            router,
        };

        let mut samples = Vec::new();
        for _ in 0..20 {
            let started = Instant::now();
            let _ = build_health_response_uncached(&state);
            samples.push(started.elapsed());
        }
        samples.sort();
        let p95 = samples[18];
        assert!(
            p95.as_millis() < 50,
            "/health benchmark p95 should be < 50ms, got {}ms",
            p95.as_millis()
        );
    }
}
