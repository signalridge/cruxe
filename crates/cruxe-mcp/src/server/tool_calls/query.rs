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

    let Some(project) = cruxe_state::project::get_by_id(conn, project_id)? else {
        return Ok(None);
    };
    if !project.vcs_mode || effective_ref == project.default_ref {
        return Ok(None);
    }

    let Some(branch_state) =
        cruxe_state::branch_state::get_branch_state(conn, project_id, effective_ref)?
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
        .unwrap_or_else(|| cruxe_indexer::overlay::overlay_dir_for_ref(&data_dir, effective_ref));
    let data_dir_canonical = cruxe_state::overlay_paths::canonicalize_data_dir(&data_dir);
    let overlay_canonical = cruxe_state::overlay_paths::canonicalize_overlay_dir(&overlay_dir)
        .map_err(|err| {
            StateError::overlay_not_ready(project_id, effective_ref, format!("error={err}"))
        })?;
    let allowed =
        cruxe_state::overlay_paths::is_overlay_dir_allowed(&data_dir_canonical, &overlay_canonical)
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
        cruxe_state::tantivy_index::IndexSet::open_existing_at(&overlay_canonical).map_err(
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
    role: Option<&str>,
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
            role,
            language,
            limit,
        );
    }

    let results = locate::locate_symbol(
        &index_set.symbols,
        name,
        kind,
        role,
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
    search_options: search::SearchExecutionOptions,
) -> Result<search::SearchResponse, StateError> {
    let QueryExecutionContext {
        index_set,
        conn,
        config,
        project_id,
        effective_ref,
    } = ctx;

    if let Some(vcs) = resolve_vcs_overlay_context(conn, config, project_id, effective_ref)? {
        return search::search_code_vcs_merged_with_options(
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
            search_options.clone(),
        );
    }

    search::search_code_with_options(
        index_set,
        conn,
        query,
        Some(effective_ref),
        language,
        limit,
        debug_ranking,
        search_options,
    )
}

fn merge_freshness_status(
    left: cruxe_core::types::FreshnessStatus,
    right: cruxe_core::types::FreshnessStatus,
) -> cruxe_core::types::FreshnessStatus {
    use cruxe_core::types::FreshnessStatus;
    match (left, right) {
        (FreshnessStatus::Syncing, _) | (_, FreshnessStatus::Syncing) => FreshnessStatus::Syncing,
        (FreshnessStatus::Stale, _) | (_, FreshnessStatus::Stale) => FreshnessStatus::Stale,
        _ => FreshnessStatus::Fresh,
    }
}

fn freshness_status_label(status: cruxe_core::types::FreshnessStatus) -> &'static str {
    match status {
        cruxe_core::types::FreshnessStatus::Fresh => "fresh",
        cruxe_core::types::FreshnessStatus::Stale => "stale",
        cruxe_core::types::FreshnessStatus::Syncing => "syncing",
    }
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
    let role = arguments.get("role").and_then(|v| v.as_str());
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
        role,
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
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Truncated;
                metadata.safety_limit_applied = Some(true);
            }

            if ranking_explain_level != cruxe_core::types::RankingExplainLevel::Off {
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
    let role = arguments.get("role").and_then(|v| v.as_str());
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

    let debug_ranking = ranking_explain_level != cruxe_core::types::RankingExplainLevel::Off;
    // MCP input validation rejects out-of-range values (hard error) because
    // the caller is an AI agent that should retry with a corrected value.
    // Config-layer validation (config.rs) clamp+warns instead, since config
    // files may have stale or rounded values that should degrade gracefully.
    let semantic_ratio_override = match arguments.get("semantic_ratio").and_then(|v| v.as_f64()) {
        Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => Some(value),
        Some(_) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Parameter `semantic_ratio` must be a number between 0.0 and 1.0.",
                None,
                metadata,
            );
        }
        None => None,
    };
    let confidence_threshold_override = match arguments
        .get("confidence_threshold")
        .and_then(|v| v.as_f64())
    {
        Some(value) if value.is_finite() && (0.0..=1.0).contains(&value) => Some(value),
        Some(_) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Parameter `confidence_threshold` must be a number between 0.0 and 1.0.",
                None,
                metadata,
            );
        }
        None => None,
    };
    let plan_override = match arguments.get("plan").and_then(|v| v.as_str()) {
        Some(raw) => {
            let normalized = raw.trim().to_ascii_lowercase();
            let valid = matches!(
                normalized.as_str(),
                "lexical_fast"
                    | "hybrid_standard"
                    | "semantic_deep"
                    | "fast"
                    | "hybrid"
                    | "standard"
                    | "semantic"
                    | "deep"
                    | "lexical"
            );
            if !valid {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidInput,
                    "Parameter `plan` must be one of: lexical_fast, hybrid_standard, semantic_deep.",
                    Some(json!({ "plan": raw })),
                    metadata,
                );
            }
            if !config.search.adaptive_plan.allow_override {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidInput,
                    "Plan override is disabled by configuration (`search.adaptive_plan.allow_override=false`).",
                    Some(json!({ "plan": raw })),
                    metadata,
                );
            }
            Some(normalized)
        }
        None => None,
    };
    let search_options = search::SearchExecutionOptions {
        search_config: config.search.clone(),
        semantic_ratio_override,
        confidence_threshold_override,
        role: role.map(ToString::to_string),
        plan_override,
    };
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
        search_options,
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
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Truncated;
                metadata.safety_limit_applied = Some(true);
            }

            if let Some(reasons) = ranking_reasons.as_ref() {
                let aligned_reasons = align_ranking_reasons_to_dedup(reasons, &kept_reason_indices);
                metadata.ranking_reasons = ranking_reasons_payload(
                    aligned_reasons.into_iter().take(filtered.len()).collect(),
                    ranking_explain_level,
                );
            }

            metadata.semantic_mode = Some(response.metadata.semantic_mode.clone());
            metadata.semantic_enabled = Some(response.metadata.semantic_enabled);
            metadata.semantic_ratio_used = Some(response.metadata.semantic_ratio_used);
            metadata.semantic_triggered = Some(response.metadata.semantic_triggered);
            metadata.semantic_skipped_reason = response.metadata.semantic_skipped_reason.clone();
            metadata.semantic_fallback = Some(response.metadata.semantic_fallback);
            metadata.semantic_degraded = Some(response.metadata.semantic_degraded);
            metadata.semantic_limit_used = Some(response.metadata.semantic_limit_used);
            metadata.lexical_fanout_used = Some(response.metadata.lexical_fanout_used);
            metadata.semantic_fanout_used = Some(response.metadata.semantic_fanout_used);
            metadata.semantic_budget_exhausted = Some(response.metadata.semantic_budget_exhausted);
            metadata.external_provider_blocked = Some(response.metadata.external_provider_blocked);
            metadata.embedding_model_version =
                Some(response.metadata.embedding_model_version.clone());
            metadata.rerank_provider = Some(response.metadata.rerank_provider.clone());
            metadata.rerank_fallback = Some(response.metadata.rerank_fallback);
            metadata.rerank_fallback_reason = response.metadata.rerank_fallback_reason.clone();
            metadata.low_confidence = Some(response.metadata.low_confidence);
            metadata.suggested_action = response.metadata.suggested_action.clone();
            metadata.confidence_threshold = Some(response.metadata.confidence_threshold);
            metadata.top_score = Some(response.metadata.top_score);
            metadata.score_margin = Some(response.metadata.score_margin);
            metadata.channel_agreement = Some(response.metadata.channel_agreement);
            metadata.query_intent_confidence = Some(response.metadata.query_intent_confidence);
            metadata.intent_escalation_hint = response.metadata.intent_escalation_hint.clone();
            metadata.query_plan_selected = Some(response.metadata.query_plan_selected.clone());
            metadata.query_plan_executed = Some(response.metadata.query_plan_executed.clone());
            metadata.query_plan_selection_reason =
                Some(response.metadata.query_plan_selection_reason.clone());
            metadata.query_plan_downgraded = Some(response.metadata.query_plan_downgraded);
            metadata.query_plan_downgrade_reason =
                response.metadata.query_plan_downgrade_reason.clone();
            metadata.query_plan_budget_used =
                serde_json::to_value(&response.metadata.query_plan_budget_used).ok();
            if !response.metadata.warnings.is_empty() {
                let mut warnings = metadata.warnings.take().unwrap_or_default();
                warnings.extend(response.metadata.warnings.clone());
                metadata.warnings = Some(warnings);
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

pub(super) fn handle_get_call_graph(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
    let path = arguments.get("path").and_then(|value| value.as_str());
    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let direction_raw = arguments
        .get("direction")
        .and_then(|value| value.as_str())
        .unwrap_or("both");
    let requested_depth = arguments
        .get("depth")
        .and_then(|value| value.as_u64())
        .unwrap_or(1) as u32;
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

    let Some(direction) = call_graph::CallGraphDirection::parse(direction_raw) else {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `direction` must be one of: callers, callees, both.",
            Some(json!({ "direction": direction_raw })),
            base_metadata,
        );
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

    match cruxe_state::branch_state::get_branch_state(c, project_id, &effective_ref) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::RefNotIndexed,
                "The requested ref has no indexed state yet.",
                Some(json!({
                    "ref": effective_ref,
                    "remediation": "Run sync_repo for this ref before querying.",
                })),
                validation_metadata(&effective_ref, schema_status),
            );
        }
        Err(err) => {
            let (code, message, data) = map_state_error(&err);
            return tool_error_response(
                id,
                code,
                message,
                data,
                validation_metadata(&effective_ref, schema_status),
            );
        }
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
    let mut warnings = Vec::new();
    if requested_depth == 0 {
        warnings.push("Depth 0 is invalid; using depth=1.".to_string());
    }
    if requested_depth > call_graph::MAX_CALL_GRAPH_DEPTH {
        warnings.push(format!(
            "Requested depth {} exceeds max {}; clamped.",
            requested_depth,
            call_graph::MAX_CALL_GRAPH_DEPTH
        ));
    }
    if !warnings.is_empty() {
        metadata.warnings = Some(warnings);
    }

    match call_graph::get_call_graph(
        c,
        project_id,
        &effective_ref,
        &call_graph::CallGraphRequest {
            symbol_name,
            path,
            direction,
            depth: requested_depth,
            limit,
        },
    ) {
        Ok(result) => {
            if result.truncated {
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Truncated;
            }
            let mut payload = match serde_json::to_value(result) {
                Ok(value) => value,
                Err(err) => {
                    return tool_error_response(
                        id,
                        ProtocolErrorCode::InternalError,
                        "Failed to serialize get_call_graph payload.",
                        Some(json!({ "error": err.to_string() })),
                        metadata.clone(),
                    );
                }
            };
            if let Value::Object(object) = &mut payload {
                object.insert("metadata".to_string(), json!(metadata));
            }
            tool_text_response(id, payload)
        }
        Err(call_graph::CallGraphError::SymbolNotFound) => tool_error_response(
            id,
            ProtocolErrorCode::SymbolNotFound,
            "No symbol matching the requested name was found.",
            Some(json!({
                "symbol_name": symbol_name,
                "path": path,
                "ref": effective_ref,
            })),
            metadata,
        ),
        Err(call_graph::CallGraphError::State(err)) => {
            let (code, message, data) = map_state_error(&err);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_compare_symbol_between_commits(
    params: QueryToolParams<'_>,
) -> JsonRpcResponse {
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
    let path = arguments.get("path").and_then(|value| value.as_str());
    let base_ref = arguments
        .get("base_ref")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let head_ref = arguments
        .get("head_ref")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let base_metadata = validation_metadata(head_ref, schema_status);

    if symbol_name.trim().is_empty() || base_ref.trim().is_empty() || head_ref.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameters `symbol_name`, `base_ref`, and `head_ref` are required.",
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
            ref_name: head_ref,
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
            ref_name: head_ref,
        });
    };

    for ref_name in [base_ref, head_ref] {
        match cruxe_state::branch_state::get_branch_state(c, project_id, ref_name) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::RefNotIndexed,
                    "The requested ref has no indexed state yet.",
                    Some(json!({
                        "ref": ref_name,
                        "remediation": "Run sync_repo for this ref before querying.",
                    })),
                    validation_metadata(ref_name, schema_status),
                );
            }
            Err(err) => {
                let (code, message, data) = map_state_error(&err);
                return tool_error_response(
                    id,
                    code,
                    message,
                    data,
                    validation_metadata(ref_name, schema_status),
                );
            }
        }
    }
    let base_freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        base_ref,
        schema_status,
    );
    if let Some(block) = base_freshness.block_response {
        return block;
    }
    let head_freshness = check_and_enforce_freshness(
        id.clone(),
        arguments,
        config,
        conn,
        workspace,
        project_id,
        head_ref,
        schema_status,
    );
    if let Some(block) = head_freshness.block_response {
        return block;
    }

    let mut metadata = head_freshness.metadata;
    let merged_freshness = merge_freshness_status(
        metadata.freshness_status,
        base_freshness.metadata.freshness_status,
    );
    if base_freshness.metadata.freshness_status != cruxe_core::types::FreshnessStatus::Fresh {
        metadata.freshness_status = merged_freshness;
        let warnings = metadata.warnings.get_or_insert_with(Vec::new);
        warnings.push(format!(
            "Base ref `{}` freshness_status is `{}`.",
            base_ref,
            freshness_status_label(base_freshness.metadata.freshness_status)
        ));
    }

    match symbol_compare::compare_symbol_between_refs(
        c,
        project_id,
        symbol_name,
        path,
        base_ref,
        head_ref,
    ) {
        Ok(result) => {
            let mut payload = match serde_json::to_value(result) {
                Ok(value) => value,
                Err(err) => {
                    return tool_error_response(
                        id,
                        ProtocolErrorCode::InternalError,
                        "Failed to serialize compare_symbol_between_commits payload.",
                        Some(json!({ "error": err.to_string() })),
                        metadata.clone(),
                    );
                }
            };
            if let Value::Object(object) = &mut payload {
                object.insert("metadata".to_string(), json!(metadata));
            }
            tool_text_response(id, payload)
        }
        Err(symbol_compare::SymbolCompareError::SymbolNotFound) => tool_error_response(
            id,
            ProtocolErrorCode::SymbolNotFound,
            "No symbol matching the requested name was found.",
            Some(json!({
                "symbol_name": symbol_name,
                "path": path,
                "base_ref": base_ref,
                "head_ref": head_ref,
            })),
            metadata.clone(),
        ),
        Err(symbol_compare::SymbolCompareError::State(err)) => {
            let (code, message, data) = map_state_error(&err);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_suggest_followup_queries(params: QueryToolParams<'_>) -> JsonRpcResponse {
    let QueryToolParams {
        id,
        arguments,
        conn,
        workspace,
        project_id,
        schema_status,
        ..
    } = params;

    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let metadata = validation_metadata(&effective_ref, schema_status);

    let Some(previous_query) = arguments
        .get("previous_query")
        .and_then(|value| value.as_object())
    else {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `previous_query` is required and must be an object.",
            None,
            metadata,
        );
    };
    let previous_query_tool = previous_query
        .get("tool")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if previous_query_tool.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `previous_query.tool` is required.",
            None,
            metadata,
        );
    }
    let previous_query_params = previous_query
        .get("params")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(previous_results) = arguments.get("previous_results") else {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `previous_results` is required.",
            None,
            metadata,
        );
    };
    if !previous_results.is_object() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `previous_results` must be an object.",
            None,
            metadata,
        );
    }
    let confidence_threshold = arguments
        .get("confidence_threshold")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.5);

    let request = followup::FollowupRequest {
        previous_query_tool: previous_query_tool.to_string(),
        previous_query_params,
        previous_results: previous_results.clone(),
        confidence_threshold,
    };
    let mut payload = match serde_json::to_value(followup::suggest_followup_queries(&request)) {
        Ok(value) => value,
        Err(err) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InternalError,
                "Failed to serialize suggest_followup_queries payload.",
                Some(json!({ "error": err.to_string() })),
                metadata.clone(),
            );
        }
    };
    if let Value::Object(object) = &mut payload {
        object.insert("metadata".to_string(), json!(metadata));
    }
    tool_text_response(id, payload)
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
        cruxe_state::project::get_by_id(connection, project_id)
            .ok()
            .flatten()
            .map(|project| project.default_ref)
    })
}
