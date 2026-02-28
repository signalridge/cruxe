use super::*;

const MAX_CONTEXT_PACK_BUDGET_TOKENS: usize = 200_000;

pub(super) fn handle_get_code_context(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let mut metadata = validation_metadata(&effective_ref, schema_status);

    if query.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `query` is required.",
            None,
            metadata,
        );
    }
    let max_tokens = match arguments.get("max_tokens") {
        None => 4000usize,
        Some(value) => {
            if let Some(raw_u64) = value.as_u64() {
                if raw_u64 == 0 {
                    return tool_error_response(
                        id,
                        ProtocolErrorCode::InvalidMaxTokens,
                        "Parameter `max_tokens` must be greater than 0.",
                        None,
                        metadata,
                    );
                }
                match usize::try_from(raw_u64) {
                    Ok(v) => v,
                    Err(_) => {
                        return tool_error_response(
                            id,
                            ProtocolErrorCode::InvalidMaxTokens,
                            "Parameter `max_tokens` is too large.",
                            None,
                            metadata,
                        );
                    }
                }
            } else {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidMaxTokens,
                    "Parameter `max_tokens` must be a positive integer.",
                    None,
                    metadata,
                );
            }
        }
    };
    let strategy = match cruxe_query::context::parse_strategy(
        arguments.get("strategy").and_then(|v| v.as_str()),
    ) {
        Ok(strategy) => strategy,
        Err(cruxe_query::context::ContextError::InvalidStrategy) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidStrategy,
                "Parameter `strategy` must be `breadth` or `depth`.",
                None,
                metadata,
            );
        }
        Err(_) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Invalid `strategy` parameter.",
                None,
                metadata,
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
    metadata = freshness.metadata;

    match cruxe_query::context::get_code_context(cruxe_query::context::GetCodeContextParams {
        index_set,
        conn,
        workspace,
        query,
        ref_name: Some(&effective_ref),
        language,
        max_tokens,
        strategy,
    }) {
        Ok(response) => {
            if response.truncated {
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Truncated;
            }
            let mut merged_metadata = response.metadata;
            if let Some(obj) = merged_metadata.as_object_mut() {
                obj.insert(
                    "cruxe_protocol_version".to_string(),
                    json!(metadata.cruxe_protocol_version),
                );
                obj.insert(
                    "freshness_status".to_string(),
                    json!(metadata.freshness_status),
                );
                obj.insert(
                    "indexing_status".to_string(),
                    json!(metadata.indexing_status),
                );
                obj.insert(
                    "result_completeness".to_string(),
                    json!(metadata.result_completeness),
                );
                obj.insert("ref".to_string(), json!(metadata.r#ref));
                obj.insert("schema_status".to_string(), json!(metadata.schema_status));
            }
            tool_text_response(
                id,
                json!({
                    "context_items": response.context_items,
                    "estimated_tokens": response.estimated_tokens,
                    "truncated": response.truncated,
                    "metadata": merged_metadata,
                }),
            )
        }
        Err(cruxe_query::context::ContextError::InvalidMaxTokens) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidMaxTokens,
            "Parameter `max_tokens` must be greater than 0.",
            None,
            metadata,
        ),
        Err(cruxe_query::context::ContextError::State(e)) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
        Err(cruxe_query::context::ContextError::InvalidStrategy) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidStrategy,
            "Parameter `strategy` must be `breadth` or `depth`.",
            None,
            metadata,
        ),
    }
}

pub(super) fn handle_build_context_pack(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
    let requested_ref = arguments.get("ref").and_then(|value| value.as_str());
    let language = arguments.get("language").and_then(|value| value.as_str());
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
    let mut metadata = validation_metadata(&effective_ref, schema_status);

    if query.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `query` is required.",
            None,
            metadata,
        );
    }

    let budget_tokens = match arguments.get("budget_tokens") {
        None => 4000usize,
        Some(value) => {
            if let Some(raw_u64) = value.as_u64() {
                if raw_u64 == 0 {
                    return tool_error_response(
                        id,
                        ProtocolErrorCode::InvalidMaxTokens,
                        "Parameter `budget_tokens` must be greater than 0.",
                        None,
                        metadata,
                    );
                }
                match usize::try_from(raw_u64) {
                    Ok(v) => {
                        if v > MAX_CONTEXT_PACK_BUDGET_TOKENS {
                            return tool_error_response(
                                id,
                                ProtocolErrorCode::InvalidMaxTokens,
                                "Parameter `budget_tokens` must be less than or equal to 200000.",
                                None,
                                metadata,
                            );
                        }
                        v
                    }
                    Err(_) => {
                        return tool_error_response(
                            id,
                            ProtocolErrorCode::InvalidMaxTokens,
                            "Parameter `budget_tokens` is too large.",
                            None,
                            metadata,
                        );
                    }
                }
            } else {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidMaxTokens,
                    "Parameter `budget_tokens` must be a positive integer.",
                    None,
                    metadata,
                );
            }
        }
    };

    let max_candidates = match arguments.get("max_candidates") {
        None => cruxe_query::context_pack::DEFAULT_MAX_CANDIDATES,
        Some(value) => {
            if let Some(raw_u64) = value.as_u64() {
                if raw_u64 == 0 {
                    return tool_error_response(
                        id,
                        ProtocolErrorCode::InvalidInput,
                        "Parameter `max_candidates` must be greater than 0.",
                        None,
                        metadata,
                    );
                }
                match usize::try_from(raw_u64) {
                    Ok(v) => v,
                    Err(_) => {
                        return tool_error_response(
                            id,
                            ProtocolErrorCode::InvalidInput,
                            "Parameter `max_candidates` is too large.",
                            None,
                            metadata,
                        );
                    }
                }
            } else {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidInput,
                    "Parameter `max_candidates` must be a positive integer.",
                    None,
                    metadata,
                );
            }
        }
    };

    let mode = match cruxe_query::context_pack::parse_mode(
        arguments.get("mode").and_then(|value| value.as_str()),
    ) {
        Ok(mode) => mode,
        Err(cruxe_query::context_pack::ContextPackError::InvalidMode) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Parameter `mode` must be `full` or `edit_minimal`.",
                None,
                metadata,
            );
        }
        Err(_) => {
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Invalid `mode` parameter.",
                None,
                metadata,
            );
        }
    };

    let section_caps = match arguments.get("section_caps") {
        None => None,
        Some(value) => match serde_json::from_value::<cruxe_query::context_pack::SectionCapsPatch>(
            value.clone(),
        ) {
            Ok(patch) => {
                Some(cruxe_query::context_pack::SectionCaps::defaults(mode).with_patch(patch))
            }
            Err(_) => {
                return tool_error_response(
                    id,
                    ProtocolErrorCode::InvalidInput,
                    "Parameter `section_caps` must be an object with non-negative integer caps.",
                    None,
                    metadata,
                );
            }
        },
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
    metadata = freshness.metadata;

    match cruxe_query::context_pack::build_context_pack(
        cruxe_query::context_pack::BuildContextPackParams {
            index_set,
            conn,
            workspace,
            query,
            ref_name: Some(&effective_ref),
            language,
            budget_tokens,
            max_candidates,
            mode,
            section_caps,
        },
    ) {
        Ok(response) => {
            if response.dropped_candidates > 0 {
                metadata.result_completeness = cruxe_core::types::ResultCompleteness::Truncated;
            }

            let mut merged_metadata = response.metadata;
            if let Some(obj) = merged_metadata.as_object_mut() {
                obj.insert(
                    "cruxe_protocol_version".to_string(),
                    json!(metadata.cruxe_protocol_version),
                );
                obj.insert(
                    "freshness_status".to_string(),
                    json!(metadata.freshness_status),
                );
                obj.insert(
                    "indexing_status".to_string(),
                    json!(metadata.indexing_status),
                );
                obj.insert(
                    "result_completeness".to_string(),
                    json!(metadata.result_completeness),
                );
                obj.insert("ref".to_string(), json!(metadata.r#ref));
                obj.insert("schema_status".to_string(), json!(metadata.schema_status));
            }

            tool_text_response(
                id,
                json!({
                    "query": response.query,
                    "ref": response.ref_name,
                    "mode": response.mode,
                    "budget_tokens": response.budget_tokens,
                    "token_budget_used": response.token_budget_used,
                    "sections": response.sections,
                    "dropped_candidates": response.dropped_candidates,
                    "coverage_summary": response.coverage_summary,
                    "suggested_next_queries": response.suggested_next_queries,
                    "missing_context_hints": response.missing_context_hints,
                    "metadata": merged_metadata,
                }),
            )
        }
        Err(cruxe_query::context_pack::ContextPackError::InvalidQuery) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `query` is required.",
            None,
            metadata,
        ),
        Err(cruxe_query::context_pack::ContextPackError::InvalidBudgetTokens) => {
            tool_error_response(
                id,
                ProtocolErrorCode::InvalidMaxTokens,
                "Parameter `budget_tokens` must be greater than 0.",
                None,
                metadata,
            )
        }
        Err(cruxe_query::context_pack::ContextPackError::InvalidMode) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `mode` must be `full` or `edit_minimal`.",
            None,
            metadata,
        ),
        Err(cruxe_query::context_pack::ContextPackError::State(e)) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}
