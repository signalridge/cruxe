use super::*;

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

    match locate::locate_symbol(
        &index_set.symbols,
        name,
        kind,
        language,
        Some(&effective_ref),
        limit,
    ) {
        Ok(results) => {
            let total_candidates = results.len();
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
