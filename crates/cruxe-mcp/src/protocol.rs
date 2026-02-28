use cruxe_core::constants;
use cruxe_core::types::{FreshnessStatus, IndexingStatus, ResultCompleteness, SchemaStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol v1 response metadata included in every tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetadata {
    pub cruxe_protocol_version: String,
    pub freshness_status: FreshnessStatus,
    pub indexing_status: IndexingStatus,
    pub result_completeness: ResultCompleteness,
    pub r#ref: String,
    pub schema_status: SchemaStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_reasons: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressed_duplicate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_limit_applied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_blocked_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_redacted_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_audit_counts: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_redaction_categories: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_ratio_used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_triggered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_degraded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_limit_used: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_fanout_used: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_fanout_used: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_budget_exhausted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_provider_blocked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_model_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_confidence: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_margin: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_agreement: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_intent_confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_escalation_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_selected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_executed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_selection_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_downgraded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_downgrade_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_plan_budget_used: Option<Value>,
}

impl ProtocolMetadata {
    /// Internal base constructor — all `Option` fields default to `None`.
    fn base(
        r#ref: &str,
        freshness_status: FreshnessStatus,
        indexing_status: IndexingStatus,
        result_completeness: ResultCompleteness,
        schema_status: SchemaStatus,
    ) -> Self {
        Self {
            cruxe_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status,
            indexing_status,
            result_completeness,
            r#ref: r#ref.to_string(),
            schema_status,
            ranking_reasons: None,
            suppressed_duplicate_count: None,
            safety_limit_applied: None,
            warnings: None,
            policy_mode: None,
            policy_blocked_count: None,
            policy_redacted_count: None,
            policy_warnings: None,
            policy_audit_counts: None,
            policy_redaction_categories: None,
            semantic_mode: None,
            semantic_enabled: None,
            semantic_ratio_used: None,
            semantic_triggered: None,
            semantic_skipped_reason: None,
            semantic_fallback: None,
            semantic_degraded: None,
            semantic_limit_used: None,
            lexical_fanout_used: None,
            semantic_fanout_used: None,
            semantic_budget_exhausted: None,
            external_provider_blocked: None,
            embedding_model_version: None,
            rerank_provider: None,
            rerank_fallback: None,
            rerank_fallback_reason: None,
            low_confidence: None,
            suggested_action: None,
            confidence_threshold: None,
            top_score: None,
            score_margin: None,
            channel_agreement: None,
            query_intent_confidence: None,
            intent_escalation_hint: None,
            query_plan_selected: None,
            query_plan_executed: None,
            query_plan_selection_reason: None,
            query_plan_downgraded: None,
            query_plan_downgrade_reason: None,
            query_plan_budget_used: None,
        }
    }

    /// Create metadata with default "healthy" values.
    pub fn new(r#ref: &str) -> Self {
        Self::base(
            r#ref,
            FreshnessStatus::Fresh,
            IndexingStatus::Ready,
            ResultCompleteness::Complete,
            SchemaStatus::Compatible,
        )
    }

    /// Create metadata indicating no index is available.
    pub fn not_indexed(r#ref: &str) -> Self {
        Self::base(
            r#ref,
            FreshnessStatus::Stale,
            IndexingStatus::NotIndexed,
            ResultCompleteness::Partial,
            SchemaStatus::NotIndexed,
        )
    }

    /// Create metadata indicating indexing is in progress.
    pub fn syncing(r#ref: &str) -> Self {
        Self::base(
            r#ref,
            FreshnessStatus::Syncing,
            IndexingStatus::Indexing,
            ResultCompleteness::Partial,
            SchemaStatus::Compatible,
        )
    }

    /// Create metadata indicating schema migration is required before querying.
    pub fn reindex_required(r#ref: &str) -> Self {
        Self::base(
            r#ref,
            FreshnessStatus::Stale,
            IndexingStatus::Failed,
            ResultCompleteness::Partial,
            SchemaStatus::ReindexRequired,
        )
    }

    /// Create metadata indicating the index is corrupted and unusable.
    pub fn corrupt_manifest(r#ref: &str) -> Self {
        Self::base(
            r#ref,
            FreshnessStatus::Stale,
            IndexingStatus::Failed,
            ResultCompleteness::Partial,
            SchemaStatus::CorruptManifest,
        )
    }

    /// Update freshness based on active job check.
    pub fn with_active_job(mut self, has_active_job: bool) -> Self {
        if has_active_job {
            self.freshness_status = FreshnessStatus::Syncing;
            self.indexing_status = IndexingStatus::Indexing;
            self.result_completeness = ResultCompleteness::Partial;
        }
        self
    }
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}
