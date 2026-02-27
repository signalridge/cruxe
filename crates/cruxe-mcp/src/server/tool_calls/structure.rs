use super::*;

pub(super) fn handle_get_symbol_hierarchy(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let path = arguments.get("path").and_then(|v| v.as_str());
    let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
    let direction_raw = arguments
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("ancestors");
    let direction = match direction_raw {
        "ancestors" => hierarchy::HierarchyDirection::Ancestors,
        "descendants" => hierarchy::HierarchyDirection::Descendants,
        _ => {
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let metadata = validation_metadata(&effective_ref, schema_status);
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Parameter `direction` must be `ancestors` or `descendants`.",
                None,
                metadata,
            );
        }
    };
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);

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

    if symbol_name.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `symbol_name` is required.",
            None,
            metadata,
        );
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

    match hierarchy::get_symbol_hierarchy(
        c,
        project_id,
        &effective_ref,
        symbol_name,
        path,
        direction,
    ) {
        Ok(response) => tool_text_response(
            id,
            json!({
                "hierarchy": response.hierarchy,
                "direction": response.direction,
                "chain_length": response.chain_length,
                "metadata": metadata,
            }),
        ),
        Err(hierarchy::HierarchyError::SymbolNotFound) => tool_error_response(
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
        Err(hierarchy::HierarchyError::AmbiguousSymbol { count }) => tool_error_response(
            id,
            ProtocolErrorCode::AmbiguousSymbol,
            "Multiple symbols matched. Provide `path` to disambiguate.",
            Some(json!({
                "symbol_name": symbol_name,
                "candidate_count": count,
            })),
            metadata,
        ),
        Err(hierarchy::HierarchyError::State(e)) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_find_related_symbols(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let path = arguments.get("path").and_then(|v| v.as_str());
    let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
    let scope = match arguments
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("file")
    {
        "file" => related::RelatedScope::File,
        "module" => related::RelatedScope::Module,
        "package" => related::RelatedScope::Package,
        _ => {
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let metadata = validation_metadata(&effective_ref, schema_status);
            return tool_error_response(
                id,
                ProtocolErrorCode::InvalidInput,
                "Parameter `scope` must be `file`, `module`, or `package`.",
                None,
                metadata,
            );
        }
    };
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;
    let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);

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

    if symbol_name.trim().is_empty() {
        return tool_error_response(
            id,
            ProtocolErrorCode::InvalidInput,
            "Parameter `symbol_name` is required.",
            None,
            metadata,
        );
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

    match related::find_related_symbols(
        c,
        project_id,
        &effective_ref,
        symbol_name,
        path,
        scope,
        limit,
    ) {
        Ok(response) => tool_text_response(
            id,
            json!({
                "anchor": response.anchor,
                "related": response.related,
                "scope_used": response.scope_used,
                "total_found": response.total_found,
                "metadata": metadata,
            }),
        ),
        Err(related::RelatedError::SymbolNotFound) => tool_error_response(
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
        Err(related::RelatedError::AmbiguousSymbol { count }) => tool_error_response(
            id,
            ProtocolErrorCode::AmbiguousSymbol,
            "Multiple symbols matched. Provide `path` to disambiguate.",
            Some(json!({
                "symbol_name": symbol_name,
                "candidate_count": count,
            })),
            metadata,
        ),
        Err(related::RelatedError::State(e)) => {
            let (code, message, data) = map_state_error(&e);
            tool_error_response(id, code, message, data, metadata)
        }
    }
}

pub(super) fn handle_get_file_outline(params: QueryToolParams<'_>) -> JsonRpcResponse {
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
            ProtocolErrorCode::InvalidInput,
            "Parameter `path` is required.",
            None,
            metadata,
        );
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

    let top_only = depth == "top";
    match cruxe_state::symbols::get_file_outline_query(
        c,
        project_id,
        &effective_ref,
        path,
        top_only,
    ) {
        Ok(flat_symbols) => {
            if flat_symbols.is_empty() {
                let file_exists =
                    cruxe_state::manifest::get_content_hash(c, project_id, &effective_ref, path)
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
                            "cruxe_protocol_version": metadata.cruxe_protocol_version,
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
                    ProtocolErrorCode::FileNotFound,
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
                cruxe_state::symbols::build_symbol_tree(flat_symbols)
            };

            let response = json!({
                "file_path": path,
                "language": language,
                "symbols": symbols,
                "metadata": {
                    "cruxe_protocol_version": metadata.cruxe_protocol_version,
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
