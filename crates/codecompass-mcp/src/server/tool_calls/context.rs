use super::*;

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
    let strategy = match codecompass_query::context::parse_strategy(
        arguments.get("strategy").and_then(|v| v.as_str()),
    ) {
        Ok(strategy) => strategy,
        Err(codecompass_query::context::ContextError::InvalidStrategy) => {
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

    match codecompass_query::context::get_code_context(
        codecompass_query::context::GetCodeContextParams {
            index_set,
            conn,
            workspace,
            query,
            ref_name: Some(&effective_ref),
            language,
            max_tokens,
            strategy,
        },
    ) {
        Ok(response) => {
            if response.truncated {
                metadata.result_completeness =
                    codecompass_core::types::ResultCompleteness::Truncated;
            }
            let mut merged_metadata = response.metadata;
            if let Some(obj) = merged_metadata.as_object_mut() {
                obj.insert(
                    "codecompass_protocol_version".to_string(),
                    json!(metadata.codecompass_protocol_version),
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
        Err(codecompass_query::context::ContextError::InvalidMaxTokens) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidMaxTokens,
            "Parameter `max_tokens` must be greater than 0.",
            None,
            metadata,
        ),
        Err(codecompass_query::context::ContextError::State(e)) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
        Err(codecompass_query::context::ContextError::InvalidStrategy) => tool_error_response(
            id,
            ProtocolErrorCode::InvalidStrategy,
            "Parameter `strategy` must be `breadth` or `depth`.",
            None,
            metadata,
        ),
    }
}
