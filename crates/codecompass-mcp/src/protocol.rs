use codecompass_core::constants;
use codecompass_core::types::{
    FreshnessStatus, IndexingStatus, RankingReasons, ResultCompleteness, SchemaStatus,
};
use serde::{Deserialize, Serialize};

/// Protocol v1 response metadata included in every tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetadata {
    pub codecompass_protocol_version: String,
    pub freshness_status: FreshnessStatus,
    pub indexing_status: IndexingStatus,
    pub result_completeness: ResultCompleteness,
    pub r#ref: String,
    pub schema_status: SchemaStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_reasons: Option<Vec<RankingReasons>>,
}

impl ProtocolMetadata {
    /// Create metadata with default "healthy" values.
    pub fn new(r#ref: &str) -> Self {
        Self {
            codecompass_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status: FreshnessStatus::Fresh,
            indexing_status: IndexingStatus::Idle,
            result_completeness: ResultCompleteness::Complete,
            r#ref: r#ref.to_string(),
            schema_status: SchemaStatus::Compatible,
            ranking_reasons: None,
        }
    }

    /// Create metadata indicating no index is available.
    pub fn not_indexed(r#ref: &str) -> Self {
        Self {
            codecompass_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status: FreshnessStatus::Stale,
            indexing_status: IndexingStatus::Idle,
            result_completeness: ResultCompleteness::Partial,
            r#ref: r#ref.to_string(),
            schema_status: SchemaStatus::NotIndexed,
            ranking_reasons: None,
        }
    }

    /// Create metadata indicating indexing is in progress.
    pub fn syncing(r#ref: &str) -> Self {
        Self {
            codecompass_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status: FreshnessStatus::Syncing,
            indexing_status: IndexingStatus::Indexing,
            result_completeness: ResultCompleteness::Partial,
            r#ref: r#ref.to_string(),
            schema_status: SchemaStatus::Compatible,
            ranking_reasons: None,
        }
    }

    /// Create metadata indicating schema migration is required before querying.
    pub fn reindex_required(r#ref: &str) -> Self {
        Self {
            codecompass_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status: FreshnessStatus::Stale,
            indexing_status: IndexingStatus::Idle,
            result_completeness: ResultCompleteness::Partial,
            r#ref: r#ref.to_string(),
            schema_status: SchemaStatus::ReindexRequired,
            ranking_reasons: None,
        }
    }

    /// Create metadata indicating the index is corrupted and unusable.
    pub fn corrupt_manifest(r#ref: &str) -> Self {
        Self {
            codecompass_protocol_version: constants::PROTOCOL_VERSION.to_string(),
            freshness_status: FreshnessStatus::Stale,
            indexing_status: IndexingStatus::Idle,
            result_completeness: ResultCompleteness::Partial,
            r#ref: r#ref.to_string(),
            schema_status: SchemaStatus::CorruptManifest,
            ranking_reasons: None,
        }
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
