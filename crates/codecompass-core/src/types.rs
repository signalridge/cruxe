use serde::{Deserialize, Serialize};

/// A registered workspace/project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    pub repo_root: String,
    pub display_name: Option<String>,
    pub default_ref: String,
    pub vcs_mode: bool,
    pub schema_version: u32,
    pub parser_version: u32,
    pub created_at: String,
    pub updated_at: String,
}

/// Symbol kinds recognized by CodeCompass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Trait,
    Interface,
    Constant,
    Variable,
    TypeAlias,
    Module,
    Import,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::TypeAlias => "type_alias",
            Self::Module => "module",
            Self::Import => "import",
        }
    }

    pub fn parse_kind(s: &str) -> Option<Self> {
        match s {
            "function" | "fn" | "func" | "def" => Some(Self::Function),
            "method" => Some(Self::Method),
            "struct" => Some(Self::Struct),
            "class" => Some(Self::Class),
            "enum" => Some(Self::Enum),
            "trait" => Some(Self::Trait),
            "interface" => Some(Self::Interface),
            "constant" | "const" => Some(Self::Constant),
            "variable" | "var" => Some(Self::Variable),
            "type_alias" | "type" => Some(Self::TypeAlias),
            "module" | "mod" => Some(Self::Module),
            "import" | "use" => Some(Self::Import),
            _ => None,
        }
    }
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A symbol definition extracted from source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRecord {
    pub repo: String,
    pub r#ref: String,
    pub commit: Option<String>,
    pub path: String,
    pub language: String,
    pub symbol_id: String,
    pub symbol_stable_id: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub signature: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub parent_symbol_id: Option<String>,
    pub visibility: Option<String>,
    pub content: Option<String>,
}

/// A code snippet (function body, class body) for full-text search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetRecord {
    pub repo: String,
    pub r#ref: String,
    pub commit: Option<String>,
    pub path: String,
    pub language: String,
    pub chunk_type: String,
    pub imports: Option<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub content: String,
}

/// A source file record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub repo: String,
    pub r#ref: String,
    pub commit: Option<String>,
    pub path: String,
    pub filename: String,
    pub language: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub updated_at: String,
    pub content_head: Option<String>,
}

/// A directed relationship edge between two symbols.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolEdge {
    pub repo: String,
    pub ref_name: String,
    pub from_symbol_id: String,
    pub to_symbol_id: String,
    pub edge_type: String,
    pub confidence: String,
}

/// Detail level for response verbosity control.
/// Controls how many fields are included in search/locate results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    /// Minimal: path, line_start, line_end, kind, name (~50 tokens)
    Location,
    /// Default: adds qualified_name, signature, language, visibility (~100 tokens)
    #[default]
    Signature,
    /// Full: adds body_preview, parent, related_symbols (~300-500 tokens)
    Context,
}

/// Freshness policy for pre-query staleness checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessPolicy {
    /// Block queries when index is stale.
    Strict,
    /// Return results with stale indicator; trigger async sync.
    #[default]
    Balanced,
    /// Always return results immediately; no sync triggered.
    BestEffort,
}

/// Ranking explainability payload level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingExplainLevel {
    /// Omit ranking reasons from metadata.
    #[default]
    Off,
    /// Include compact normalized factors for agent routing.
    Basic,
    /// Include full per-result scoring breakdown.
    Full,
}

/// Per-result ranking explanation for debug mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingReasons {
    pub result_index: usize,
    pub exact_match_boost: f64,
    pub qualified_name_boost: f64,
    pub path_affinity: f64,
    pub definition_boost: f64,
    pub kind_match: f64,
    pub bm25_score: f64,
    pub final_score: f64,
}

/// Compact ranking factors for basic explainability mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicRankingReasons {
    pub result_index: usize,
    pub exact_match: f64,
    pub path_boost: f64,
    pub definition_boost: f64,
    pub semantic_similarity: f64,
    pub final_score: f64,
}

/// Query intent classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    Symbol,
    Path,
    Error,
    NaturalLanguage,
}

/// Ref scope for queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefScope {
    pub r#ref: String,
    pub is_explicit: bool,
}

impl RefScope {
    pub fn live() -> Self {
        Self {
            r#ref: crate::constants::REF_LIVE.to_string(),
            is_explicit: false,
        }
    }

    pub fn explicit(r#ref: impl Into<String>) -> Self {
        Self {
            r#ref: r#ref.into(),
            is_explicit: true,
        }
    }
}

/// Freshness status of the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessStatus {
    Fresh,
    Stale,
    Syncing,
}

/// Indexing status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexingStatus {
    NotIndexed,
    Indexing,
    #[serde(alias = "idle", alias = "partial_available")]
    Ready,
    Failed,
}

/// Result completeness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCompleteness {
    Complete,
    Partial,
    Truncated,
}

/// Schema compatibility status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaStatus {
    Compatible,
    NotIndexed,
    ReindexRequired,
    CorruptManifest,
}

/// Index job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Validating,
    Published,
    Failed,
    RolledBack,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Validating => "validating",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
        }
    }
}

/// Generate project_id from repo root path.
/// Uses blake3 hash of the canonical path, truncated to 16 hex characters.
pub fn generate_project_id(repo_root: &str) -> String {
    let canonical =
        std::fs::canonicalize(repo_root).unwrap_or_else(|_| std::path::PathBuf::from(repo_root));
    let hash = blake3::hash(canonical.to_string_lossy().as_bytes());
    hash.to_hex()[..16].to_string()
}

/// Compute symbol_stable_id using blake3.
/// Format: blake3("stable_id:v1|{language}|{kind}|{qualified_name}|{normalized_signature}")
pub fn compute_symbol_stable_id(
    language: &str,
    kind: &SymbolKind,
    qualified_name: &str,
    signature: Option<&str>,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}",
        crate::constants::STABLE_ID_VERSION,
        language,
        kind.as_str(),
        qualified_name,
        signature.unwrap_or("")
    );
    let hash = blake3::hash(input.as_bytes());
    hash.to_hex().to_string()
}

/// Compute symbol_id (ref-local, changes on line movement).
/// Format: blake3("{repo}|{ref}|{path}|{kind}|{line_start}|{name}")
pub fn compute_symbol_id(
    repo: &str,
    r#ref: &str,
    path: &str,
    kind: &SymbolKind,
    line_start: u32,
    name: &str,
) -> String {
    let input = format!(
        "{}|{}|{}|{}|{}|{}",
        repo,
        r#ref,
        path,
        kind.as_str(),
        line_start,
        name
    );
    let hash = blake3::hash(input.as_bytes());
    hash.to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_stable_id_line_independence() {
        // Same symbol at different lines should produce same stable_id
        let id1 = compute_symbol_stable_id(
            "rust",
            &SymbolKind::Function,
            "auth::validate_token",
            Some("fn(token: &str) -> Result<Claims>"),
        );
        let id2 = compute_symbol_stable_id(
            "rust",
            &SymbolKind::Function,
            "auth::validate_token",
            Some("fn(token: &str) -> Result<Claims>"),
        );
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_symbol_stable_id_signature_change() {
        // Different signature should produce different stable_id
        let id1 = compute_symbol_stable_id(
            "rust",
            &SymbolKind::Function,
            "auth::validate_token",
            Some("fn(token: &str) -> Result<Claims>"),
        );
        let id2 = compute_symbol_stable_id(
            "rust",
            &SymbolKind::Function,
            "auth::validate_token",
            Some("fn(token: &str, key: &[u8]) -> Result<Claims>"),
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_symbol_stable_id_no_signature() {
        let id = compute_symbol_stable_id("rust", &SymbolKind::Struct, "auth::Claims", None);
        assert!(!id.is_empty());
        assert_eq!(id.len(), 64); // blake3 hex
    }

    #[test]
    fn test_symbol_id_changes_with_line() {
        let id1 = compute_symbol_id(
            "repo",
            "main",
            "src/lib.rs",
            &SymbolKind::Function,
            10,
            "foo",
        );
        let id2 = compute_symbol_id(
            "repo",
            "main",
            "src/lib.rs",
            &SymbolKind::Function,
            20,
            "foo",
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_detail_level_serde_roundtrip() {
        for (variant, expected_str) in [
            (DetailLevel::Location, "\"location\""),
            (DetailLevel::Signature, "\"signature\""),
            (DetailLevel::Context, "\"context\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let parsed: DetailLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_detail_level_default() {
        assert_eq!(DetailLevel::default(), DetailLevel::Signature);
    }

    #[test]
    fn test_freshness_policy_serde_roundtrip() {
        for (variant, expected_str) in [
            (FreshnessPolicy::Strict, "\"strict\""),
            (FreshnessPolicy::Balanced, "\"balanced\""),
            (FreshnessPolicy::BestEffort, "\"best_effort\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let parsed: FreshnessPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_freshness_policy_default() {
        assert_eq!(FreshnessPolicy::default(), FreshnessPolicy::Balanced);
    }

    #[test]
    fn test_ranking_explain_level_serde_roundtrip() {
        for (variant, expected_str) in [
            (RankingExplainLevel::Off, "\"off\""),
            (RankingExplainLevel::Basic, "\"basic\""),
            (RankingExplainLevel::Full, "\"full\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let parsed: RankingExplainLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_indexing_status_legacy_aliases() {
        let parsed_idle: IndexingStatus = serde_json::from_str("\"idle\"").unwrap();
        assert_eq!(parsed_idle, IndexingStatus::Ready);

        let parsed_partial: IndexingStatus = serde_json::from_str("\"partial_available\"").unwrap();
        assert_eq!(parsed_partial, IndexingStatus::Ready);
    }

    #[test]
    fn test_result_completeness_roundtrip() {
        for (variant, expected_str) in [
            (ResultCompleteness::Complete, "\"complete\""),
            (ResultCompleteness::Partial, "\"partial\""),
            (ResultCompleteness::Truncated, "\"truncated\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let parsed: ResultCompleteness = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_symbol_kind_roundtrip() {
        for kind in [
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Struct,
            SymbolKind::Class,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Interface,
            SymbolKind::Constant,
        ] {
            assert_eq!(SymbolKind::parse_kind(kind.as_str()), Some(kind));
        }
    }
}
