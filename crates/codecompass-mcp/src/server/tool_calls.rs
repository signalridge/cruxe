use super::*;

pub(super) struct ToolCallParams<'a> {
    pub id: Option<Value>,
    pub tool_name: &'a str,
    pub arguments: &'a Value,
    pub config: &'a Config,
    pub index_set: Option<&'a IndexSet>,
    pub schema_status: SchemaStatus,
    pub compatibility_reason: Option<&'a str>,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
    pub prewarm_status: &'a AtomicU8,
    pub server_start: &'a Instant,
    pub notifier: Arc<dyn ProgressNotifier>,
    pub progress_token: Option<String>,
}

const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024;

pub(super) fn handle_tool_call(params: ToolCallParams<'_>) -> JsonRpcResponse {
    // Handle health_check before destructuring since it needs the full params struct
    if params.tool_name == "health_check" {
        return handle_health_check(&params);
    }

    let ToolCallParams {
        id,
        tool_name,
        arguments,
        config,
        index_set,
        schema_status,
        compatibility_reason,
        conn,
        workspace,
        project_id,
        notifier,
        progress_token,
        ..
    } = params;

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
            let compact = parse_compact(arguments);
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

            let ranking_explain_level = match resolve_ranking_explain_level(arguments, config) {
                Ok(level) => level,
                Err(message) => {
                    return tool_error_response(id, "invalid_input", message, None, base_metadata);
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

            // Freshness check
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

                    let mut result_values: Vec<Value> = results
                        .iter()
                        .filter_map(|r| serde_json::to_value(r).ok())
                        .collect();

                    // Enrich with context data if needed
                    if detail_level == DetailLevel::Context && !compact {
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
                    let filtered =
                        detail::serialize_results_at_level(&result_values, detail_level, compact);
                    let (filtered, safety_limit_applied) =
                        enforce_payload_safety_limit(filtered, config.search.max_response_bytes);
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
                        response["suggested_next_actions"] = json!(
                            deterministic_locate_suggested_actions(name, &effective_ref, limit)
                        );
                    }

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
            let compact = parse_compact(arguments);
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

            let ranking_explain_level = match resolve_ranking_explain_level(arguments, config) {
                Ok(level) => level,
                Err(message) => {
                    return tool_error_response(id, "invalid_input", message, None, base_metadata);
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

            // Freshness check
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

            let debug_ranking =
                ranking_explain_level != codecompass_core::types::RankingExplainLevel::Off;
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
                    let (results, suppressed_duplicate_count) =
                        dedup_search_results(std::mem::take(&mut response.results));
                    if suppressed_duplicate_count > 0 {
                        metadata.suppressed_duplicate_count = Some(suppressed_duplicate_count);
                    }

                    let mut result_values: Vec<Value> = results
                        .iter()
                        .filter_map(|r| serde_json::to_value(r).ok())
                        .collect();

                    // Enrich with context data if needed
                    if detail_level == DetailLevel::Context && !compact {
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
                    let filtered =
                        detail::serialize_results_at_level(&result_values, detail_level, compact);
                    let (filtered, safety_limit_applied) =
                        enforce_payload_safety_limit(filtered, config.search.max_response_bytes);
                    if safety_limit_applied {
                        metadata.result_completeness =
                            codecompass_core::types::ResultCompleteness::Truncated;
                        metadata.safety_limit_applied = Some(true);
                    }

                    if let Some(reasons) = &response.ranking_reasons {
                        metadata.ranking_reasons = ranking_reasons_payload(
                            reasons.iter().take(filtered.len()).cloned().collect(),
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
        "get_symbol_hierarchy" => {
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
                    let effective_ref =
                        resolve_tool_ref(requested_ref, workspace, conn, project_id);
                    let metadata = build_metadata(
                        &effective_ref,
                        schema_status,
                        config,
                        conn,
                        workspace,
                        project_id,
                    );
                    return tool_error_response(
                        id,
                        "invalid_input",
                        "Parameter `direction` must be `ancestors` or `descendants`.",
                        None,
                        metadata,
                    );
                }
            };
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);

            // Freshness check
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
                    "invalid_input",
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
                    "symbol_not_found",
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
                    "ambiguous_symbol",
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
        "find_related_symbols" => {
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
                    let effective_ref =
                        resolve_tool_ref(requested_ref, workspace, conn, project_id);
                    let metadata = build_metadata(
                        &effective_ref,
                        schema_status,
                        config,
                        conn,
                        workspace,
                        project_id,
                    );
                    return tool_error_response(
                        id,
                        "invalid_input",
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

            // Freshness check
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
                    "invalid_input",
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
                    "symbol_not_found",
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
                    "ambiguous_symbol",
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
        "get_code_context" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let requested_ref = arguments.get("ref").and_then(|v| v.as_str());
            let language = arguments.get("language").and_then(|v| v.as_str());
            let effective_ref = resolve_tool_ref(requested_ref, workspace, conn, project_id);
            let mut metadata = build_metadata(
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
                                "invalid_max_tokens",
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
                                    "invalid_max_tokens",
                                    "Parameter `max_tokens` is too large.",
                                    None,
                                    metadata,
                                );
                            }
                        }
                    } else {
                        return tool_error_response(
                            id,
                            "invalid_max_tokens",
                            "Parameter `max_tokens` must be a positive integer.",
                            None,
                            metadata,
                        );
                    }
                }
            };
            let strategy =
                match context::parse_strategy(arguments.get("strategy").and_then(|v| v.as_str())) {
                    Ok(strategy) => strategy,
                    Err(context::ContextError::InvalidStrategy) => {
                        return tool_error_response(
                            id,
                            "invalid_strategy",
                            "Parameter `strategy` must be `breadth` or `depth`.",
                            None,
                            metadata,
                        );
                    }
                    Err(_) => {
                        return tool_error_response(
                            id,
                            "invalid_input",
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

            // Freshness check
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

            match context::get_code_context(context::GetCodeContextParams {
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
                Err(context::ContextError::InvalidMaxTokens) => tool_error_response(
                    id,
                    "invalid_max_tokens",
                    "Parameter `max_tokens` must be greater than 0.",
                    None,
                    metadata,
                ),
                Err(context::ContextError::State(e)) => {
                    let (code, message, data) = map_state_error(&e);
                    tool_error_response(id, code, message, data, metadata)
                }
                Err(context::ContextError::InvalidStrategy) => tool_error_response(
                    id,
                    "invalid_strategy",
                    "Parameter `strategy` must be `breadth` or `depth`.",
                    None,
                    metadata,
                ),
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

            // T455: Include interrupted recovery report if any jobs were marked interrupted
            let interrupted_jobs = conn
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
                "active_job": active_job.map(|j| {
                    let total = j.files_scanned.max(1);
                    let pct = if j.files_scanned > 0 {
                        Some(((j.files_indexed as f64 / total as f64) * 100.0).min(99.0) as u32)
                    } else {
                        None
                    };
                    json!({
                        "job_id": j.job_id,
                        "mode": j.mode,
                        "status": j.status,
                        "ref": j.r#ref,
                        "progress_token": j.progress_token,
                        "files_scanned": j.files_scanned,
                        "files_indexed": j.files_indexed,
                        "symbols_extracted": j.symbols_extracted,
                        "estimated_completion_pct": pct,
                    })
                }),
                "recent_jobs": recent_jobs.iter().map(|j| json!({
                    "job_id": j.job_id,
                    "ref": j.r#ref,
                    "mode": j.mode,
                    "status": j.status,
                    "changed_files": j.changed_files,
                    "duration_ms": j.duration_ms,
                    "created_at": j.created_at,
                })).collect::<Vec<_>>(),
                "interrupted_recovery_report": interrupted_recovery_report,
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
            // HIGH-2: Server-generated progress_token per spec T216
            let server_progress_token = format!("index-job-{}", job_id);
            // Use client-provided token for notifications if available, otherwise server-generated
            let effective_progress_token = progress_token
                .clone()
                .filter(|token| !token.trim().is_empty())
                .unwrap_or_else(|| server_progress_token.clone());
            let notifications_enabled = progress_token.is_some();

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
                    // T216: Emit begin notification and start progress polling
                    if notifications_enabled {
                        notifier.emit_begin(
                            &effective_progress_token,
                            "Indexing",
                            "Starting indexer...",
                        );
                    }

                    // T215: Background thread that polls progress and emits notifications
                    let notifier_clone = Arc::clone(&notifier);
                    let poll_token = if notifications_enabled {
                        Some(effective_progress_token.clone())
                    } else {
                        None
                    };
                    let poll_db_path = config
                        .project_data_dir(project_id)
                        .join(constants::STATE_DB_FILE);
                    let poll_project_id = project_id.to_string();
                    let notification_start = std::time::Instant::now();
                    std::thread::spawn(move || {
                        let mut child = child;
                        let mut last_scanned = 0i64;
                        let mut last_indexed = 0i64;
                        let mut last_symbols = 0i64;
                        let mut poll_conn =
                            codecompass_state::db::open_connection(&poll_db_path).ok();
                        let mut sleep_ms = 1000u64;

                        loop {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    // Child exited — emit end notification with summary
                                    if let Some(ref token) = poll_token {
                                        let summary = if status.success() {
                                            format!(
                                                "Indexed {} files, {} symbols in {:.1}s",
                                                last_indexed,
                                                last_symbols,
                                                notification_start.elapsed().as_secs_f64(),
                                            )
                                        } else {
                                            format!(
                                                "Error: Indexer exited with code {}",
                                                status.code().unwrap_or(-1)
                                            )
                                        };
                                        let title = if status.success() {
                                            "Indexing complete"
                                        } else {
                                            "Indexing failed"
                                        };
                                        notifier_clone.emit_end(token, title, &summary);
                                    }
                                    break;
                                }
                                Ok(None) => {
                                    // Still running — poll progress from SQLite
                                    if poll_conn.is_none() {
                                        poll_conn =
                                            codecompass_state::db::open_connection(&poll_db_path)
                                                .ok();
                                    }

                                    let mut progress_changed = false;
                                    if let Some(ref token) = poll_token
                                        && let Some(conn) = poll_conn.as_ref()
                                    {
                                        match codecompass_state::jobs::get_active_job(
                                            conn,
                                            &poll_project_id,
                                        ) {
                                            Ok(Some(job))
                                                if job.files_scanned != last_scanned
                                                    || job.files_indexed != last_indexed
                                                    || job.symbols_extracted != last_symbols =>
                                            {
                                                last_scanned = job.files_scanned;
                                                last_indexed = job.files_indexed;
                                                last_symbols = job.symbols_extracted;
                                                progress_changed = true;

                                                let total = last_scanned.max(1);
                                                let pct = ((last_indexed as f64 / total as f64)
                                                    * 100.0)
                                                    .min(99.0)
                                                    as u32;
                                                let (msg, stage_pct) = if last_scanned == 0 {
                                                    ("Scanning files: 0 discovered".to_string(), 0)
                                                } else if last_indexed == 0 {
                                                    (
                                                        format!(
                                                            "Scanning files: {} discovered",
                                                            last_scanned
                                                        ),
                                                        10,
                                                    )
                                                } else if pct < 70 {
                                                    (
                                                        format!(
                                                            "Parsing files: {}/{} ({}%)",
                                                            last_indexed, last_scanned, pct
                                                        ),
                                                        pct.max(10),
                                                    )
                                                } else if pct < 95 {
                                                    (
                                                        format!(
                                                            "Indexing: {}/{} files, {} symbols",
                                                            last_indexed,
                                                            last_scanned,
                                                            last_symbols
                                                        ),
                                                        pct,
                                                    )
                                                } else {
                                                    ("Finalizing index...".to_string(), pct.max(95))
                                                };
                                                notifier_clone.emit_progress(
                                                    token,
                                                    "Indexing",
                                                    &msg,
                                                    Some(stage_pct.min(99)),
                                                );
                                            }
                                            Ok(_) => {}
                                            Err(_) => {
                                                poll_conn = None;
                                            }
                                        }
                                    }
                                    sleep_ms = if progress_changed {
                                        500
                                    } else {
                                        (sleep_ms + 250).min(2000)
                                    };
                                    std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                                }
                                Err(err) => {
                                    if let Some(ref token) = poll_token {
                                        notifier_clone.emit_end(
                                            token,
                                            "Indexing failed",
                                            &format!(
                                                "Error: Failed to poll indexer process: {}",
                                                err
                                            ),
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    });

                    let mut payload = serde_json::Map::new();
                    payload.insert("job_id".to_string(), json!(job_id));
                    payload.insert("status".to_string(), json!("running"));
                    payload.insert("mode".to_string(), json!(mode));
                    // Always include server-generated progress_token (per spec T216)
                    payload.insert("progress_token".to_string(), json!(server_progress_token));
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

fn handle_health_check(params: &ToolCallParams<'_>) -> JsonRpcResponse {
    let ToolCallParams {
        id,
        arguments,
        config,
        index_set,
        schema_status,
        conn,
        workspace,
        project_id,
        prewarm_status,
        server_start,
        ..
    } = params;

    let requested_workspace = arguments.get("workspace").and_then(|v| v.as_str());
    let effective_ref = resolve_tool_ref(None, workspace, *conn, project_id);
    let metadata = build_metadata(
        &effective_ref,
        *schema_status,
        config,
        *conn,
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
                        id.clone(),
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
    let warmset_capacity = crate::server::warmset_capacity();
    let warmset_members =
        crate::server::collect_warmset_members(*conn, workspace, warmset_capacity);
    let warmset_enabled = pw_status != PREWARM_SKIPPED;

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

    // Grammar availability
    let supported = codecompass_indexer::parser::supported_languages();
    let mut grammars_available = Vec::new();
    let mut grammars_missing = Vec::new();
    for lang in &supported {
        match codecompass_indexer::parser::get_language(lang) {
            Ok(_) => grammars_available.push(*lang),
            Err(_) => grammars_missing.push(*lang),
        }
    }

    // Per-project status
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
            let project_schema_status =
                resolve_project_schema_status(config, project_id, &p.project_id, *schema_status);
            let project_schema_status_str = match project_schema_status {
                SchemaStatus::Compatible => "compatible",
                SchemaStatus::NotIndexed => "not_indexed",
                SchemaStatus::ReindexRequired => "reindex_required",
                SchemaStatus::CorruptManifest => "corrupt_manifest",
            };
            let project_current_schema_version = match project_schema_status {
                SchemaStatus::Compatible => constants::SCHEMA_VERSION,
                _ => p.schema_version,
            };
            let freshness_result = check_freshness_with_scan_params(
                Some(c),
                project_workspace,
                &p.project_id,
                &project_ref,
                config.index.max_file_size,
                Some(&config.index.languages),
            );
            let proj_freshness_status = freshness::freshness_status(&freshness_result);

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

            let has_schema_error = !matches!(project_schema_status, SchemaStatus::Compatible);
            let prewarm_failed_for_project =
                p.project_id == *project_id && pw_status == PREWARM_FAILED;
            let (index_status, warming) = if has_schema_error || prewarm_failed_for_project {
                ("error", false)
            } else if p.project_id == *project_id && pw_status == PREWARM_IN_PROGRESS {
                ("warming", true)
            } else if active_job.is_some() {
                ("indexing", false)
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
            let last_indexed_at = codecompass_state::jobs::get_recent_jobs(c, &p.project_id, 5)
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
                "freshness_status": proj_freshness_status,
                "last_indexed_at": last_indexed_at,
                "ref": project_ref,
                "file_count": file_count,
                "symbol_count": symbol_count,
                "schema_status": project_schema_status_str,
                "current_schema_version": project_current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
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
            let fallback_schema_status = match schema_status {
                SchemaStatus::Compatible => "compatible",
                SchemaStatus::NotIndexed => "not_indexed",
                SchemaStatus::ReindexRequired => "reindex_required",
                SchemaStatus::CorruptManifest => "corrupt_manifest",
            };
            let fallback_current_schema_version = match schema_status {
                SchemaStatus::Compatible => constants::SCHEMA_VERSION,
                _ => codecompass_state::project::get_by_id(c, project_id)
                    .ok()
                    .flatten()
                    .map(|p| p.schema_version)
                    .unwrap_or(0),
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
                "schema_status": fallback_schema_status,
                "current_schema_version": fallback_current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
            }));
        }
    } else {
        let fallback_schema_status = match schema_status {
            SchemaStatus::Compatible => "compatible",
            SchemaStatus::NotIndexed => "not_indexed",
            SchemaStatus::ReindexRequired => "reindex_required",
            SchemaStatus::CorruptManifest => "corrupt_manifest",
        };
        let fallback_current_schema_version = if matches!(schema_status, SchemaStatus::Compatible) {
            constants::SCHEMA_VERSION
        } else {
            0
        };
        project_payloads.push(json!({
            "project_id": project_id,
            "repo_root": workspace.to_string_lossy(),
            "index_status": "error",
            "freshness_status": metadata.freshness_status,
            "last_indexed_at": Value::Null,
            "ref": effective_ref,
            "file_count": 0,
            "symbol_count": 0,
            "schema_status": fallback_schema_status,
            "current_schema_version": fallback_current_schema_version,
            "required_schema_version": constants::SCHEMA_VERSION,
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
    // Status priority: error > warming > indexing > ready (per spec)
    let overall_status = if any_error_project {
        "error"
    } else if any_warming_project {
        "warming"
    } else if overall_has_active_job {
        "indexing"
    } else {
        "ready"
    };

    // CRITICAL-4: Add interrupted_recovery_report to health_check (FR-327)
    let interrupted_jobs = conn
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
        "interrupted_recovery_report": interrupted_recovery_report,
        "startup_checks": {
            "index": {
                "status": index_compat_status,
                "current_schema_version": current_schema_version,
                "required_schema_version": constants::SCHEMA_VERSION,
                "message": compat_message,
            }
        },
        "workspace_warmset": {
            "enabled": warmset_enabled,
            "capacity": warmset_capacity,
            "members": if warmset_enabled { warmset_members } else { Vec::<String>::new() },
        },
        "projects": project_payloads,
        "metadata": metadata,
    });
    tool_text_response(id.clone(), result)
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

struct ToolCompatibilityParams<'a> {
    id: Option<Value>,
    schema_status: SchemaStatus,
    compatibility_reason: Option<&'a str>,
    config: &'a Config,
    conn: Option<&'a rusqlite::Connection>,
    workspace: &'a Path,
    project_id: &'a str,
    ref_name: &'a str,
}

fn tool_compatibility_error(params: ToolCompatibilityParams<'_>) -> JsonRpcResponse {
    let ToolCompatibilityParams {
        id,
        schema_status,
        compatibility_reason,
        config,
        conn,
        workspace,
        project_id,
        ref_name,
    } = params;

    let metadata = build_metadata(ref_name, schema_status, config, conn, workspace, project_id);
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
pub(crate) fn tool_text_response(id: Option<Value>, payload: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "content": [{"type": "text", "text": serde_json::to_string(&payload).unwrap_or_default()}]
        }),
    )
}

/// Result of freshness check + policy enforcement for query tools.
struct FreshnessEnforced {
    metadata: ProtocolMetadata,
    /// If the policy requires blocking, this holds the pre-built error response.
    block_response: Option<JsonRpcResponse>,
}

/// Check freshness and enforce the configured policy. Returns metadata and an optional
/// block response. When `block_response` is `Some`, the caller must return it immediately.
#[allow(clippy::too_many_arguments)]
fn check_and_enforce_freshness(
    id: Option<Value>,
    arguments: &Value,
    config: &Config,
    conn: Option<&rusqlite::Connection>,
    workspace: &Path,
    project_id: &str,
    effective_ref: &str,
    schema_status: SchemaStatus,
) -> FreshnessEnforced {
    let policy = resolve_freshness_policy(arguments, config);
    let freshness_result = check_freshness_with_scan_params(
        conn,
        workspace,
        project_id,
        effective_ref,
        config.index.max_file_size,
        Some(&config.index.languages),
    );
    let policy_action = apply_freshness_policy(policy, &freshness_result);
    let metadata = build_metadata_with_freshness(effective_ref, schema_status, &freshness_result);

    if let PolicyAction::BlockWithError {
        last_indexed_commit,
        current_head,
    } = &policy_action
    {
        return FreshnessEnforced {
            block_response: Some(tool_error_response(
                id,
                "index_stale",
                "Index is stale and freshness_policy is strict. Sync before querying.",
                Some(json!({
                    "last_indexed_commit": last_indexed_commit,
                    "current_head": current_head,
                    "suggestion": "Call sync_repo to update the index before querying.",
                })),
                metadata,
            )),
            metadata: ProtocolMetadata::new(effective_ref), // unused when blocking
        };
    }
    if policy_action == PolicyAction::ProceedWithStaleIndicatorAndSync {
        trigger_async_sync(workspace, effective_ref);
    }

    FreshnessEnforced {
        metadata,
        block_response: None,
    }
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

fn parse_compact(arguments: &Value) -> bool {
    arguments
        .get("compact")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn resolve_ranking_explain_level(
    arguments: &Value,
    config: &Config,
) -> Result<codecompass_core::types::RankingExplainLevel, String> {
    if let Some(raw) = arguments
        .get("ranking_explain_level")
        .and_then(|v| v.as_str())
    {
        return parse_ranking_explain_level(raw).ok_or_else(|| {
            "Parameter `ranking_explain_level` must be `off`, `basic`, or `full`.".to_string()
        });
    }

    let level = parse_ranking_explain_level(&config.search.ranking_explain_level)
        .unwrap_or(codecompass_core::types::RankingExplainLevel::Off);
    // Config::load_with_file already promotes legacy `debug.ranking_reasons` into
    // `search.ranking_explain_level`. Keep this runtime fallback for compatibility
    // with direct/manual Config construction paths (e.g., focused tests).
    if level == codecompass_core::types::RankingExplainLevel::Off && config.debug.ranking_reasons {
        return Ok(codecompass_core::types::RankingExplainLevel::Full);
    }
    Ok(level)
}

fn parse_ranking_explain_level(raw: &str) -> Option<codecompass_core::types::RankingExplainLevel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(codecompass_core::types::RankingExplainLevel::Off),
        "basic" => Some(codecompass_core::types::RankingExplainLevel::Basic),
        "full" => Some(codecompass_core::types::RankingExplainLevel::Full),
        _ => None,
    }
}

fn ranking_reasons_payload(
    reasons: Vec<codecompass_core::types::RankingReasons>,
    level: codecompass_core::types::RankingExplainLevel,
) -> Option<Value> {
    match level {
        codecompass_core::types::RankingExplainLevel::Off => None,
        codecompass_core::types::RankingExplainLevel::Full => serde_json::to_value(reasons).ok(),
        codecompass_core::types::RankingExplainLevel::Basic => {
            serde_json::to_value(ranking::to_basic_ranking_reasons(&reasons)).ok()
        }
    }
}

fn dedup_search_results(results: Vec<search::SearchResult>) -> (Vec<search::SearchResult>, usize) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(results.len());
    let mut suppressed = 0usize;
    for result in results {
        let key =
            if let Some(stable_id) = result.symbol_stable_id.as_ref().filter(|s| !s.is_empty()) {
                format!("stable:{}", stable_id)
            } else {
                format!(
                    "{}:{}:{}:{}:{}",
                    result.result_type,
                    result.path,
                    result.line_start,
                    result.line_end,
                    result.name.as_deref().unwrap_or("")
                )
            };
        if seen.insert(key) {
            deduped.push(result);
        } else {
            suppressed += 1;
        }
    }
    (deduped, suppressed)
}

fn dedup_locate_results(results: Vec<locate::LocateResult>) -> (Vec<locate::LocateResult>, usize) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(results.len());
    let mut suppressed = 0usize;
    for result in results {
        let key = if result.symbol_stable_id.is_empty() {
            format!(
                "{}:{}:{}:{}",
                result.path, result.line_start, result.line_end, result.name
            )
        } else {
            format!("stable:{}", result.symbol_stable_id)
        };
        if seen.insert(key) {
            deduped.push(result);
        } else {
            suppressed += 1;
        }
    }
    (deduped, suppressed)
}

fn enforce_payload_safety_limit(results: Vec<Value>, max_bytes: usize) -> (Vec<Value>, bool) {
    let max_bytes = if max_bytes == 0 {
        DEFAULT_MAX_RESPONSE_BYTES
    } else {
        max_bytes
    };

    let mut output = Vec::new();
    let mut used = 2usize; // '[' + ']'
    let mut truncated = false;
    for item in results {
        let item_size = serde_json::to_vec(&item).map(|v| v.len()).unwrap_or(0);
        let separator = usize::from(!output.is_empty());
        if used + separator + item_size > max_bytes {
            truncated = true;
            break;
        }
        used += separator + item_size;
        output.push(item);
    }

    if output.is_empty() && truncated {
        // Under extremely small byte limits, even the first item may not fit.
        // Returning [] with `truncated=true` keeps behavior deterministic while
        // signaling callers to follow `suggested_next_actions`.
        return (Vec::new(), true);
    }
    (output, truncated)
}

fn deterministic_suggested_actions(
    existing: &[search::SuggestedAction],
    query: &str,
    effective_ref: &str,
    limit: usize,
) -> Vec<search::SuggestedAction> {
    if !existing.is_empty() {
        return existing.to_vec();
    }
    vec![search::SuggestedAction {
        tool: "search_code".to_string(),
        name: None,
        query: Some(query.to_string()),
        r#ref: Some(effective_ref.to_string()),
        limit: Some(limit.max(1) / 2 + 1),
    }]
}

fn deterministic_locate_suggested_actions(
    name: &str,
    effective_ref: &str,
    limit: usize,
) -> Vec<search::SuggestedAction> {
    vec![
        search::SuggestedAction {
            tool: "locate_symbol".to_string(),
            name: Some(name.to_string()),
            query: None,
            r#ref: Some(effective_ref.to_string()),
            limit: Some((limit / 2).max(1)),
        },
        search::SuggestedAction {
            tool: "search_code".to_string(),
            name: None,
            query: Some(name.to_string()),
            r#ref: Some(effective_ref.to_string()),
            limit: Some(5),
        },
    ]
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

    #[test]
    fn dedup_search_results_by_stable_id() {
        let base = search::SearchResult {
            result_id: "r1".to_string(),
            symbol_id: Some("sym1".to_string()),
            symbol_stable_id: Some("stable1".to_string()),
            result_type: "symbol".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            kind: Some("fn".to_string()),
            name: Some("foo".to_string()),
            qualified_name: Some("foo".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score: 1.0,
            snippet: None,
        };
        let mut second = base.clone();
        second.result_id = "r2".to_string();
        let third = search::SearchResult {
            result_id: "r3".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "file".to_string(),
            path: "src/other.rs".to_string(),
            line_start: 1,
            line_end: 1,
            kind: None,
            name: None,
            qualified_name: None,
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score: 0.5,
            snippet: None,
        };

        let (deduped, suppressed) = dedup_search_results(vec![base, second, third]);
        assert_eq!(suppressed, 1);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn dedup_locate_results_by_stable_id() {
        let a = locate::LocateResult {
            symbol_id: "s1".to_string(),
            symbol_stable_id: "stable".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            kind: "fn".to_string(),
            name: "foo".to_string(),
            qualified_name: "foo".to_string(),
            signature: None,
            language: "rust".to_string(),
            visibility: None,
            score: 1.0,
        };
        let mut b = a.clone();
        b.symbol_id = "s2".to_string();
        let (deduped, suppressed) = dedup_locate_results(vec![a, b]);
        assert_eq!(suppressed, 1);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn safety_limit_truncates_results() {
        let results = vec![
            json!({"name": "a", "payload": "x".repeat(80)}),
            json!({"name": "b", "payload": "y".repeat(80)}),
            json!({"name": "c", "payload": "z".repeat(80)}),
        ];
        let (trimmed, truncated) = enforce_payload_safety_limit(results, 120);
        assert!(truncated);
        assert!(trimmed.len() < 3);
    }

    #[test]
    fn ranking_payload_basic_uses_compact_fields() {
        let reasons = vec![codecompass_core::types::RankingReasons {
            result_index: 0,
            exact_match_boost: 5.0,
            qualified_name_boost: 2.0,
            path_affinity: 1.0,
            definition_boost: 1.0,
            kind_match: 0.0,
            bm25_score: 10.0,
            final_score: 19.0,
        }];

        let payload =
            ranking_reasons_payload(reasons, codecompass_core::types::RankingExplainLevel::Basic)
                .unwrap();
        let first = payload.as_array().unwrap().first().unwrap();
        assert!(first.get("exact_match").is_some());
        assert!(first.get("path_boost").is_some());
        assert!(first.get("semantic_similarity").is_some());
        assert!(first.get("qualified_name_boost").is_none());
    }

    #[test]
    fn deterministic_locate_actions_are_stable() {
        let actions = deterministic_locate_suggested_actions("validate_token", "main", 10);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].tool, "locate_symbol");
        assert_eq!(actions[0].name.as_deref(), Some("validate_token"));
        assert_eq!(actions[0].limit, Some(5));
        assert_eq!(actions[1].tool, "search_code");
        assert_eq!(actions[1].query.as_deref(), Some("validate_token"));
        assert_eq!(actions[1].limit, Some(5));
    }
}
