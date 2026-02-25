use super::*;

const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Build metadata for request validation errors without triggering filesystem
/// freshness scans. Query handlers call `check_and_enforce_freshness` once
/// after argument validation and before execution.
pub(super) fn validation_metadata(ref_name: &str, schema_status: SchemaStatus) -> ProtocolMetadata {
    match schema_status {
        SchemaStatus::Compatible => ProtocolMetadata::new(ref_name),
        SchemaStatus::NotIndexed => ProtocolMetadata::not_indexed(ref_name),
        SchemaStatus::ReindexRequired => ProtocolMetadata::reindex_required(ref_name),
        SchemaStatus::CorruptManifest => ProtocolMetadata::corrupt_manifest(ref_name),
    }
}

/// Result of freshness check + policy enforcement for query tools.
pub(super) struct FreshnessEnforced {
    pub(super) metadata: ProtocolMetadata,
    /// If the policy requires blocking, this holds the pre-built error response.
    pub(super) block_response: Option<JsonRpcResponse>,
}

/// Check freshness and enforce the configured policy. Returns metadata and an optional
/// block response. When `block_response` is `Some`, the caller must return it immediately.
#[allow(clippy::too_many_arguments)]
pub(super) fn check_and_enforce_freshness(
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
                ProtocolErrorCode::IndexStale,
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
pub(super) fn parse_detail_level(arguments: &Value) -> DetailLevel {
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

pub(super) fn parse_compact(arguments: &Value) -> bool {
    arguments
        .get("compact")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub(super) fn resolve_ranking_explain_level(
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

    let level = config.search.ranking_explain_level_typed();
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

pub(super) fn ranking_reasons_payload(
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

pub(super) fn dedup_search_results(
    results: Vec<search::SearchResult>,
) -> (Vec<search::SearchResult>, Vec<usize>, usize) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(results.len());
    let mut kept_indices = Vec::with_capacity(results.len());
    let mut suppressed = 0usize;
    for (index, result) in results.into_iter().enumerate() {
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
            kept_indices.push(index);
        } else {
            suppressed += 1;
        }
    }
    (deduped, kept_indices, suppressed)
}

pub(super) fn dedup_locate_results(
    results: Vec<locate::LocateResult>,
) -> (Vec<locate::LocateResult>, usize) {
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

pub(super) fn align_ranking_reasons_to_dedup(
    reasons: &[codecompass_core::types::RankingReasons],
    kept_indices: &[usize],
) -> Vec<codecompass_core::types::RankingReasons> {
    let mut aligned = Vec::with_capacity(kept_indices.len());
    for (new_index, old_index) in kept_indices.iter().copied().enumerate() {
        let Some(reason) = reasons.get(old_index) else {
            continue;
        };
        let mut updated = reason.clone();
        updated.result_index = new_index;
        aligned.push(updated);
    }
    aligned
}

pub(super) fn enforce_payload_safety_limit(
    results: Vec<Value>,
    max_bytes: usize,
) -> (Vec<Value>, bool) {
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

pub(super) struct FilteredResultPayload {
    pub(super) filtered: Vec<Value>,
    pub(super) safety_limit_applied: bool,
}

pub(super) fn build_filtered_result_payload(
    mut result_values: Vec<Value>,
    detail_level: DetailLevel,
    compact: bool,
    conn: Option<&rusqlite::Connection>,
    project_id: &str,
    effective_ref: &str,
    max_response_bytes: usize,
) -> FilteredResultPayload {
    if detail_level == DetailLevel::Context && !compact {
        detail::enrich_body_previews(&mut result_values);
        if let Some(c) = conn {
            detail::enrich_results_with_relations(&mut result_values, c, project_id, effective_ref);
        }
    }

    let filtered = detail::serialize_results_at_level(&result_values, detail_level, compact);
    let (filtered, safety_limit_applied) =
        enforce_payload_safety_limit(filtered, max_response_bytes);
    FilteredResultPayload {
        filtered,
        safety_limit_applied,
    }
}

pub(super) fn deterministic_suggested_actions(
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

pub(super) fn deterministic_locate_suggested_actions(
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

pub(super) fn map_state_error(err: &StateError) -> (ProtocolErrorCode, String, Option<Value>) {
    match err {
        StateError::SyncInProgress {
            project_id,
            ref_name,
            job_id,
        } => (
            ProtocolErrorCode::SyncInProgress,
            "A sync job is already active for this project/ref.".to_string(),
            Some(json!({
                "details": format!(
                    "sync_in_progress: project_id={project_id}, ref={ref_name}, job_id={job_id}"
                ),
                "remediation": "Poll index_status and retry sync_repo after the active sync completes.",
            })),
        ),
        StateError::MaintenanceLockBusy {
            operation,
            lock_path,
        } => (
            ProtocolErrorCode::SyncInProgress,
            "Project maintenance is already in progress.".to_string(),
            Some(json!({
                "details": format!(
                    "maintenance_lock_busy: operation={operation}, lock_path={lock_path}"
                ),
                "remediation": "Retry after the active maintenance operation completes.",
            })),
        ),
        StateError::RefNotIndexed {
            project_id,
            ref_name,
        } => (
            ProtocolErrorCode::RefNotIndexed,
            "The requested ref has no indexed state yet.".to_string(),
            Some(json!({
                "details": format!(
                    "ref_not_indexed: project_id={project_id}, ref={ref_name}"
                ),
                "remediation": "Run sync_repo for this ref before querying.",
            })),
        ),
        StateError::OverlayNotReady {
            project_id,
            ref_name,
            reason,
        } => (
            ProtocolErrorCode::OverlayNotReady,
            "The requested ref overlay is not query-ready yet.".to_string(),
            Some(json!({
                "details": format!(
                    "overlay_not_ready: project_id={project_id}, ref={ref_name}, {reason}"
                ),
                "remediation": "Poll index_status until indexing finishes, then retry.",
            })),
        ),
        StateError::MergeBaseFailed {
            base_ref,
            head_ref,
            reason,
        } => (
            ProtocolErrorCode::MergeBaseFailed,
            "Unable to compute merge-base for the requested refs.".to_string(),
            Some(json!({
                "details": format!(
                    "merge_base_failed: base_ref={base_ref}, head_ref={head_ref}, reason={reason}"
                ),
                "remediation": "Validate the refs and repository integrity, then retry.",
            })),
        ),
        StateError::ResultNotFound { path, line_start } => (
            ProtocolErrorCode::ResultNotFound,
            "Requested result target was not found.".to_string(),
            Some(json!({
                "details": format!(
                    "result_not_found: path={path}, line_start={line_start}"
                ),
                "remediation": "Re-run the query and select a valid result from the returned list.",
            })),
        ),
        StateError::SchemaMigrationRequired { current, required } => (
            ProtocolErrorCode::IndexIncompatible,
            "Index schema is incompatible. Run `codecompass index --force`.".to_string(),
            Some(json!({
                "current_schema_version": current,
                "required_schema_version": required,
                "remediation": "codecompass index --force",
            })),
        ),
        StateError::CorruptManifest(details) => (
            ProtocolErrorCode::IndexIncompatible,
            "Index metadata is corrupted. Run `codecompass index --force`.".to_string(),
            Some(json!({
                "details": details,
                "remediation": "codecompass index --force",
            })),
        ),
        other => (
            ProtocolErrorCode::InternalError,
            format!("Tool execution failed: {}", other),
            None,
        ),
    }
}
