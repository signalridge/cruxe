use super::*;
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::warn;

struct VcsOverlayContext {
    default_ref: String,
    overlay_index_set: IndexSet,
    tombstones: HashSet<String>,
}

struct QueryExecutionContext<'a> {
    index_set: &'a IndexSet,
    conn: Option<&'a rusqlite::Connection>,
    config: &'a Config,
    project_id: &'a str,
    effective_ref: &'a str,
}

fn resolve_vcs_overlay_context(
    conn: Option<&rusqlite::Connection>,
    config: &Config,
    project_id: &str,
    effective_ref: &str,
) -> Result<Option<VcsOverlayContext>, StateError> {
    let Some(conn) = conn else {
        return Ok(None);
    };

    let Some(project) = codecompass_state::project::get_by_id(conn, project_id)? else {
        return Ok(None);
    };
    if !project.vcs_mode || effective_ref == project.default_ref {
        return Ok(None);
    }

    let Some(branch_state) =
        codecompass_state::branch_state::get_branch_state(conn, project_id, effective_ref)?
    else {
        return Err(StateError::ref_not_indexed(project_id, effective_ref));
    };

    if matches!(
        branch_state.status.as_str(),
        "syncing" | "rebuilding" | "indexing"
    ) {
        return Err(StateError::overlay_not_ready(
            project_id,
            effective_ref,
            format!("status={}", branch_state.status),
        ));
    }

    let data_dir = config.project_data_dir(project_id);
    let overlay_dir = branch_state
        .overlay_dir
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { data_dir.join(p) })
        .unwrap_or_else(|| {
            codecompass_indexer::overlay::overlay_dir_for_ref(&data_dir, effective_ref)
        });
    let data_dir_canonical = codecompass_state::overlay_paths::canonicalize_data_dir(&data_dir);
    let overlay_canonical =
        codecompass_state::overlay_paths::canonicalize_overlay_dir(&overlay_dir).map_err(
            |err| StateError::overlay_not_ready(project_id, effective_ref, format!("error={err}")),
        )?;
    let allowed = codecompass_state::overlay_paths::is_overlay_dir_allowed(
        &data_dir_canonical,
        &overlay_canonical,
    )
    .map_err(|err| {
        StateError::overlay_not_ready(project_id, effective_ref, format!("error={err}"))
    })?;
    if !allowed {
        return Err(StateError::overlay_not_ready(
            project_id,
            effective_ref,
            format!(
                "overlay_path_outside_data_dir: {}",
                overlay_canonical.display()
            ),
        ));
    }
    let overlay_index_set =
        codecompass_state::tantivy_index::IndexSet::open_existing_at(&overlay_canonical).map_err(
            |err| StateError::overlay_not_ready(project_id, effective_ref, format!("error={err}")),
        )?;

    let mut tombstone_cache = TombstoneCache::new(Some(conn));
    let tombstones = match tombstone_cache.load_paths(project_id, effective_ref) {
        Ok(paths) => paths.clone(),
        Err(err) => {
            warn!(
                project_id = %project_id,
                ref_name = %effective_ref,
                error = %err,
                "Failed to load tombstones; continuing without suppression"
            );
            HashSet::new()
        }
    };

    Ok(Some(VcsOverlayContext {
        default_ref: project.default_ref,
        overlay_index_set,
        tombstones,
    }))
}

fn execute_locate_with_optional_overlay(
    ctx: QueryExecutionContext<'_>,
    name: &str,
    kind: Option<&str>,
    language: Option<&str>,
    limit: usize,
) -> Result<(Vec<locate::LocateResult>, usize), StateError> {
    let QueryExecutionContext {
        index_set,
        conn,
        config,
        project_id,
        effective_ref,
    } = ctx;

    if let Some(vcs) = resolve_vcs_overlay_context(conn, config, project_id, effective_ref)? {
        return locate::locate_symbol_vcs_merged(
            locate::VcsLocateContext {
                base_index: &index_set.symbols,
                overlay_index: &vcs.overlay_index_set.symbols,
                tombstones: &vcs.tombstones,
                base_ref: &vcs.default_ref,
                target_ref: effective_ref,
            },
            name,
            kind,
            language,
            limit,
        );
    }

    let results = locate::locate_symbol(
        &index_set.symbols,
        name,
        kind,
        language,
        Some(effective_ref),
        limit,
    )?;
    let total_candidates = results.len();
    Ok((results, total_candidates))
}

fn execute_search_with_optional_overlay(
    ctx: QueryExecutionContext<'_>,
    query: &str,
    language: Option<&str>,
    limit: usize,
    debug_ranking: bool,
) -> Result<search::SearchResponse, StateError> {
    let QueryExecutionContext {
        index_set,
        conn,
        config,
        project_id,
        effective_ref,
    } = ctx;

    if let Some(vcs) = resolve_vcs_overlay_context(conn, config, project_id, effective_ref)? {
        return search::search_code_vcs_merged(
            search::VcsSearchContext {
                base_index_set: index_set,
                overlay_index_set: &vcs.overlay_index_set,
                tombstones: &vcs.tombstones,
                base_ref: &vcs.default_ref,
                target_ref: effective_ref,
            },
            conn,
            query,
            language,
            limit,
            debug_ranking,
        );
    }

    search::search_code(
        index_set,
        conn,
        query,
        Some(effective_ref),
        language,
        limit,
        debug_ranking,
    )
}

pub(super) fn handle_locate_symbol(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        arguments,
        config,
        index_set,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
    } = params;

    let name = arguments.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let kind = arguments.get("kind").and_then(|v| v.as_str());
    let language = arguments.get("language").and_then(|v| v.as_str());
    let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let detail_level = parse_detail_level(arguments);
    let compact = parse_compact(arguments);
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let base_metadata = validation_metadata(&effective_ref, schema_status);

    if name.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `name` is required.",
            None,
            base_metadata,
        );
    }

    let ranking_explain_level = match resolve_ranking_explain_level(arguments, config) {
        Ok(level) => level,
        Err(message) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                message,
                None,
                base_metadata,
            );
        }
    };

    let Some(index_set) = index_set else {
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

    if schema_status != SchemaStatus::Compatible {
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
    }

    let freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        &effective_ref,
        schema_status,
    );
    if let Some(block) = freshness.block_response {
        return block;
    }
    let mut metadata = freshness.metadata;

    match execute_locate_with_optional_overlay(
        QueryExecutionContext {
            index_set,
            conn,
            config,
            project_id,
            effective_ref: &effective_ref,
        },
        name,
        kind,
        language,
        limit,
    ) {
        Ok((results, total_candidates)) => {
            let (results, suppressed_duplicate_count) = dedup_locate_results(results);
            if suppressed_duplicate_count > 0 {
                metadata.suppressed_duplicate_count = Some(suppressed_duplicate_count);
            }

            let result_values: Vec<Value> = results
                .iter()
                .filter_map(|r| serde_json::to_value(r).ok())
                .collect();
            let FilteredResultPayload {
                filtered,
                safety_limit_applied,
            } = build_filtered_result_payload(
                result_values,
                detail_level,
                compact,
                conn,
                project_id,
                &effective_ref,
                config.search.max_response_bytes,
            );
            if safety_limit_applied {
                metadata.result_completeness =
                    codecompass_core::types::ResultCompleteness::Truncated;
                metadata.safety_limit_applied = Some(true);
            }

            if ranking_explain_level != codecompass_core::types::RankingExplainLevel::Off {
                let reasons = ranking::locate_ranking_reasons(&results, name);
                metadata.ranking_reasons = ranking_reasons_payload(
                    reasons.into_iter().take(filtered.len()).collect(),
                    ranking_explain_level,
                );
            }

            let mut response = json!({
                "results": filtered,
                "total_candidates": total_candidates,
                "metadata": metadata,
            });
            if safety_limit_applied {
                response["suggested_next_actions"] = json!(deterministic_locate_suggested_actions(
                    name,
                    &effective_ref,
                    limit
                ));
            }

            tool_text_response(id, response)
        }
        Err(e) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_search_code(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        arguments,
        config,
        index_set,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
    } = params;

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
    let compact = parse_compact(arguments);
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let base_metadata = validation_metadata(&effective_ref, schema_status);

    if query.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `query` is required.",
            None,
            base_metadata,
        );
    }

    let ranking_explain_level = match resolve_ranking_explain_level(arguments, config) {
        Ok(level) => level,
        Err(message) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                message,
                None,
                base_metadata,
            );
        }
    };

    let Some(index_set) = index_set else {
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

    if schema_status != SchemaStatus::Compatible {
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
    }

    let freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        &effective_ref,
        schema_status,
    );
    if let Some(block) = freshness.block_response {
        return block;
    }
    let mut metadata = freshness.metadata;

    let debug_ranking = ranking_explain_level != codecompass_core::types::RankingExplainLevel::Off;
    match execute_search_with_optional_overlay(
        QueryExecutionContext {
            index_set,
            conn,
            config,
            project_id,
            effective_ref: &effective_ref,
        },
        query,
        language,
        limit,
        debug_ranking,
    ) {
        Ok(response) => {
            let mut response = response;
            let ranking_reasons = response.ranking_reasons.take();
            let (results, kept_reason_indices, suppressed_duplicate_count) =
                dedup_search_results(std::mem::take(&mut response.results));
            if suppressed_duplicate_count > 0 {
                metadata.suppressed_duplicate_count = Some(suppressed_duplicate_count);
            }

            let result_values: Vec<Value> = results
                .iter()
                .filter_map(|r| serde_json::to_value(r).ok())
                .collect();
            let FilteredResultPayload {
                filtered,
                safety_limit_applied,
            } = build_filtered_result_payload(
                result_values,
                detail_level,
                compact,
                conn,
                project_id,
                &effective_ref,
                config.search.max_response_bytes,
            );
            if safety_limit_applied {
                metadata.result_completeness =
                    codecompass_core::types::ResultCompleteness::Truncated;
                metadata.safety_limit_applied = Some(true);
            }

            if let Some(reasons) = ranking_reasons.as_ref() {
                let aligned_reasons = align_ranking_reasons_to_dedup(reasons, &kept_reason_indices);
                metadata.ranking_reasons = ranking_reasons_payload(
                    aligned_reasons.into_iter().take(filtered.len()).collect(),
                    ranking_explain_level,
                );
            }

            let suggested_next_actions = if safety_limit_applied {
                deterministic_suggested_actions(
                    &response.suggested_next_actions,
                    query,
                    &effective_ref,
                    limit,
                )
            } else {
                response.suggested_next_actions.clone()
            };

            let mut result = json!({
                "results": filtered,
                "query_intent": &response.query_intent,
                "total_candidates": response.total_candidates,
                "suggested_next_actions": suggested_next_actions,
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

pub(super) fn handle_diff_context(params: QueryToolParams<'_>) -> JsonRpcResponse {
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

    let head_ref = arguments
        .get("head_ref")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| resolve_tool_ref(None, workspace, conn, project_id));
    let base_ref = arguments
        .get("base_ref")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            project_default_ref(conn, project_id).unwrap_or_else(|| "main".to_string())
        });
    let path_filter = arguments
        .get("path_filter")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());
    let limit = arguments
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(50) as usize;

    if schema_status != SchemaStatus::Compatible {
        return tool_compatibility_error(ToolCompatibilityParams {
            id,
            schema_status,
            compatibility_reason,
            config,
            conn,
            workspace,
            project_id,
            ref_name: &head_ref,
        });
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
            ref_name: &head_ref,
        });
    };

    let freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        &head_ref,
        schema_status,
    );
    if let Some(block) = freshness.block_response {
        return block;
    }
    let metadata = freshness.metadata;

    match diff_context::diff_context(
        c,
        workspace,
        project_id,
        &base_ref,
        &head_ref,
        path_filter,
        limit,
    ) {
        Ok(result) => {
            let mut payload = serde_json::to_value(result)
                .unwrap_or_else(|_| json!({"error": "failed to serialize diff_context payload"}));
            if let Value::Object(object) = &mut payload {
                object.insert("metadata".to_string(), json!(metadata));
            }
            tool_text_response(id, payload)
        }
        Err(err) => {
            let (code, message, data) = map_state_error(&err);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_find_references(params: QueryToolParams<'_>) -> JsonRpcResponse {
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

    let symbol_name = arguments
        .get("symbol_name")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let kind = arguments.get("kind").and_then(|value| value.as_str());
    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let limit = arguments
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(20) as usize;
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);

    let base_metadata = validation_metadata(&effective_ref, schema_status);
    if symbol_name.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `symbol_name` is required.",
            None,
            base_metadata,
        );
    }

    if schema_status != SchemaStatus::Compatible {
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
            ref_name: &effective_ref,
        });
    };

    let freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        &effective_ref,
        schema_status,
    );
    if let Some(block) = freshness.block_response {
        return block;
    }
    let metadata = freshness.metadata;

    match find_references::find_references(
        c,
        workspace,
        project_id,
        &effective_ref,
        kind,
        symbol_name,
        limit,
    ) {
        Ok(result) => {
            let mut payload = serde_json::to_value(result).unwrap_or_else(
                |_| json!({"error": "failed to serialize find_references payload"}),
            );
            if let Value::Object(object) = &mut payload {
                object.insert("metadata".to_string(), json!(metadata));
            }
            tool_text_response(id, payload)
        }
        Err(find_references::FindReferencesError::SymbolNotFound) => tool_error_response(
            id,
            ProtocolErrorCode::SymbolNotFound,
            "No symbol matching the requested name was found.",
            Some(json!({
                "symbol_name": symbol_name,
                "ref": effective_ref,
            })),
            metadata,
        ),
        Err(find_references::FindReferencesError::NoEdgesAvailable) => tool_error_response(
            id,
            ProtocolErrorCode::NoEdgesAvailable,
            "symbol_edges data is not available for this project/ref yet.",
            Some(json!({
                "symbol_name": symbol_name,
                "ref": effective_ref,
                "remediation": "Run index_repo/sync_repo to populate relation edges.",
            })),
            metadata,
        ),
        Err(find_references::FindReferencesError::State(err)) => {
            let (code, message, data) = map_state_error(&err);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_explain_ranking(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        arguments,
        config,
        index_set,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
    } = params;

    let query = arguments
        .get("query")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let result_path = arguments
        .get("result_path")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let result_line_start = arguments
        .get("result_line_start")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as u32;
    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let language = arguments.get("language").and_then(|value| value.as_str());
    let limit = arguments
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(200) as usize;
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let base_metadata = validation_metadata(&effective_ref, schema_status);

    if query.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `query` is required.",
            None,
            base_metadata,
        );
    }
    if result_path.trim().is_empty() || result_line_start == 0 {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameters `result_path` and `result_line_start` are required.",
            None,
            base_metadata,
        );
    }

    let Some(index_set) = index_set else {
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
    if schema_status != SchemaStatus::Compatible {
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
    }

    let freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        &effective_ref,
        schema_status,
    );
    if let Some(block) = freshness.block_response {
        return block;
    }
    let metadata = freshness.metadata;

    match explain_ranking::explain_ranking(
        index_set,
        conn,
        query,
        result_path,
        result_line_start,
        Some(&effective_ref),
        language,
        limit,
    ) {
        Ok(explanation) => {
            let mut payload = serde_json::to_value(explanation).unwrap_or_else(
                |_| json!({"error": "failed to serialize explain_ranking payload"}),
            );
            if let Value::Object(object) = &mut payload {
                object.insert("metadata".to_string(), json!(metadata));
            }
            tool_text_response(id, payload)
        }
        Err(explain_ranking::ExplainRankingError::ResultNotFound) => tool_error_response(
            id,
            ProtocolErrorCode::ResultNotFound,
            "No result matching the requested path/line was found for this query.",
            Some(json!({
                "query": query,
                "result_path": result_path,
                "result_line_start": result_line_start,
            })),
            metadata,
        ),
        Err(explain_ranking::ExplainRankingError::State(err)) => {
            let (code, message, data) = map_state_error(&err);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

fn project_default_ref(conn: Option<&rusqlite::Connection>, project_id: &str) -> Option<String> {
    conn.and_then(|connection| {
        codecompass_state::project::get_by_id(connection, project_id)
            .ok()
            .flatten()
            .map(|project| project.default_ref)
    })
}
