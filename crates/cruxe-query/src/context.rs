use crate::search;
use crate::{policy::PolicyRuntime, search::SearchExecutionOptions};
use cruxe_core::config::SearchConfig as CoreSearchConfig;
use cruxe_core::error::StateError;
use cruxe_core::tokens::estimate_tokens;
use cruxe_core::types::PolicyMode;
use cruxe_state::tantivy_index::IndexSet;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStrategy {
    Breadth,
    Depth,
}

impl ContextStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Breadth => "breadth",
            Self::Depth => "depth",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeContextResponse {
    pub context_items: Vec<serde_json::Value>,
    pub estimated_tokens: usize,
    pub truncated: bool,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("invalid strategy")]
    InvalidStrategy,
    #[error("invalid max_tokens")]
    InvalidMaxTokens,
    #[error("state error: {0}")]
    State(#[from] StateError),
}

pub fn parse_strategy(value: Option<&str>) -> Result<ContextStrategy, ContextError> {
    match value.unwrap_or("breadth") {
        "breadth" => Ok(ContextStrategy::Breadth),
        "depth" => Ok(ContextStrategy::Depth),
        _ => Err(ContextError::InvalidStrategy),
    }
}

pub struct GetCodeContextParams<'a> {
    pub index_set: &'a IndexSet,
    pub conn: Option<&'a Connection>,
    pub search_config: &'a CoreSearchConfig,
    pub workspace: &'a Path,
    pub query: &'a str,
    pub ref_name: Option<&'a str>,
    pub language: Option<&'a str>,
    pub max_tokens: usize,
    pub strategy: ContextStrategy,
    pub policy_mode_override: Option<PolicyMode>,
}

pub fn get_code_context(
    params: GetCodeContextParams<'_>,
) -> Result<CodeContextResponse, ContextError> {
    let GetCodeContextParams {
        index_set,
        conn,
        search_config,
        workspace,
        query,
        ref_name,
        language,
        max_tokens,
        strategy,
        policy_mode_override,
    } = params;

    if max_tokens == 0 {
        return Err(ContextError::InvalidMaxTokens);
    }

    let policy_runtime = PolicyRuntime::from_search_config(search_config, policy_mode_override)?;
    let mut search_response = search::search_code_with_options(
        index_set,
        conn,
        query,
        ref_name,
        language,
        50,
        false,
        SearchExecutionOptions {
            search_config: search_config.clone(),
            semantic_ratio_override: None,
            confidence_threshold_override: None,
            role: None,
            policy_mode_override,
            policy_runtime: Some(policy_runtime.clone()),
        },
    )?;
    let total_candidates = search_response.results.len();
    let mut items = Vec::new();
    let mut estimated = 0usize;
    let mut truncated = false;
    let mut body_redacted_count = 0usize;
    let mut body_redaction_categories = BTreeMap::new();

    for result in search_response.results {
        let item = match strategy {
            ContextStrategy::Breadth => json!({
                "symbol_id": result.symbol_id,
                "symbol_stable_id": result.symbol_stable_id,
                "name": result.name,
                "kind": result.kind,
                "qualified_name": result.qualified_name,
                "path": result.path,
                "line_start": result.line_start,
                "line_end": result.line_end,
                "signature": result.signature,
                "language": result.language,
                "score": result.score,
            }),
            ContextStrategy::Depth => {
                let original_body = load_symbol_body(
                    workspace,
                    &result.path,
                    result.line_start,
                    result.line_end,
                    result.snippet.as_deref(),
                );
                let body_redaction = policy_runtime.redact_text(&original_body);
                if body_redaction.redacted_count > 0 {
                    body_redacted_count += body_redaction.redacted_count;
                    for (category, count) in body_redaction.category_counts {
                        *body_redaction_categories.entry(category).or_insert(0) += count;
                    }
                }
                let body = if policy_runtime.mode() == PolicyMode::AuditOnly {
                    original_body
                } else {
                    body_redaction.text
                };
                json!({
                    "symbol_id": result.symbol_id,
                    "symbol_stable_id": result.symbol_stable_id,
                    "name": result.name,
                    "kind": result.kind,
                    "qualified_name": result.qualified_name,
                    "path": result.path,
                    "line_start": result.line_start,
                    "line_end": result.line_end,
                    "signature": result.signature,
                    "language": result.language,
                    "score": result.score,
                    "body": body,
                })
            }
        };

        let item_text = serde_json::to_string(&item).unwrap_or_default();
        let item_tokens = estimate_tokens(&item_text);
        if estimated + item_tokens > max_tokens {
            truncated = true;
            break;
        }

        estimated += item_tokens;
        items.push(item);
    }

    let remaining = total_candidates.saturating_sub(items.len());
    search_response.metadata.policy_redacted_count += body_redacted_count;
    for (category, count) in body_redaction_categories {
        *search_response
            .metadata
            .policy_audit_counts
            .entry(format!("redacted:{category}"))
            .or_insert(0) += count;
    }

    let metadata = if truncated {
        json!({
            "total_candidates": total_candidates,
            "returned": items.len(),
            "remaining_candidates": remaining,
            "strategy": strategy.as_str(),
            "policy_mode": search_response.metadata.policy_mode,
            "policy_blocked_count": search_response.metadata.policy_blocked_count,
            "policy_redacted_count": search_response.metadata.policy_redacted_count,
            "policy_warnings": search_response.metadata.policy_warnings,
            "policy_audit_counts": search_response.metadata.policy_audit_counts,
            "policy_redaction_categories": search_response.metadata.policy_redaction_categories,
            "suggestion": "Use locate_symbol for specific symbols, or increase max_tokens",
        })
    } else {
        json!({
            "total_candidates": total_candidates,
            "returned": items.len(),
            "strategy": strategy.as_str(),
            "policy_mode": search_response.metadata.policy_mode,
            "policy_blocked_count": search_response.metadata.policy_blocked_count,
            "policy_redacted_count": search_response.metadata.policy_redacted_count,
            "policy_warnings": search_response.metadata.policy_warnings,
            "policy_audit_counts": search_response.metadata.policy_audit_counts,
            "policy_redaction_categories": search_response.metadata.policy_redaction_categories,
        })
    };

    Ok(CodeContextResponse {
        context_items: items,
        estimated_tokens: estimated,
        truncated,
        metadata,
    })
}

fn load_symbol_body(
    workspace: &Path,
    relative_path: &str,
    line_start: u32,
    line_end: u32,
    fallback: Option<&str>,
) -> String {
    if line_start == 0 || line_end == 0 || line_end < line_start {
        return fallback.unwrap_or("").to_string();
    }

    let full_path = workspace.join(relative_path);
    let Ok(content) = std::fs::read_to_string(full_path) else {
        return fallback.unwrap_or("").to_string();
    };
    let lines = content.lines().collect::<Vec<_>>();
    let start = (line_start.saturating_sub(1) as usize).min(lines.len());
    let end = (line_end as usize).min(lines.len());
    if start >= end {
        return fallback.unwrap_or("").to_string();
    }
    lines[start..end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strategy_defaults_to_breadth() {
        assert_eq!(parse_strategy(None).unwrap(), ContextStrategy::Breadth);
    }

    #[test]
    fn parse_strategy_rejects_invalid_values() {
        let err = parse_strategy(Some("invalid")).unwrap_err();
        assert!(matches!(err, ContextError::InvalidStrategy));
    }

    #[test]
    fn load_symbol_body_uses_fallback_when_file_missing() {
        let workspace = std::path::Path::new("/tmp/non-existent-workspace");
        let body = load_symbol_body(workspace, "missing.rs", 1, 2, Some("fallback"));
        assert_eq!(body, "fallback");
    }

    #[test]
    fn token_estimation_consistency_matches_formula() {
        let serialized = r#"{"name":"validate_token","kind":"function"}"#;
        let words = serialized.split_whitespace().count();
        let expected = ((words as f64) * 1.3).ceil() as usize;
        assert_eq!(estimate_tokens(serialized), expected);
    }
}
