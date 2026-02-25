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

pub(super) struct QueryToolParams<'a> {
    pub id: Option<Value>,
    pub arguments: &'a Value,
    pub config: &'a Config,
    pub index_set: Option<&'a IndexSet>,
    pub schema_status: SchemaStatus,
    pub compatibility_reason: Option<&'a str>,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
}

pub(super) struct IndexStatusToolParams<'a> {
    pub id: Option<Value>,
    pub arguments: &'a Value,
    pub config: &'a Config,
    pub schema_status: SchemaStatus,
    pub compatibility_reason: Option<&'a str>,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
}

pub(super) struct IndexOperationParams<'a> {
    pub id: Option<Value>,
    pub tool_name: &'a str,
    pub arguments: &'a Value,
    pub config: &'a Config,
    pub schema_status: SchemaStatus,
    pub conn: Option<&'a rusqlite::Connection>,
    pub workspace: &'a Path,
    pub project_id: &'a str,
    pub notifier: Arc<dyn ProgressNotifier>,
    pub progress_token: Option<String>,
}

mod context;
mod health;
mod index;
mod query;
mod refs;
mod shared;
mod status;
mod structure;
use shared::*;

pub(super) fn handle_tool_call(params: ToolCallParams<'_>) -> JsonRpcResponse {
    if params.tool_name == "health_check" {
        return health::handle_health_check(&params);
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
        "locate_symbol" => query::handle_locate_symbol(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "search_code" => query::handle_search_code(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "get_symbol_hierarchy" => structure::handle_get_symbol_hierarchy(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "find_related_symbols" => structure::handle_find_related_symbols(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "get_code_context" => context::handle_get_code_context(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "diff_context" => query::handle_diff_context(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "find_references" => query::handle_find_references(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "explain_ranking" => query::handle_explain_ranking(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "list_refs" => refs::handle_list_refs(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "switch_ref" => refs::handle_switch_ref(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "get_file_outline" => structure::handle_get_file_outline(QueryToolParams {
            id,
            arguments,
            config,
            index_set,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "index_status" => status::handle_index_status(IndexStatusToolParams {
            id,
            arguments,
            config,
            schema_status,
            compatibility_reason,
            conn,
            workspace,
            project_id,
        }),
        "index_repo" | "sync_repo" => index::handle_index_operation(IndexOperationParams {
            id,
            tool_name,
            arguments,
            config,
            schema_status,
            conn,
            workspace,
            project_id,
            notifier,
            progress_token,
        }),
        _ => JsonRpcResponse::error(id, -32601, format!("Unknown tool: {}", tool_name)),
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
            ProtocolErrorCode::ProjectNotFound,
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
        ProtocolErrorCode::IndexIncompatible,
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
    code: ProtocolErrorCode,
    message: impl Into<String>,
    data: Option<Value>,
    metadata: ProtocolMetadata,
) -> JsonRpcResponse {
    let mut error_obj = serde_json::Map::new();
    error_obj.insert("code".to_string(), Value::String(code.as_str().to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_search_results_by_stable_id() {
        let base = search::SearchResult {
            repo: "repo-1".to_string(),
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
            chunk_type: None,
            source_layer: None,
        };
        let mut second = base.clone();
        second.result_id = "r2".to_string();
        let third = search::SearchResult {
            repo: "repo-1".to_string(),
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
            chunk_type: None,
            source_layer: None,
        };

        let (deduped, kept_indices, suppressed) = dedup_search_results(vec![base, second, third]);
        assert_eq!(suppressed, 1);
        assert_eq!(deduped.len(), 2);
        assert_eq!(kept_indices, vec![0, 2]);
    }

    #[test]
    fn dedup_locate_results_by_stable_id() {
        let a = locate::LocateResult {
            repo: "repo-1".to_string(),
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
            source_layer: None,
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
    fn filtered_payload_compact_context_omits_heavy_fields() {
        let results = vec![json!({
            "symbol_id": "sym_1",
            "symbol_stable_id": "stable_1",
            "result_id": "res_1",
            "result_type": "symbol",
            "path": "src/lib.rs",
            "line_start": 10,
            "line_end": 20,
            "kind": "function",
            "name": "validate_token",
            "snippet": "fn validate_token() { /* body */ }",
            "body_preview": "preview",
        })];

        let payload = build_filtered_result_payload(
            results,
            DetailLevel::Context,
            true,
            None,
            "proj_1",
            "main",
            4096,
        );

        assert!(!payload.safety_limit_applied);
        let first = payload
            .filtered
            .first()
            .and_then(|v| v.as_object())
            .unwrap();
        assert!(first.get("snippet").is_none());
        assert!(first.get("body_preview").is_none());
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
