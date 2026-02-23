use crate::protocol::{JsonRpcRequest, JsonRpcResponse, ProtocolMetadata};
use crate::tools;
use codecompass_core::config::Config;
use codecompass_core::constants;
use codecompass_core::error::StateError;
use codecompass_core::types::{DetailLevel, SchemaStatus, generate_project_id};
use codecompass_query::detail;
use codecompass_query::freshness::{
    self, FreshnessResult, PolicyAction, apply_freshness_policy, check_freshness_with_scan_params,
    parse_freshness_policy, trigger_async_sync,
};
use codecompass_query::locate;
use codecompass_query::ranking;
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
    enable_ranking_reasons: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load_with_file(Some(workspace), config_file)?;
    if enable_ranking_reasons {
        config.debug.ranking_reasons = true;
    }
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
        let response = handle_request(
            &request,
            &config,
            index_runtime.index_set.as_ref(),
            index_runtime.schema_status,
            index_runtime.compatibility_reason.as_deref(),
            conn.as_ref(),
            workspace,
            &project_id,
            &prewarm_status,
            &server_start,
        );
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    request: &JsonRpcRequest,
    config: &Config,
    index_set: Option<&IndexSet>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<&str>,
    conn: Option<&rusqlite::Connection>,
    workspace: &Path,
    project_id: &str,
    prewarm_status: &AtomicU8,
    server_start: &Instant,
) -> JsonRpcResponse {
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

            handle_tool_call(
                request.id.clone(),
                tool_name,
                &arguments,
                config,
                index_set,
                schema_status,
                compatibility_reason,
                conn,
                workspace,
                project_id,
                prewarm_status,
                server_start,
            )
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

#[allow(clippy::too_many_arguments)]
fn handle_tool_call(
    id: Option<Value>,
    tool_name: &str,
    arguments: &Value,
    config: &Config,
    index_set: Option<&IndexSet>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<&str>,
    conn: Option<&rusqlite::Connection>,
    workspace: &Path,
    project_id: &str,
    prewarm_status: &AtomicU8,
    server_start: &Instant,
) -> JsonRpcResponse {
    match tool_name {
        "locate_symbol" => {
            let name = arguments.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let kind = arguments.get("kind").and_then(|v| v.as_str());
            let language = arguments.get("language").and_then(|v| v.as_str());
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let detail_level = parse_detail_level(arguments);
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let base_metadata = build_metadata(
                &effective_ref,
                schema_status,
                config,
                conn,
                workspace,
                project_id,
            );

            if name.trim().is_empty() {
                return tool_error_response(
                    id,
                    "invalid_input",
                    "Parameter `name` is required.",
                    None,
                    base_metadata,
                );
            }

            let Some(index_set) = index_set else {
                return tool_compatibility_error(
                    id,
                    schema_status,
                    compatibility_reason,
                    config,
                    conn,
                    workspace,
                    project_id,
                    &effective_ref,
                );
            };

            if schema_status != SchemaStatus::Compatible {
                return tool_compatibility_error(
                    id,
                    schema_status,
                    compatibility_reason,
                    config,
                    conn,
                    workspace,
                    project_id,
                    &effective_ref,
                );
            }

            // Freshness check
            let policy = resolve_freshness_policy(arguments, config);
            let freshness_result = check_freshness_with_scan_params(
                conn,
                workspace,
                project_id,
                &effective_ref,
                config.index.max_file_size,
                Some(&config.index.languages),
            );
            let policy_action = apply_freshness_policy(policy, &freshness_result);
            let mut metadata =
                build_metadata_with_freshness(&effective_ref, schema_status, &freshness_result);

            if let PolicyAction::BlockWithError {
                last_indexed_commit,
                current_head,
            } = &policy_action
            {
                return tool_error_response(
                    id,
                    "index_stale",
                    "Index is stale and freshness_policy is strict. Sync before querying.",
                    Some(json!({
                        "last_indexed_commit": last_indexed_commit,
                        "current_head": current_head,
                        "suggestion": "Call sync_repo to update the index before querying.",
                    })),
                    metadata,
                );
            }
            if policy_action == PolicyAction::ProceedWithStaleIndicatorAndSync {
                trigger_async_sync(workspace, &effective_ref);
            }

            match locate::locate_symbol(
                &index_set.symbols,
                name,
                kind,
                language,
                Some(&effective_ref),
                limit,
            ) {
                Ok(results) => {
                    let mut result_values: Vec<Value> = results
                        .iter()
                        .filter_map(|r| serde_json::to_value(r).ok())
                        .collect();

                    // Enrich with context data if needed
                    if detail_level == DetailLevel::Context {
                        detail::enrich_body_previews(&mut result_values);
                        if let Some(c) = conn {
                            detail::enrich_results_with_relations(
                                &mut result_values,
                                c,
                                project_id,
                                &effective_ref,
                            );
                        }
                    }

                    // Apply detail level filtering
                    let filtered = detail::serialize_results_at_level(&result_values, detail_level);

                    // Include ranking reasons in metadata when debug mode is enabled.
                    if config.debug.ranking_reasons {
                        metadata.ranking_reasons =
                            Some(ranking::locate_ranking_reasons(&results, name));
                    }

                    let response = json!({
                        "results": filtered,
                        "total_candidates": results.len(),
                        "metadata": metadata,
                    });

                    tool_text_response(id, response)
                }
                Err(e) => {
                    let (code, message, data) = map_state_error(&e);
                    tool_error_response(id, code, message, data, metadata)
                }
            }
        }
        "search_code" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let language = arguments.get("language").and_then(|v| v.as_str());
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let detail_level = parse_detail_level(arguments);
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let base_metadata = build_metadata(
                &effective_ref,
                schema_status,
                config,
                conn,
                workspace,
                project_id,
            );

            if query.trim().is_empty() {
                return tool_error_response(
                    id,
                    "invalid_input",
                    "Parameter `query` is required.",
                    None,
                    base_metadata,
                );
            }

            let Some(index_set) = index_set else {
                return tool_compatibility_error(
                    id,
                    schema_status,
                    compatibility_reason,
                    config,
                    conn,
                    workspace,
                    project_id,
                    &effective_ref,
                );
            };

            if schema_status != SchemaStatus::Compatible {
                return tool_compatibility_error(
                    id,
                    schema_status,
                    compatibility_reason,
                    config,
                    conn,
                    workspace,
                    project_id,
                    &effective_ref,
                );
            }

            // Freshness check
            let policy = resolve_freshness_policy(arguments, config);
            let freshness_result = check_freshness_with_scan_params(
                conn,
                workspace,
                project_id,
                &effective_ref,
                config.index.max_file_size,
                Some(&config.index.languages),
            );
            let policy_action = apply_freshness_policy(policy, &freshness_result);
            let mut metadata =
                build_metadata_with_freshness(&effective_ref, schema_status, &freshness_result);

            if let PolicyAction::BlockWithError {
                last_indexed_commit,
                current_head,
            } = &policy_action
            {
                return tool_error_response(
                    id,
                    "index_stale",
                    "Index is stale and freshness_policy is strict. Sync before querying.",
                    Some(json!({
                        "last_indexed_commit": last_indexed_commit,
                        "current_head": current_head,
                        "suggestion": "Call sync_repo to update the index before querying.",
                    })),
                    metadata,
                );
            }
            if policy_action == PolicyAction::ProceedWithStaleIndicatorAndSync {
                trigger_async_sync(workspace, &effective_ref);
            }

            let debug_ranking = config.debug.ranking_reasons;
            match search::search_code(
                index_set,
                conn,
                query,
                Some(&effective_ref),
                language,
                limit,
                debug_ranking,
            ) {
                Ok(response) => {
                    let mut result_values: Vec<Value> = response
                        .results
                        .iter()
                        .filter_map(|r| serde_json::to_value(r).ok())
                        .collect();

                    // Enrich with context data if needed
                    if detail_level == DetailLevel::Context {
                        detail::enrich_body_previews(&mut result_values);
                        if let Some(c) = conn {
                            detail::enrich_results_with_relations(
                                &mut result_values,
                                c,
                                project_id,
                                &effective_ref,
                            );
                        }
                    }

                    // Apply detail level filtering
                    let filtered = detail::serialize_results_at_level(&result_values, detail_level);

                    if let Some(reasons) = &response.ranking_reasons {
                        metadata.ranking_reasons = Some(reasons.clone());
                    }

                    let mut result = json!({
                        "results": filtered,
                        "query_intent": &response.query_intent,
                        "total_candidates": response.total_candidates,
                        "suggested_next_actions": &response.suggested_next_actions,
                        "metadata": metadata,
                    });
                    if let Some(debug_payload) = &response.debug
                        && let Ok(value) = serde_json::to_value(debug_payload)
                    {
                        result["debug"] = value;
                    }
                    tool_text_response(id, result)
                }
                Err(e) => {
                    let (code, message, data) = map_state_error(&e);
                    tool_error_response(id, code, message, data, metadata)
                }
            }
        }
        "get_file_outline" => {
            let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let depth = arguments
                .get("depth")
                .and_then(|v| v.as_str())
                .unwrap_or("all");
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let metadata = build_metadata(
                &effective_ref,
                schema_status,
                config,
                conn,
                workspace,
                project_id,
            );

            if path.trim().is_empty() {
                return tool_error_response(
                    id,
                    "invalid_input",
                    "Parameter `path` is required.",
                    None,
                    metadata,
                );
            }

            let Some(c) = conn else {
                return tool_compatibility_error(
                    id,
                    schema_status,
                    compatibility_reason,
                    config,
                    conn,
                    workspace,
                    project_id,
                    &effective_ref,
                );
            };

            let top_only = depth == "top";
            match codecompass_state::symbols::get_file_outline_query(
                c,
                project_id,
                &effective_ref,
                path,
                top_only,
            ) {
                Ok(flat_symbols) => {
                    if flat_symbols.is_empty() {
                        let file_exists = codecompass_state::manifest::get_content_hash(
                            c,
                            project_id,
                            &effective_ref,
                            path,
                        )
                        .ok()
                        .flatten()
                        .is_some();
                        if file_exists {
                            let language = arguments
                                .get("language")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let response = json!({
                                "file_path": path,
                                "language": language,
                                "symbols": [],
                                "metadata": {
                                    "codecompass_protocol_version": metadata.codecompass_protocol_version,
                                    "freshness_status": metadata.freshness_status,
                                    "indexing_status": metadata.indexing_status,
                                    "result_completeness": metadata.result_completeness,
                                    "ref": effective_ref,
                                    "schema_status": metadata.schema_status,
                                    "symbol_count": 0,
                                },
                            });
                            return tool_text_response(id, response);
                        }
                        return tool_error_response(
                            id,
                            "file_not_found",
                            format!(
                                "No symbols found for path '{}' on ref '{}'.",
                                path, effective_ref
                            ),
                            Some(json!({
                                "path": path,
                                "ref": effective_ref,
                                "remediation": "Verify the file path and ensure the project is indexed.",
                            })),
                            metadata,
                        );
                    }

                    let symbol_count = flat_symbols.len();
                    let language = flat_symbols
                        .first()
                        .map(|s| s.language.clone())
                        .unwrap_or_default();

                    let symbols = if top_only {
                        flat_symbols
                    } else {
                        codecompass_state::symbols::build_symbol_tree(flat_symbols)
                    };

                    let response = json!({
                        "file_path": path,
                        "language": language,
                        "symbols": symbols,
                        "metadata": {
                            "codecompass_protocol_version": metadata.codecompass_protocol_version,
                            "freshness_status": metadata.freshness_status,
                            "indexing_status": metadata.indexing_status,
                            "result_completeness": metadata.result_completeness,
                            "ref": effective_ref,
                            "schema_status": metadata.schema_status,
                            "symbol_count": symbol_count,
                        },
                    });
                    tool_text_response(id, response)
                }
                Err(e) => {
                    let (code, message, data) = map_state_error(&e);
                    tool_error_response(id, code, message, data, metadata)
                }
            }
        }
        "health_check" => {
            let requested_workspace = arguments.get("workspace").and_then(|v| v.as_str());
            let effective_ref = resolve_tool_ref(None, workspace, conn, project_id);
            let metadata = build_metadata(
                &effective_ref,
                schema_status,
                config,
                conn,
                workspace,
                project_id,
            );

            // Resolve target projects.
            let projects = if let Some(c) = conn {
                if let Some(rw) = requested_workspace {
                    match codecompass_state::project::get_by_root(c, rw)
                        .ok()
                        .flatten()
                    {
                        Some(p) => vec![p],
                        None => {
                            return tool_error_response(
                                id,
                                "workspace_not_registered",
                                format!("The specified workspace '{}' is not registered.", rw),
                                Some(json!({
                                    "requested_workspace": rw,
                                })),
                                metadata,
                            );
                        }
                    }
                } else {
                    codecompass_state::project::list_projects(c).unwrap_or_default()
                }
            } else {
                Vec::new()
            };

            // Current prewarm status
            let pw_status = prewarm_status.load(Ordering::Acquire);
            let pw_label = prewarm_status_label(pw_status);

            // Tantivy health
            let tantivy_checks = if let Some(idx) = index_set {
                codecompass_state::tantivy_index::check_tantivy_health(idx)
            } else {
                Vec::new()
            };
            let tantivy_ok = !tantivy_checks.is_empty() && tantivy_checks.iter().all(|c| c.ok);

            // SQLite health
            let (sqlite_ok, sqlite_error) = conn
                .and_then(|c| codecompass_state::db::check_sqlite_health(c).ok())
                .unwrap_or((false, Some("No database connection".into())));

            // Grammar availability (T109)
            let supported = codecompass_indexer::parser::supported_languages();
            let mut grammars_available = Vec::new();
            let mut grammars_missing = Vec::new();
            for lang in &supported {
                match codecompass_indexer::parser::get_language(lang) {
                    Ok(_) => grammars_available.push(*lang),
                    Err(_) => grammars_missing.push(*lang),
                }
            }

            // Active job
            let mut overall_has_active_job = false;
            let mut active_job_payload: Option<Value> = None;
            let mut project_payloads = Vec::new();
            let mut any_error_project = false;
            let mut any_warming_project = false;

            if let Some(c) = conn {
                let iter_projects: Vec<_> = if projects.is_empty() {
                    codecompass_state::project::get_by_id(c, project_id)
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect()
                } else {
                    projects
                };

                for p in iter_projects {
                    let project_workspace = Path::new(&p.repo_root);
                    let project_ref = if p.default_ref.trim().is_empty() {
                        constants::REF_LIVE.to_string()
                    } else {
                        p.default_ref.clone()
                    };
                    let project_schema_status = resolve_project_schema_status(
                        config,
                        project_id,
                        &p.project_id,
                        schema_status,
                    );
                    let freshness = check_freshness_with_scan_params(
                        Some(c),
                        project_workspace,
                        &p.project_id,
                        &project_ref,
                        config.index.max_file_size,
                        Some(&config.index.languages),
                    );
                    let freshness_status = freshness::freshness_status(&freshness);

                    let active_job = codecompass_state::jobs::get_active_job(c, &p.project_id)
                        .ok()
                        .flatten();
                    if let Some(j) = &active_job {
                        overall_has_active_job = true;
                        if active_job_payload.is_none() {
                            active_job_payload = Some(json!({
                                "job_id": j.job_id,
                                "project_id": j.project_id,
                                "mode": j.mode,
                                "status": j.status,
                                "ref": j.r#ref,
                                "changed_files": j.changed_files,
                                "started_at": j.created_at,
                            }));
                        }
                    }

                    let (index_status, warming) = if active_job.is_some() {
                        ("indexing", false)
                    } else if !matches!(project_schema_status, SchemaStatus::Compatible) {
                        ("error", false)
                    } else if p.project_id == project_id && pw_status == PREWARM_IN_PROGRESS {
                        ("warming", true)
                    } else if p.project_id == project_id && pw_status == PREWARM_FAILED {
                        ("error", false)
                    } else {
                        ("ready", false)
                    };
                    any_warming_project |= warming;
                    any_error_project |= index_status == "error";

                    let file_count =
                        codecompass_state::manifest::file_count(c, &p.project_id, &project_ref)
                            .unwrap_or(0);
                    let symbol_count =
                        codecompass_state::symbols::symbol_count(c, &p.project_id, &project_ref)
                            .unwrap_or(0);
                    let last_indexed_at =
                        codecompass_state::jobs::get_recent_jobs(c, &p.project_id, 5)
                            .ok()
                            .and_then(|jobs| {
                                jobs.into_iter()
                                    .find(|j| j.status == "published" && j.r#ref == project_ref)
                                    .map(|j| j.updated_at)
                            });

                    project_payloads.push(json!({
                        "project_id": p.project_id,
                        "repo_root": p.repo_root,
                        "index_status": index_status,
                        "freshness_status": freshness_status,
                        "last_indexed_at": last_indexed_at,
                        "ref": project_ref,
                        "file_count": file_count,
                        "symbol_count": symbol_count,
                    }));
                }

                if project_payloads.is_empty() {
                    let fallback_status = if matches!(
                        schema_status,
                        SchemaStatus::ReindexRequired
                            | SchemaStatus::CorruptManifest
                            | SchemaStatus::NotIndexed
                    ) {
                        any_error_project = true;
                        "error"
                    } else if pw_status == PREWARM_IN_PROGRESS {
                        any_warming_project = true;
                        "warming"
                    } else if pw_status == PREWARM_FAILED {
                        any_error_project = true;
                        "error"
                    } else {
                        "ready"
                    };
                    project_payloads.push(json!({
                        "project_id": project_id,
                        "repo_root": workspace.to_string_lossy(),
                        "index_status": fallback_status,
                        "freshness_status": metadata.freshness_status,
                        "last_indexed_at": Value::Null,
                        "ref": effective_ref,
                        "file_count": codecompass_state::manifest::file_count(c, project_id, &effective_ref).unwrap_or(0),
                        "symbol_count": codecompass_state::symbols::symbol_count(c, project_id, &effective_ref).unwrap_or(0),
                    }));
                }
            } else {
                project_payloads.push(json!({
                    "project_id": project_id,
                    "repo_root": workspace.to_string_lossy(),
                    "index_status": "error",
                    "freshness_status": metadata.freshness_status,
                    "last_indexed_at": Value::Null,
                    "ref": effective_ref,
                    "file_count": 0,
                    "symbol_count": 0,
                }));
                any_error_project = true;
            }

            // Uptime
            let uptime_seconds = server_start.elapsed().as_secs();

            // Startup compatibility payload
            let stored_schema_version = conn.and_then(|c| {
                codecompass_state::project::get_by_id(c, project_id)
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
            let overall_status = if any_error_project {
                "error"
            } else if overall_has_active_job {
                "indexing"
            } else if any_warming_project {
                "warming"
            } else {
                "ready"
            };

            let result = json!({
                "status": overall_status,
                "version": env!("CARGO_PKG_VERSION"),
                "uptime_seconds": uptime_seconds,
                "tantivy_ok": tantivy_ok,
                "sqlite_ok": sqlite_ok,
                "sqlite_error": sqlite_error,
                "prewarm_status": pw_label,
                "grammars": {
                    "available": grammars_available,
                    "missing": grammars_missing,
                },
                "active_job": active_job_payload,
                "startup_checks": {
                    "index": {
                        "status": index_compat_status,
                        "current_schema_version": current_schema_version,
                        "required_schema_version": constants::SCHEMA_VERSION,
                        "message": compat_message,
                    }
                },
                "projects": project_payloads,
                "metadata": metadata,
            });
            tool_text_response(id, result)
        }
        "index_status" => {
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let stored_schema_version = conn.and_then(|c| {
                codecompass_state::project::get_by_id(c, project_id)
                    .ok()
                    .flatten()
                    .map(|p| p.schema_version)
            });
            let (status, schema_status_str, current_schema_version) = match schema_status {
                SchemaStatus::Compatible => ("ready", "compatible", constants::SCHEMA_VERSION),
                SchemaStatus::NotIndexed => (
                    "not_indexed",
                    "not_indexed",
                    stored_schema_version.unwrap_or(0),
                ),
                SchemaStatus::ReindexRequired => (
                    "not_indexed",
                    "reindex_required",
                    stored_schema_version.unwrap_or(0),
                ),
                SchemaStatus::CorruptManifest => (
                    "not_indexed",
                    "corrupt_manifest",
                    stored_schema_version.unwrap_or(0),
                ),
            };

            // Gather counts from SQLite if available
            let (file_count, symbol_count) = conn
                .map(|c| {
                    let fc = codecompass_state::manifest::file_count(c, project_id, &effective_ref)
                        .unwrap_or(0);
                    let sc =
                        codecompass_state::symbols::symbol_count(c, project_id, &effective_ref)
                            .unwrap_or(0);
                    (fc, sc)
                })
                .unwrap_or((0, 0));

            // Get recent jobs
            let recent_jobs = conn
                .and_then(|c| codecompass_state::jobs::get_recent_jobs(c, project_id, 5).ok())
                .unwrap_or_default();

            let active_job = conn.and_then(|c| {
                codecompass_state::jobs::get_active_job(c, project_id)
                    .ok()
                    .flatten()
            });

            // Derive last_indexed_at from the most recent published job for this ref
            let last_indexed_at: Option<String> = recent_jobs
                .iter()
                .find(|j| j.status == "published" && j.r#ref == effective_ref)
                .map(|j| j.updated_at.clone());

            let result = json!({
                "project_id": project_id,
                "repo_root": workspace.to_string_lossy(),
                "index_status": status,
                "schema_status": schema_status_str,
                "current_schema_version": current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "last_indexed_at": last_indexed_at,
                "ref": effective_ref,
                "file_count": file_count,
                "symbol_count": symbol_count,
                "compatibility_reason": compatibility_reason,
                "active_job": active_job.map(|j| json!({
                    "job_id": j.job_id,
                    "mode": j.mode,
                    "status": j.status,
                    "ref": j.r#ref,
                })),
                "recent_jobs": recent_jobs.iter().map(|j| json!({
                    "job_id": j.job_id,
                    "ref": j.r#ref,
                    "mode": j.mode,
                    "status": j.status,
                    "changed_files": j.changed_files,
                    "duration_ms": j.duration_ms,
                    "created_at": j.created_at,
                })).collect::<Vec<_>>(),
                "metadata": build_metadata(
                    &effective_ref,
                    schema_status,
                    config,
                    conn,
                    workspace,
                    project_id
                ),
            });
            tool_text_response(id, result)
        }
        "index_repo" | "sync_repo" => {
            let force = arguments
                .get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mode = if force { "full" } else { "incremental" };
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let metadata = build_metadata(
                &effective_ref,
                schema_status,
                config,
                conn,
                workspace,
                project_id,
            );

            if !is_project_registered(conn, workspace) {
                return tool_error_response(
                    id,
                    "project_not_found",
                    "Project is not initialized for this workspace. Run `codecompass init` first.",
                    Some(json!({
                        "workspace": workspace.to_string_lossy(),
                        "remediation": "codecompass init --path <workspace>",
                    })),
                    metadata,
                );
            }
            if has_active_job(conn, project_id) {
                return tool_error_response(
                    id,
                    "index_in_progress",
                    "An indexing job is already running.",
                    Some(json!({
                        "project_id": project_id,
                        "remediation": "Use index_status to poll and retry after completion.",
                    })),
                    metadata,
                );
            }

            // Use current_exe() to find the binary reliably (works in MCP agent setups)
            let exe = std::env::current_exe().unwrap_or_else(|_| "codecompass".into());
            let workspace_str = workspace.to_string_lossy();
            let job_id = format!("{:016x}", rand_u64());

            let mut cmd = std::process::Command::new(exe);
            cmd.arg("index")
                .arg("--path")
                .arg(workspace_str.as_ref())
                .env("CODECOMPASS_JOB_ID", &job_id)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if force {
                cmd.arg("--force");
            }
            // Pass the resolved ref so the subprocess uses the same scope and
            // avoids divergent fallback behavior.
            cmd.arg("--ref").arg(&effective_ref);

            match cmd.spawn() {
                Ok(child) => {
                    // Reap the child in a background thread to avoid zombie processes
                    std::thread::spawn(move || {
                        let mut child = child;
                        let _ = child.wait();
                    });
                    let mut payload = serde_json::Map::new();
                    payload.insert("job_id".to_string(), json!(job_id));
                    payload.insert("status".to_string(), json!("running"));
                    payload.insert("mode".to_string(), json!(mode));
                    if tool_name == "sync_repo" {
                        payload.insert("changed_files".to_string(), Value::Null);
                    } else {
                        payload.insert("file_count".to_string(), Value::Null);
                    }
                    payload.insert("metadata".to_string(), json!(metadata));
                    tool_text_response(id, Value::Object(payload))
                }
                Err(e) => tool_error_response(
                    id,
                    "internal_error",
                    "Failed to spawn indexer process.",
                    Some(json!({
                        "details": e.to_string(),
                        "remediation": "Run `codecompass index` manually to inspect logs.",
                    })),
                    metadata,
                ),
            }
        }
        _ => JsonRpcResponse::error(id, -32601, format!("Unknown tool: {}", tool_name)),
    }
}

fn map_state_error(err: &StateError) -> (&'static str, String, Option<Value>) {
    match err {
        StateError::SchemaMigrationRequired { current, required } => (
            "index_incompatible",
            "Index schema is incompatible. Run `codecompass index --force`.".to_string(),
            Some(json!({
                "current_schema_version": current,
                "required_schema_version": required,
                "remediation": "codecompass index --force",
            })),
        ),
        StateError::CorruptManifest(details) => (
            "index_incompatible",
            "Index metadata is corrupted. Run `codecompass index --force`.".to_string(),
            Some(json!({
                "details": details,
                "remediation": "codecompass index --force",
            })),
        ),
        other => (
            "internal_error",
            format!("Tool execution failed: {}", other),
            None,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn tool_compatibility_error(
    id: Option<Value>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<&str>,
    config: &Config,
    conn: Option<&rusqlite::Connection>,
    workspace: &Path,
    project_id: &str,
    r#ref: &str,
) -> JsonRpcResponse {
    let metadata = build_metadata(r#ref, schema_status, config, conn, workspace, project_id);
    if schema_status == SchemaStatus::NotIndexed && !is_project_registered(conn, workspace) {
        return tool_error_response(
            id,
            "project_not_found",
            "Project is not initialized for this workspace. Run `codecompass init` first.",
            Some(json!({
                "workspace": workspace.to_string_lossy(),
                "remediation": "codecompass init --path <workspace>",
            })),
            metadata,
        );
    }

    let remediation = match schema_status {
        SchemaStatus::NotIndexed => "codecompass index",
        SchemaStatus::ReindexRequired | SchemaStatus::CorruptManifest => {
            "codecompass index --force"
        }
        SchemaStatus::Compatible => "codecompass index",
    };
    let message = match schema_status {
        SchemaStatus::NotIndexed => "No index available. Run `codecompass index`.",
        SchemaStatus::ReindexRequired | SchemaStatus::CorruptManifest => {
            "Index is incompatible. Run `codecompass index --force`."
        }
        SchemaStatus::Compatible => "Index is unavailable.",
    };
    tool_error_response(
        id,
        "index_incompatible",
        message,
        Some(json!({
            "schema_status": schema_status,
            "reason": compatibility_reason,
            "remediation": remediation,
        })),
        metadata,
    )
}

fn tool_error_response(
    id: Option<Value>,
    code: &str,
    message: impl Into<String>,
    data: Option<Value>,
    metadata: ProtocolMetadata,
) -> JsonRpcResponse {
    let mut error_obj = serde_json::Map::new();
    error_obj.insert("code".to_string(), Value::String(code.to_string()));
    error_obj.insert("message".to_string(), Value::String(message.into()));
    if let Some(d) = data {
        error_obj.insert("data".to_string(), d);
    }

    let mut payload = serde_json::Map::new();
    payload.insert("error".to_string(), Value::Object(error_obj));
    payload.insert("metadata".to_string(), json!(metadata));

    tool_text_response(id, Value::Object(payload))
}

/// Helper: wrap a JSON value as MCP tool text content response.
fn tool_text_response(id: Option<Value>, payload: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "content": [{"type": "text", "text": serde_json::to_string(&payload).unwrap_or_default()}]
        }),
    )
}

/// Parse `detail_level` from MCP tool arguments, defaulting to `Signature`.
fn parse_detail_level(arguments: &Value) -> DetailLevel {
    arguments
        .get("detail_level")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "location" => DetailLevel::Location,
            "context" => DetailLevel::Context,
            _ => DetailLevel::Signature,
        })
        .unwrap_or(DetailLevel::Signature)
}

fn rand_u64() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codecompass_core::config::Config;
    use codecompass_core::types::Project;
    use serde_json::json;
    use std::path::Path;

    /// Default prewarm status for tests (complete).
    fn test_prewarm_status() -> AtomicU8 {
        AtomicU8::new(PREWARM_COMPLETE)
    }

    /// Default server start time for tests.
    fn test_server_start() -> Instant {
        Instant::now()
    }

    fn make_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn resolve_tool_ref_falls_back_to_project_default_when_head_unavailable() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();

        let db_path = tmp.path().join("state.db");
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        codecompass_state::schema::create_tables(&conn).unwrap();

        let project_id = "proj_test";
        let project = Project {
            project_id: project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("test".to_string()),
            default_ref: "main".to_string(),
            vcs_mode: true,
            schema_version: 1,
            parser_version: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

        // Temp dir is non-git and has no HEAD branch; should fall back to project default_ref.
        let resolved = resolve_tool_ref(None, workspace, Some(&conn), project_id);
        assert_eq!(resolved, "main");

        // Explicit argument still has top priority.
        let explicit = resolve_tool_ref(Some("feat/auth"), workspace, Some(&conn), project_id);
        assert_eq!(explicit, "feat/auth");
    }

    // ------------------------------------------------------------------
    // T065: tools/list returns all five tools
    // ------------------------------------------------------------------

    #[test]
    fn t065_tools_list_returns_all_five_tools() {
        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "fake_project_id";

        let request = make_request("tools/list", json!({}));
        let response = handle_request(
            &request,
            &config,
            None,
            SchemaStatus::NotIndexed,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success, got error");
        let result = response.result.expect("result should be present");

        let tools = result
            .get("tools")
            .expect("result should contain 'tools'")
            .as_array()
            .expect("'tools' should be an array");

        assert_eq!(tools.len(), 7, "expected 7 tools, got {}", tools.len());

        let tool_names: Vec<&str> = tools
            .iter()
            .map(|t| t.get("name").unwrap().as_str().unwrap())
            .collect();

        let expected_names = [
            "index_repo",
            "sync_repo",
            "search_code",
            "locate_symbol",
            "get_file_outline",
            "health_check",
            "index_status",
        ];
        for name in &expected_names {
            assert!(
                tool_names.contains(name),
                "missing tool: {name}; found: {tool_names:?}"
            );
        }

        for tool in tools {
            assert!(tool.get("name").is_some(), "tool missing 'name': {tool:?}");
            assert!(
                tool.get("description").is_some(),
                "tool missing 'description': {tool:?}"
            );
            assert!(
                tool.get("inputSchema").is_some(),
                "tool missing 'inputSchema': {tool:?}"
            );

            let desc = tool.get("description").unwrap().as_str().unwrap();
            assert!(!desc.is_empty(), "tool description is empty: {tool:?}");

            assert!(
                tool.get("inputSchema").unwrap().is_object(),
                "inputSchema should be an object: {tool:?}"
            );
        }
    }

    // ------------------------------------------------------------------
    // T066: locate_symbol via JSON-RPC with an indexed fixture
    // ------------------------------------------------------------------

    fn build_fixture_index(tmp_dir: &std::path::Path) -> IndexSet {
        use codecompass_indexer::{
            languages, parser, scanner, snippet_extract, symbol_extract, writer,
        };
        use codecompass_state::{db, schema, tantivy_index::IndexSet};

        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/rust-sample");
        assert!(
            fixture_dir.exists(),
            "fixture directory missing: {}",
            fixture_dir.display()
        );

        let data_dir = tmp_dir.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let index_set = IndexSet::open(&data_dir).unwrap();

        let db_path = data_dir.join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let scanned = scanner::scan_directory(&fixture_dir, 1_048_576);
        assert!(
            !scanned.is_empty(),
            "scanner found no files in fixture directory"
        );

        let repo = "test-repo";
        let r#ref = "live";

        for file in &scanned {
            let source = std::fs::read_to_string(&file.path).unwrap();
            let tree = match parser::parse_file(&source, &file.language) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let extracted = languages::extract_symbols(&tree, &source, &file.language);
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
        }

        index_set
    }

    #[test]
    fn t066_locate_symbol_via_jsonrpc() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test_project";

        let request = make_request(
            "tools/call",
            json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "validate_token"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(
            response.error.is_none(),
            "expected success, got error: {:?}",
            response.error
        );
        let result = response.result.expect("result should be present");

        let content = result
            .get("content")
            .expect("result should have 'content'")
            .as_array()
            .expect("'content' should be an array");

        assert!(!content.is_empty(), "content array should not be empty");

        let first = &content[0];
        assert_eq!(
            first.get("type").unwrap().as_str().unwrap(),
            "text",
            "content type should be 'text'"
        );

        let text = first.get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(text).expect("text payload should be valid JSON");

        let results = payload
            .get("results")
            .expect("payload should have 'results'")
            .as_array()
            .expect("'results' should be an array");

        assert!(
            !results.is_empty(),
            "results should contain at least one match for 'validate_token'"
        );

        let vt = results
            .iter()
            .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
            .expect("results should contain a 'validate_token' entry");

        assert_eq!(vt.get("kind").unwrap().as_str().unwrap(), "function");
        assert_eq!(vt.get("language").unwrap().as_str().unwrap(), "rust");
        assert!(
            vt.get("path")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("auth.rs"),
            "path should reference auth.rs"
        );
        assert!(vt.get("line_start").unwrap().as_u64().unwrap() > 0);
        assert!(vt.get("line_end").unwrap().as_u64().unwrap() > 0);
        assert!(
            !vt.get("symbol_id").unwrap().as_str().unwrap().is_empty(),
            "symbol_id should not be empty"
        );
        assert!(
            !vt.get("symbol_stable_id")
                .unwrap()
                .as_str()
                .unwrap()
                .is_empty(),
            "symbol_stable_id should not be empty"
        );

        // Verify Protocol v1 metadata
        let metadata = payload
            .get("metadata")
            .expect("payload should have 'metadata'");
        assert_eq!(
            metadata
                .get("codecompass_protocol_version")
                .unwrap()
                .as_str()
                .unwrap(),
            "1.0"
        );
        assert_eq!(
            metadata.get("ref").unwrap().as_str().unwrap(),
            "live",
            "ref should default to 'live'"
        );
    }

    // ------------------------------------------------------------------
    // Helper: extract the results array from an MCP tool response
    // ------------------------------------------------------------------

    fn extract_results_from_response(response: &JsonRpcResponse) -> Vec<serde_json::Value> {
        let result = response.result.as_ref().expect("result should be present");
        let content = result
            .get("content")
            .expect("result should have 'content'")
            .as_array()
            .expect("'content' should be an array");
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(text).expect("text payload should be valid JSON");
        payload
            .get("results")
            .expect("payload should have 'results'")
            .as_array()
            .expect("'results' should be an array")
            .clone()
    }

    // ------------------------------------------------------------------
    // T095: locate_symbol with detail_level: "location"
    // ------------------------------------------------------------------

    #[test]
    fn t095_locate_symbol_detail_level_location() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test_project";

        let request = make_request(
            "tools/call",
            json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "validate_token",
                    "detail_level": "location"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "should have results");

        let vt = results
            .iter()
            .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
            .expect("should find validate_token");

        // Location-level fields must be present
        for field in &["path", "line_start", "line_end", "kind", "name"] {
            assert!(
                vt.get(*field).is_some(),
                "location level should include field '{}'",
                field
            );
        }

        // Identity fields should always be present
        assert!(vt.get("symbol_id").is_some(), "symbol_id should be present");
        assert!(
            vt.get("symbol_stable_id").is_some(),
            "symbol_stable_id should be present"
        );
        assert!(vt.get("score").is_some(), "score should be present");

        // Signature-only fields must NOT be present at location level
        for field in &["qualified_name", "language", "visibility"] {
            assert!(
                vt.get(*field).is_none(),
                "location level should NOT include field '{}', but it was present",
                field
            );
        }

        // Context-only fields must NOT be present
        assert!(
            vt.get("body_preview").is_none(),
            "location level should NOT include body_preview"
        );
        assert!(
            vt.get("parent").is_none(),
            "location level should NOT include parent"
        );
        assert!(
            vt.get("related_symbols").is_none(),
            "location level should NOT include related_symbols"
        );
    }

    // ------------------------------------------------------------------
    // T096: locate_symbol with detail_level: "signature" (default)
    // ------------------------------------------------------------------

    #[test]
    fn t096_locate_symbol_detail_level_signature_default() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test_project";

        // No explicit detail_level — defaults to "signature"
        let request = make_request(
            "tools/call",
            json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "validate_token"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "should have results");

        let vt = results
            .iter()
            .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
            .expect("should find validate_token");

        // All signature-level fields must be present
        for field in &[
            "path",
            "line_start",
            "line_end",
            "kind",
            "name",
            "qualified_name",
            "language",
        ] {
            assert!(
                vt.get(*field).is_some(),
                "signature level should include field '{}'",
                field
            );
        }

        // Identity fields should always be present
        assert!(vt.get("symbol_id").is_some(), "symbol_id should be present");
        assert!(
            vt.get("symbol_stable_id").is_some(),
            "symbol_stable_id should be present"
        );
        assert!(vt.get("score").is_some(), "score should be present");

        // Context-only fields must NOT be present at signature level
        assert!(
            vt.get("body_preview").is_none(),
            "signature level should NOT include body_preview"
        );
        assert!(
            vt.get("parent").is_none(),
            "signature level should NOT include parent"
        );
        assert!(
            vt.get("related_symbols").is_none(),
            "signature level should NOT include related_symbols"
        );
    }

    // ------------------------------------------------------------------
    // T097: search_code with detail_level: "context"
    // ------------------------------------------------------------------

    #[test]
    fn t097_search_code_detail_level_context() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test_project";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token",
                    "detail_level": "context"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "should have results");

        // At context level, all standard fields pass through
        let first = &results[0];
        assert!(
            first.get("path").is_some(),
            "context level should include path"
        );
        assert!(
            first.get("line_start").is_some(),
            "context level should include line_start"
        );

        // Context level should include enrichment fields when data is available.
        // body_preview comes from snippet/content fields which are populated in search results.
        let has_body_preview = results.iter().any(|r| r.get("body_preview").is_some());
        assert!(
            has_body_preview,
            "at least one context-level result should have body_preview"
        );
    }

    // ------------------------------------------------------------------
    // Helper: build fixture index and return both IndexSet and DB path
    // ------------------------------------------------------------------

    fn build_fixture_index_with_db(tmp_dir: &std::path::Path) -> (IndexSet, std::path::PathBuf) {
        let index_set = build_fixture_index(tmp_dir);
        let db_path = tmp_dir.join("data").join("state.db");
        (index_set, db_path)
    }

    // ------------------------------------------------------------------
    // T102: get_file_outline nested tree
    // ------------------------------------------------------------------

    #[test]
    fn t102_get_file_outline_nested_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        // types.rs has struct User + impl User with methods — good for nesting test
        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": "src/types.rs"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(
            response.error.is_none(),
            "expected success, got error: {:?}",
            response.error
        );
        let result = response.result.as_ref().expect("result should be present");
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(
            payload.get("file_path").unwrap().as_str().unwrap(),
            "src/types.rs"
        );
        assert_eq!(payload.get("language").unwrap().as_str().unwrap(), "rust");

        let symbols = payload
            .get("symbols")
            .expect("should have symbols")
            .as_array()
            .expect("symbols should be an array");

        assert!(!symbols.is_empty(), "should have symbols for types.rs");

        // Verify at least one symbol has children (impl block with methods)
        let has_children = symbols.iter().any(|s| {
            let children = s.get("children").and_then(|c| c.as_array());
            children.map(|c| !c.is_empty()).unwrap_or(false)
        });
        assert!(
            has_children,
            "types.rs should have impl blocks with children (methods)"
        );

        // Verify metadata
        let metadata = payload.get("metadata").expect("should have metadata");
        assert!(metadata.get("symbol_count").unwrap().as_u64().unwrap() > 0);
    }

    // ------------------------------------------------------------------
    // T103: get_file_outline with depth: "top"
    // ------------------------------------------------------------------

    #[test]
    fn t103_get_file_outline_top_level_only() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": "src/types.rs",
                    "depth": "top"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        let symbols = payload.get("symbols").unwrap().as_array().unwrap();

        assert!(!symbols.is_empty(), "should have top-level symbols");

        // With depth="top", no symbol should have non-empty children
        for sym in symbols {
            let children = sym.get("children").and_then(|c| c.as_array());
            let has_children = children.map(|c| !c.is_empty()).unwrap_or(false);
            assert!(
                !has_children,
                "top-level only mode should not include children, but '{}' has children",
                sym.get("name").unwrap().as_str().unwrap_or("?")
            );
        }
    }

    // ------------------------------------------------------------------
    // T104: get_file_outline on non-existent file
    // ------------------------------------------------------------------

    #[test]
    fn t104_get_file_outline_nonexistent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": "src/nonexistent_file.rs"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "MCP tool errors are in content");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // Should have an error object with file_not_found code
        let error = payload.get("error").expect("should have error object");
        assert_eq!(
            error.get("code").unwrap().as_str().unwrap(),
            "file_not_found"
        );
    }

    #[test]
    fn t104_get_file_outline_existing_file_without_symbols_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        codecompass_state::manifest::upsert_manifest(
            &conn,
            &codecompass_state::manifest::ManifestEntry {
                repo: "test-repo".to_string(),
                r#ref: "live".to_string(),
                path: "docs/README.md".to_string(),
                content_hash: blake3::hash(b"hello").to_hex().to_string(),
                size_bytes: 5,
                mtime_ns: None,
                language: Some("markdown".to_string()),
                indexed_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": "docs/README.md",
                    "language": "markdown"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        assert!(
            payload.get("error").is_none(),
            "existing file without symbols should not return file_not_found"
        );
        assert_eq!(
            payload.get("file_path").unwrap().as_str().unwrap(),
            "docs/README.md"
        );
        let symbols = payload.get("symbols").unwrap().as_array().unwrap();
        assert!(symbols.is_empty(), "symbols should be empty");
    }

    /// T111: health_check on a healthy system returns ready status
    #[test]
    fn t111_health_check_on_healthy_system() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success, got error");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // Status should be "ready"
        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "ready",
            "expected status 'ready'"
        );

        // Tantivy should be ok
        assert!(
            payload.get("tantivy_ok").unwrap().as_bool().unwrap(),
            "expected tantivy_ok: true"
        );

        // SQLite should be ok
        assert!(
            payload.get("sqlite_ok").unwrap().as_bool().unwrap(),
            "expected sqlite_ok: true"
        );

        // Grammars: all 4 should be available
        let grammars = payload.get("grammars").unwrap();
        let available = grammars.get("available").unwrap().as_array().unwrap();
        assert!(
            available.len() >= 4,
            "expected at least 4 grammars available, got {}",
            available.len()
        );
        let missing = grammars.get("missing").unwrap().as_array().unwrap();
        assert!(
            missing.is_empty(),
            "expected no missing grammars, got {:?}",
            missing
        );

        // Startup checks
        let startup = payload.get("startup_checks").unwrap();
        let index_check = startup.get("index").unwrap();
        assert_eq!(
            index_check.get("status").unwrap().as_str().unwrap(),
            "compatible"
        );

        // Protocol version in metadata
        let meta = payload.get("metadata").unwrap();
        assert!(meta.get("codecompass_protocol_version").is_some());

        // Prewarm status should be present
        assert!(payload.get("prewarm_status").is_some());
    }

    #[test]
    fn t111_health_check_active_job_sets_indexing_status() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let project = Project {
            project_id: project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("test".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: codecompass_core::constants::SCHEMA_VERSION,
            parser_version: codecompass_core::constants::PARSER_VERSION,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        codecompass_state::project::create_project(&conn, &project).unwrap();

        codecompass_state::jobs::create_job(
            &conn,
            &codecompass_state::jobs::IndexJob {
                job_id: "job_active".to_string(),
                project_id: project_id.to_string(),
                r#ref: "live".to_string(),
                mode: "incremental".to_string(),
                head_commit: Some("abc123".to_string()),
                sync_id: None,
                status: "running".to_string(),
                changed_files: 1,
                duration_ms: None,
                error_message: None,
                retry_count: 0,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );
        let response = handle_request(
            &request,
            &Config::default(),
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(payload.get("status").unwrap().as_str().unwrap(), "indexing");
        assert!(
            payload.get("active_job").is_some(),
            "active_job should be present when an indexing job is running"
        );
    }

    /// T116: health_check returns "warming" when prewarm is in progress
    #[test]
    fn t116_health_check_warming_status_during_prewarm() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        // Simulate prewarm in progress
        let pw = AtomicU8::new(PREWARM_IN_PROGRESS);

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &pw,
            &test_server_start(),
        );

        assert!(response.error.is_none());
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // Status should be "warming" while prewarm is in progress
        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "warming",
            "expected status 'warming' during prewarm"
        );
        assert_eq!(
            payload.get("prewarm_status").unwrap().as_str().unwrap(),
            "warming"
        );

        // Now simulate prewarm complete
        pw.store(PREWARM_COMPLETE, Ordering::Release);

        let response2 = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &pw,
            &test_server_start(),
        );

        let result2 = response2.result.as_ref().unwrap();
        let content2 = result2.get("content").unwrap().as_array().unwrap();
        let text2 = content2[0].get("text").unwrap().as_str().unwrap();
        let payload2: serde_json::Value = serde_json::from_str(text2).unwrap();

        // Status should now be "ready"
        assert_eq!(
            payload2.get("status").unwrap().as_str().unwrap(),
            "ready",
            "expected status 'ready' after prewarm completes"
        );
        assert_eq!(
            payload2.get("prewarm_status").unwrap().as_str().unwrap(),
            "complete"
        );
    }

    /// T117: health_check returns "ready" immediately with --no-prewarm (skipped)
    #[test]
    fn t117_health_check_no_prewarm_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        // Simulate --no-prewarm: status is SKIPPED
        let pw = AtomicU8::new(PREWARM_SKIPPED);

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &pw,
            &test_server_start(),
        );

        assert!(response.error.is_none());
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // Status should be "ready" immediately (not "warming")
        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "ready",
            "expected status 'ready' with --no-prewarm"
        );
        assert_eq!(
            payload.get("prewarm_status").unwrap().as_str().unwrap(),
            "skipped"
        );
    }

    #[test]
    fn t118_health_check_prewarm_failed_reports_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";
        let pw = AtomicU8::new(PREWARM_FAILED);

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &pw,
            &test_server_start(),
        );

        assert!(response.error.is_none());
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "error",
            "expected status 'error' when prewarm fails"
        );
        assert_eq!(
            payload.get("prewarm_status").unwrap().as_str().unwrap(),
            "failed"
        );
    }

    #[test]
    fn t119_health_check_not_indexed_registered_project_sets_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, db_path) = build_fixture_index_with_db(tmp.path());
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();
        let workspace = Path::new("/tmp/fake-workspace");
        let current_project_id = "test-repo";
        let missing_project_id = "missing-proj-never-indexed";

        let current = Project {
            project_id: current_project_id.to_string(),
            repo_root: workspace.to_string_lossy().to_string(),
            display_name: Some("current".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: codecompass_core::constants::SCHEMA_VERSION,
            parser_version: codecompass_core::constants::PARSER_VERSION,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        codecompass_state::project::create_project(&conn, &current).unwrap();

        let missing = Project {
            project_id: missing_project_id.to_string(),
            repo_root: "/tmp/missing-workspace".to_string(),
            display_name: Some("missing".to_string()),
            default_ref: "live".to_string(),
            vcs_mode: false,
            schema_version: codecompass_core::constants::SCHEMA_VERSION,
            parser_version: codecompass_core::constants::PARSER_VERSION,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        codecompass_state::project::create_project(&conn, &missing).unwrap();

        let mut config = Config::default();
        config.storage.data_dir = tmp.path().join("health-data").to_string_lossy().to_string();

        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            current_project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "error",
            "overall status should be error when any registered project is not indexed"
        );
        let projects = payload.get("projects").unwrap().as_array().unwrap();
        let missing_status = projects
            .iter()
            .find(|p| p.get("project_id").and_then(|v| v.as_str()) == Some(missing_project_id))
            .and_then(|p| p.get("index_status"))
            .and_then(|v| v.as_str());
        assert_eq!(missing_status, Some("error"));
    }

    /// T122: search_code with debug ranking_reasons enabled returns per-result explanations
    #[test]
    fn t122_search_code_ranking_reasons_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let mut config = Config::default();
        config.debug.ranking_reasons = true;
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // ranking_reasons should be present in metadata
        let meta = payload.get("metadata").unwrap();
        let reasons = meta
            .get("ranking_reasons")
            .expect("ranking_reasons should be present in metadata when debug is enabled");
        let reasons_array = reasons.as_array().unwrap();

        // Should have one entry per result
        let results = payload.get("results").unwrap().as_array().unwrap();
        assert_eq!(
            reasons_array.len(),
            results.len(),
            "ranking_reasons should have one entry per result"
        );

        // Each reason should have all 7 fields
        if let Some(first) = reasons_array.first() {
            assert!(first.get("result_index").is_some(), "missing result_index");
            assert!(
                first.get("exact_match_boost").is_some(),
                "missing exact_match_boost"
            );
            assert!(
                first.get("qualified_name_boost").is_some(),
                "missing qualified_name_boost"
            );
            assert!(
                first.get("path_affinity").is_some(),
                "missing path_affinity"
            );
            assert!(
                first.get("definition_boost").is_some(),
                "missing definition_boost"
            );
            assert!(first.get("kind_match").is_some(), "missing kind_match");
            assert!(first.get("bm25_score").is_some(), "missing bm25_score");
            assert!(first.get("final_score").is_some(), "missing final_score");
        }
    }

    /// T123: search_code with debug ranking_reasons disabled (default) omits ranking_reasons
    #[test]
    fn t123_search_code_ranking_reasons_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default(); // ranking_reasons defaults to false
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let result = response.result.as_ref().unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();

        // ranking_reasons should be absent from metadata
        let meta = payload.get("metadata").unwrap();
        assert!(
            meta.get("ranking_reasons").is_none(),
            "ranking_reasons should be absent when debug is disabled"
        );
    }

    // ------------------------------------------------------------------
    // T134: tools/list schema verification for get_file_outline + health_check
    // ------------------------------------------------------------------

    #[test]
    fn t134_tools_list_schema_verification() {
        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "fake_project_id";

        let request = make_request("tools/list", json!({}));
        let response = handle_request(
            &request,
            &config,
            None,
            SchemaStatus::NotIndexed,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        let result = response.result.unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();

        // Verify get_file_outline schema
        let outline = tools
            .iter()
            .find(|t| t.get("name").unwrap().as_str().unwrap() == "get_file_outline")
            .expect("get_file_outline should be listed");
        let outline_schema = outline.get("inputSchema").unwrap();
        let outline_props = outline_schema.get("properties").unwrap();
        assert!(
            outline_props.get("path").is_some(),
            "get_file_outline should have 'path' property"
        );
        let outline_required = outline_schema.get("required").unwrap().as_array().unwrap();
        assert!(
            outline_required.contains(&json!("path")),
            "get_file_outline should require 'path'"
        );

        // Verify health_check schema
        let health = tools
            .iter()
            .find(|t| t.get("name").unwrap().as_str().unwrap() == "health_check")
            .expect("health_check should be listed");
        let health_schema = health.get("inputSchema").unwrap();
        let health_props = health_schema.get("properties").unwrap();
        assert!(
            health_props.get("workspace").is_some(),
            "health_check should have 'workspace' property"
        );
    }

    // ------------------------------------------------------------------
    // T135: Full E2E workflow test
    // ------------------------------------------------------------------

    #[test]
    fn t135_full_e2e_workflow() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        // Open a DB connection for outline queries
        let db_path = tmp.path().join("data/state.db");
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        // Must match the repo name used in build_fixture_index
        let project_id = "test-repo";

        // Step 1: health_check
        let request = make_request(
            "tools/call",
            json!({
                "name": "health_check",
                "arguments": {}
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(response.error.is_none(), "health_check should succeed");
        let payload = extract_payload_from_response(&response);
        assert_eq!(
            payload.get("status").unwrap().as_str().unwrap(),
            "ready",
            "health_check should report 'ready'"
        );

        // Step 2: locate_symbol with detail_level: "location"
        let request = make_request(
            "tools/call",
            json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "validate_token",
                    "detail_level": "location"
                }
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(
            response.error.is_none(),
            "locate_symbol(location) should succeed"
        );
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "locate should find validate_token");
        let vt = &results[0];
        assert!(
            vt.get("path").is_some(),
            "location level should include path"
        );
        assert!(
            vt.get("qualified_name").is_none(),
            "location level should NOT include qualified_name"
        );

        // Step 3: get_file_outline for the found file
        let found_path = vt.get("path").unwrap().as_str().unwrap();
        let request = make_request(
            "tools/call",
            json!({
                "name": "get_file_outline",
                "arguments": {
                    "path": found_path
                }
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(response.error.is_none(), "get_file_outline should succeed");
        let payload = extract_payload_from_response(&response);
        assert!(
            payload.get("symbols").is_some(),
            "get_file_outline should return symbols"
        );
        let symbols = payload.get("symbols").unwrap().as_array().unwrap();
        assert!(
            !symbols.is_empty(),
            "get_file_outline should return at least one symbol"
        );

        // Step 4: search_code with detail_level: "context"
        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token",
                    "detail_level": "context"
                }
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(
            response.error.is_none(),
            "search_code(context) should succeed"
        );
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "search should find validate_token");

        // Verify metadata conforms to Protocol v1
        let result = response.result.unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        let meta = payload.get("metadata").unwrap();
        assert!(
            meta.get("codecompass_protocol_version").is_some(),
            "metadata should include protocol version"
        );
        assert!(
            meta.get("freshness_status").is_some(),
            "metadata should include freshness_status"
        );
        assert!(
            meta.get("schema_status").is_some(),
            "metadata should include schema_status"
        );
    }

    // ------------------------------------------------------------------
    // T139: Backward compatibility - default detail_level is "signature"
    // ------------------------------------------------------------------

    #[test]
    fn t139_backward_compatibility_default_detail_level() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test_project";

        // Call locate_symbol without detail_level parameter
        let request = make_request(
            "tools/call",
            json!({
                "name": "locate_symbol",
                "arguments": {
                    "name": "validate_token"
                }
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(
            response.error.is_none(),
            "locate_symbol without detail_level should succeed"
        );
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "should find validate_token");

        let vt = results
            .iter()
            .find(|r| r.get("name").unwrap().as_str().unwrap() == "validate_token")
            .unwrap();

        // Signature-level fields should be present (default)
        assert!(
            vt.get("qualified_name").is_some(),
            "default should include qualified_name (signature level)"
        );
        assert!(
            vt.get("language").is_some(),
            "default should include language (signature level)"
        );

        // Context-only fields should NOT be present
        assert!(
            vt.get("body_preview").is_none(),
            "default should NOT include body_preview (context only)"
        );

        // Same test for search_code
        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token"
                }
            }),
        );
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            None,
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        assert!(
            response.error.is_none(),
            "search_code without detail_level should succeed"
        );
        let results = extract_results_from_response(&response);
        assert!(!results.is_empty(), "search should find results");

        // Verify metadata is present
        let result = response.result.unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        let text = content[0].get("text").unwrap().as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert!(
            payload.get("metadata").is_some(),
            "response should include metadata for backward compatibility"
        );
    }

    // ------------------------------------------------------------------
    // T138: Performance benchmark
    // ------------------------------------------------------------------

    #[test]
    fn t138_performance_benchmark() {
        let tmp = tempfile::tempdir().unwrap();
        let index_set = build_fixture_index(tmp.path());

        let db_path = tmp.path().join("data/state.db");
        let conn = codecompass_state::db::open_connection(&db_path).unwrap();

        let config = Config::default();
        let workspace = Path::new("/tmp/fake-workspace");
        let project_id = "test-repo";

        // Benchmark get_file_outline: measure 10 iterations, verify p95 < 50ms
        let mut outline_times = Vec::new();
        for _ in 0..10 {
            let request = make_request(
                "tools/call",
                json!({
                    "name": "get_file_outline",
                    "arguments": {
                        "path": "src/auth.rs"
                    }
                }),
            );
            let start = std::time::Instant::now();
            let response = handle_request(
                &request,
                &config,
                Some(&index_set),
                SchemaStatus::Compatible,
                None,
                Some(&conn),
                workspace,
                project_id,
                &test_prewarm_status(),
                &test_server_start(),
            );
            let elapsed = start.elapsed();
            outline_times.push(elapsed);
            assert!(response.error.is_none(), "get_file_outline should succeed");
        }
        outline_times.sort();
        let p95_outline = outline_times[8]; // 95th percentile of 10 samples
        assert!(
            p95_outline.as_millis() < 50,
            "get_file_outline p95 should be < 50ms, got {}ms",
            p95_outline.as_millis()
        );

        // Benchmark first-query latency: search_code after prewarm
        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token"
                }
            }),
        );
        let start = std::time::Instant::now();
        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );
        let elapsed = start.elapsed();
        assert!(response.error.is_none(), "search_code should succeed");
        assert!(
            elapsed.as_millis() < 500,
            "first-query latency should be < 500ms, got {}ms",
            elapsed.as_millis()
        );
    }

    // ------------------------------------------------------------------
    // Helper: extract the full JSON payload from an MCP tool response
    // ------------------------------------------------------------------

    fn extract_payload_from_response(response: &JsonRpcResponse) -> serde_json::Value {
        let result = response.result.as_ref().expect("result should be present");
        let content = result
            .get("content")
            .expect("result should have 'content'")
            .as_array()
            .expect("'content' should be an array");
        let text = content[0].get("text").unwrap().as_str().unwrap();
        serde_json::from_str(text).expect("text payload should be valid JSON")
    }

    // ------------------------------------------------------------------
    // Helper: build a fixture index inside a real git repo for freshness tests
    // ------------------------------------------------------------------

    fn build_fixture_index_in_git_repo(
        tmp_dir: &std::path::Path,
    ) -> (IndexSet, rusqlite::Connection, String) {
        use codecompass_indexer::{
            languages, parser, scanner, snippet_extract, symbol_extract, writer,
        };
        use codecompass_state::{db, schema, tantivy_index::IndexSet};

        let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/rust-sample");

        // Initialize a git repo in tmp_dir and commit
        let workspace = tmp_dir.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&workspace)
            .output()
            .unwrap();

        // Copy fixture files into workspace (recursive)
        fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
            std::fs::create_dir_all(dst).unwrap();
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let dest = dst.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir_recursive(&entry.path(), &dest);
                } else {
                    std::fs::copy(entry.path(), &dest).unwrap();
                }
            }
        }
        copy_dir_recursive(&fixture_dir, &workspace);

        // Initial commit
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&workspace)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&workspace)
            .output()
            .unwrap();

        // Get the initial commit hash
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        let initial_commit = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // Build index
        let data_dir = workspace.join(".codecompass/data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let index_set = IndexSet::open(&data_dir).unwrap();

        let db_path = data_dir.join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let repo = "test-repo";
        // Detect the current branch name
        let branch_output = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        let branch_name = String::from_utf8(branch_output.stdout)
            .unwrap()
            .trim()
            .to_string();

        let scanned = scanner::scan_directory(&workspace, 1_048_576);
        for file in &scanned {
            let source = std::fs::read_to_string(&file.path).unwrap();
            let tree = match parser::parse_file(&source, &file.language) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let extracted = languages::extract_symbols(&tree, &source, &file.language);
            let symbols = symbol_extract::build_symbol_records(
                &extracted,
                repo,
                &branch_name,
                &file.relative_path,
                None,
            );
            let snippets = snippet_extract::build_snippet_records(
                &extracted,
                repo,
                &branch_name,
                &file.relative_path,
                None,
            );

            let content_hash = blake3::hash(source.as_bytes()).to_hex().to_string();
            let filename = file.path.file_name().unwrap().to_string_lossy().to_string();
            let file_record = codecompass_core::types::FileRecord {
                repo: repo.to_string(),
                r#ref: branch_name.clone(),
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
        }

        // Store branch_state with initial commit
        let branch_entry = codecompass_state::branch_state::BranchState {
            repo: repo.to_string(),
            r#ref: branch_name.clone(),
            merge_base_commit: None,
            last_indexed_commit: initial_commit,
            overlay_dir: None,
            file_count: scanned.len() as i64,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_accessed_at: "2026-01-01T00:00:00Z".to_string(),
        };
        codecompass_state::branch_state::upsert_branch_state(&conn, &branch_entry).unwrap();

        (index_set, conn, branch_name)
    }

    /// Make a new commit in the workspace to make the index stale.
    fn make_workspace_stale(workspace: &std::path::Path) {
        let dummy = workspace.join("dummy.txt");
        std::fs::write(&dummy, "stale marker").unwrap();
        std::process::Command::new("git")
            .args(["add", "dummy.txt"])
            .current_dir(workspace)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "make stale"])
            .current_dir(workspace)
            .output()
            .unwrap();
    }

    // ------------------------------------------------------------------
    // T131: balanced policy with stale index returns results + stale status
    // ------------------------------------------------------------------

    #[test]
    fn t131_search_code_balanced_policy_stale_index() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
        let workspace = tmp.path().join("workspace");

        // Make the index stale by creating a new commit
        make_workspace_stale(&workspace);

        let config = Config::default();
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token",
                    "ref": branch_name,
                    "freshness_policy": "balanced"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            &workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(
            response.error.is_none(),
            "expected success, got error: {:?}",
            response.error
        );
        let payload = extract_payload_from_response(&response);

        // Should have results (query still runs)
        let results = payload.get("results").unwrap().as_array().unwrap();
        assert!(
            !results.is_empty(),
            "balanced policy should return results even when stale"
        );

        // Metadata should show stale
        let meta = payload.get("metadata").unwrap();
        assert_eq!(
            meta.get("freshness_status").unwrap().as_str().unwrap(),
            "stale",
            "freshness_status should be 'stale' for balanced policy with stale index"
        );
    }

    // ------------------------------------------------------------------
    // T132: strict policy with stale index returns index_stale error
    // ------------------------------------------------------------------

    #[test]
    fn t132_search_code_strict_policy_stale_index_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
        let workspace = tmp.path().join("workspace");

        // Make the index stale
        make_workspace_stale(&workspace);

        let config = Config::default();
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token",
                    "ref": branch_name,
                    "freshness_policy": "strict"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            &workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        // The response should be a "success" at JSON-RPC level but contain an error payload
        assert!(
            response.error.is_none(),
            "should be JSON-RPC success (error is in payload)"
        );
        let payload = extract_payload_from_response(&response);

        // Should have error object instead of results
        let error = payload
            .get("error")
            .expect("payload should have 'error' for strict+stale");
        assert_eq!(
            error.get("code").unwrap().as_str().unwrap(),
            "index_stale",
            "error code should be 'index_stale'"
        );
        assert!(
            error
                .get("message")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("stale"),
            "error message should mention stale"
        );
        let data = error.get("data").unwrap();
        assert!(
            data.get("last_indexed_commit").is_some(),
            "error data should include last_indexed_commit"
        );
        assert!(
            data.get("current_head").is_some(),
            "error data should include current_head"
        );
        assert!(
            data.get("suggestion").is_some(),
            "error data should include suggestion"
        );

        // Metadata should show stale
        let meta = payload.get("metadata").unwrap();
        assert_eq!(
            meta.get("freshness_status").unwrap().as_str().unwrap(),
            "stale"
        );
    }

    // ------------------------------------------------------------------
    // T133: best_effort policy with stale index returns results + stale
    // ------------------------------------------------------------------

    #[test]
    fn t133_search_code_best_effort_policy_stale_index() {
        let tmp = tempfile::tempdir().unwrap();
        let (index_set, conn, branch_name) = build_fixture_index_in_git_repo(tmp.path());
        let workspace = tmp.path().join("workspace");

        // Make the index stale
        make_workspace_stale(&workspace);

        let config = Config::default();
        let project_id = "test-repo";

        let request = make_request(
            "tools/call",
            json!({
                "name": "search_code",
                "arguments": {
                    "query": "validate_token",
                    "ref": branch_name,
                    "freshness_policy": "best_effort"
                }
            }),
        );

        let response = handle_request(
            &request,
            &config,
            Some(&index_set),
            SchemaStatus::Compatible,
            None,
            Some(&conn),
            &workspace,
            project_id,
            &test_prewarm_status(),
            &test_server_start(),
        );

        assert!(response.error.is_none(), "expected success");
        let payload = extract_payload_from_response(&response);

        // Should have results (best_effort always returns)
        let results = payload.get("results").unwrap().as_array().unwrap();
        assert!(
            !results.is_empty(),
            "best_effort policy should return results even when stale"
        );

        // Metadata should show stale
        let meta = payload.get("metadata").unwrap();
        assert_eq!(
            meta.get("freshness_status").unwrap().as_str().unwrap(),
            "stale",
            "freshness_status should be 'stale' for best_effort policy with stale index"
        );
    }
}
