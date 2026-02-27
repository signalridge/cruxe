use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// A call edge extracted from source code.
///
/// `to_symbol_id` is `None` when the callee cannot be resolved to an indexed symbol.
/// In that case `to_name` carries the best-effort callee text from the call site.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CallEdge {
    pub repo: String,
    pub ref_name: String,
    pub from_symbol_id: String,
    pub to_symbol_id: Option<String>,
    pub to_name: Option<String>,
    pub edge_type: String,
    pub confidence: String,
    pub source_file: String,
    pub source_line: u32,
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

/// Semantic retrieval execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMode {
    /// Disable semantic retrieval.
    #[default]
    Off,
    /// Keep lexical retrieval and optional rerank only (no vector branch).
    RerankOnly,
    /// Enable semantic/hybrid retrieval path for natural-language intent.
    Hybrid,
}

/// Composite confidence guidance payload for search responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceGuidance {
    pub low_confidence: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    pub threshold: f64,
    pub top_score: f64,
    pub score_margin: f64,
    pub channel_agreement: f64,
}

/// Provider-specific rerank score for one document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    pub doc: String,
    pub score: f64,
    pub provider: String,
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

/// Canonical merge key for base/overlay reconciliation paths.
///
/// This prevents ad-hoc tuple/string keying drift across VCS overlay merges.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OverlayMergeKey {
    pub repo: String,
    pub ref_name: String,
    pub path: String,
}

impl OverlayMergeKey {
    pub fn new(
        repo: impl Into<String>,
        ref_name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            repo: repo.into(),
            ref_name: ref_name.into(),
            path: path.into(),
        }
    }

    pub fn symbol(
        repo: impl Into<String>,
        symbol_stable_id: impl AsRef<str>,
        kind: impl AsRef<str>,
    ) -> Self {
        Self::new(
            repo,
            "symbol",
            format!("{}:{}", symbol_stable_id.as_ref(), kind.as_ref()),
        )
    }

    pub fn snippet(
        repo: impl Into<String>,
        path: impl AsRef<str>,
        chunk_type: impl AsRef<str>,
        line_start: u32,
        line_end: u32,
    ) -> Self {
        Self::new(
            repo,
            "snippet",
            format!(
                "{}:{}:{}:{}",
                path.as_ref(),
                chunk_type.as_ref(),
                line_start,
                line_end
            ),
        )
    }

    pub fn file(repo: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new(repo, "file", path)
    }

    pub fn fallback(
        repo: impl Into<String>,
        result_type: impl Into<String>,
        path: impl AsRef<str>,
        line_start: u32,
        line_end: u32,
    ) -> Self {
        Self::new(
            repo,
            result_type,
            format!("{}:{}:{}", path.as_ref(), line_start, line_end),
        )
    }
}

/// Origin of a VCS-mode query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLayer {
    Base,
    Overlay,
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
    Interrupted,
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
            Self::Interrupted => "interrupted",
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

/// Configuration for multi-workspace auto-discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub auto_workspace: bool,
    pub allowed_roots: AllowedRoots,
    pub max_auto_workspaces: usize,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            auto_workspace: false,
            allowed_roots: AllowedRoots::default(),
            max_auto_workspaces: 10,
        }
    }
}

/// Newtype around a set of allowed root path prefixes for workspace validation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllowedRoots(Vec<PathBuf>);

impl AllowedRoots {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self(roots)
    }

    /// Check if the given path falls under at least one allowed root prefix.
    /// Both `path` and the stored roots are assumed to be already canonicalized.
    pub fn contains(&self, path: &Path) -> bool {
        self.0.iter().any(|root| path.starts_with(root))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Validate and canonicalize a workspace path against allowed roots.
///
/// 1. Resolves `path` via `std::fs::canonicalize` (realpath equivalent).
/// 2. Verifies the resolved path starts with at least one allowed root prefix.
/// 3. Returns the canonicalized path on success.
pub fn validate_workspace_path(
    path: &Path,
    allowed_roots: &AllowedRoots,
) -> std::result::Result<PathBuf, crate::error::WorkspaceError> {
    let canonical =
        std::fs::canonicalize(path).map_err(|e| crate::error::WorkspaceError::NotAllowed {
            path: path.display().to_string(),
            reason: format!("path resolution failed: {e}"),
        })?;

    if !allowed_roots.contains(&canonical) {
        return Err(crate::error::WorkspaceError::NotAllowed {
            path: canonical.display().to_string(),
            reason: "path is outside all --allowed-root prefixes".to_string(),
        });
    }

    Ok(canonical)
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

    // ------------------------------------------------------------------
    // T201: AllowedRoots::contains() unit tests
    // ------------------------------------------------------------------

    #[test]
    fn t201_allowed_roots_prefix_matching() {
        let roots = AllowedRoots::new(vec![PathBuf::from("/home/user/projects")]);
        assert!(roots.contains(Path::new("/home/user/projects")));
        assert!(roots.contains(Path::new("/home/user/projects/repo-a")));
        assert!(roots.contains(Path::new("/home/user/projects/repo-a/src")));
        assert!(!roots.contains(Path::new("/home/user")));
        assert!(!roots.contains(Path::new("/home/user/other")));
        assert!(!roots.contains(Path::new("/tmp/projects")));
    }

    #[test]
    fn t201_allowed_roots_multiple_roots() {
        let roots = AllowedRoots::new(vec![
            PathBuf::from("/home/user/projects"),
            PathBuf::from("/opt/work"),
        ]);
        assert!(roots.contains(Path::new("/home/user/projects/repo")));
        assert!(roots.contains(Path::new("/opt/work/repo")));
        assert!(!roots.contains(Path::new("/tmp/repo")));
    }

    #[test]
    fn t201_allowed_roots_empty_rejects_all() {
        let roots = AllowedRoots::default();
        assert!(roots.is_empty());
        assert!(!roots.contains(Path::new("/any/path")));
        assert!(!roots.contains(Path::new("/")));
    }

    #[test]
    fn t201_allowed_roots_path_traversal_not_fooled() {
        // Path::starts_with does component-level matching, so
        // "/home/user/projects-evil" does NOT start_with "/home/user/projects"
        let roots = AllowedRoots::new(vec![PathBuf::from("/home/user/projects")]);
        assert!(!roots.contains(Path::new("/home/user/projects-evil")));
        assert!(!roots.contains(Path::new("/home/user/projects_backup")));
    }

    // ------------------------------------------------------------------
    // T202: validate_workspace_path() unit tests
    // ------------------------------------------------------------------

    #[test]
    fn t202_validate_workspace_path_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical_root = tmp.path().canonicalize().unwrap();
        let roots = AllowedRoots::new(vec![canonical_root.clone()]);

        let sub = tmp.path().join("repo");
        std::fs::create_dir_all(&sub).unwrap();

        let result = validate_workspace_path(&sub, &roots);
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(canonical.starts_with(&canonical_root));
    }

    #[test]
    fn t202_validate_workspace_path_outside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical_root = tmp.path().canonicalize().unwrap();
        let roots = AllowedRoots::new(vec![canonical_root.join("allowed")]);

        // tmp itself exists but is not under "allowed"
        let result = validate_workspace_path(tmp.path(), &roots);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, crate::error::WorkspaceError::NotAllowed { .. }),
            "expected NotAllowed, got: {err:?}"
        );
    }

    #[test]
    fn t202_validate_workspace_path_nonexistent() {
        let roots = AllowedRoots::new(vec![PathBuf::from("/tmp")]);
        let result =
            validate_workspace_path(Path::new("/tmp/nonexistent-9f8a7b6c5d4e3f2a1b"), &roots);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // canonicalize fails on nonexistent paths -> NotAllowed with "path resolution failed"
        assert!(
            matches!(err, crate::error::WorkspaceError::NotAllowed { .. }),
            "expected NotAllowed for nonexistent path, got: {err:?}"
        );
    }

    #[test]
    fn t202_validate_workspace_path_symlink_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical_root = tmp.path().canonicalize().unwrap();
        let roots = AllowedRoots::new(vec![canonical_root.clone()]);

        // Create a real dir and a symlink to it
        let real_dir = tmp.path().join("real");
        std::fs::create_dir_all(&real_dir).unwrap();
        let link = tmp.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &link).unwrap();

        #[cfg(unix)]
        {
            let result = validate_workspace_path(&link, &roots);
            assert!(result.is_ok());
            let canonical = result.unwrap();
            // The canonical path should resolve through the symlink to the real dir
            assert!(canonical.starts_with(&canonical_root));
        }
    }

    #[test]
    fn overlay_merge_key_equality_and_ordering() {
        let base = OverlayMergeKey::new("repo", "main", "src/lib.rs");
        let overlay_same = OverlayMergeKey::new("repo", "main", "src/lib.rs");
        let overlay_other = OverlayMergeKey::new("repo", "feature", "src/lib.rs");

        assert_eq!(base, overlay_same, "equal logical merge keys must match");
        assert_ne!(
            base, overlay_other,
            "ref changes must produce distinct merge keys"
        );

        let mut keys = [
            OverlayMergeKey::new("repo", "feature", "src/z.rs"),
            OverlayMergeKey::new("repo", "feature", "src/a.rs"),
            OverlayMergeKey::new("repo", "main", "src/a.rs"),
        ];
        keys.sort();

        assert_eq!(keys[0], OverlayMergeKey::new("repo", "feature", "src/a.rs"));
        assert_eq!(keys[1], OverlayMergeKey::new("repo", "feature", "src/z.rs"));
        assert_eq!(keys[2], OverlayMergeKey::new("repo", "main", "src/a.rs"));
    }
}
