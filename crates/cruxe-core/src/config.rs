use crate::constants;
use crate::error::ConfigError;
use crate::languages;
use crate::types::{FreshnessPolicy, PolicyMode, QueryIntent, RankingExplainLevel, SemanticMode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub debug: DebugConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
    #[serde(default = "default_limit")]
    pub default_limit: usize,
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_busy_timeout")]
    pub busy_timeout_ms: u32,
    #[serde(default = "default_cache_size")]
    pub cache_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_ref")]
    pub default_ref: String,
    #[serde(default = "default_freshness_policy")]
    pub freshness_policy: String,
    #[serde(default = "default_ranking_explain_level")]
    pub ranking_explain_level: String,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: usize,
    #[serde(default)]
    pub intent: SearchIntentConfig,
    #[serde(default)]
    pub semantic: SemanticConfig,
    #[serde(default)]
    pub policy: RetrievalPolicyConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    #[serde(default)]
    pub ranking_reasons: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIntentConfig {
    #[serde(default = "default_intent_rule_order")]
    pub rule_order: Vec<String>,
    #[serde(default = "default_intent_error_patterns")]
    pub error_patterns: Vec<String>,
    #[serde(default = "default_intent_path_extensions")]
    pub path_extensions: Vec<String>,
    #[serde(default = "default_intent_symbol_kind_keywords")]
    pub symbol_kind_keywords: Vec<String>,
    #[serde(default = "default_intent_enable_wrapped_quoted_error_literal")]
    pub enable_wrapped_quoted_error_literal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    #[serde(default = "default_semantic_mode", alias = "semantic_mode")]
    pub mode: String,
    #[serde(default = "default_semantic_ratio")]
    pub ratio: f64,
    #[serde(default = "default_lexical_short_circuit_threshold")]
    pub lexical_short_circuit_threshold: f64,
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,
    #[serde(default = "default_confidence_top_score_weight")]
    pub confidence_top_score_weight: f64,
    #[serde(default = "default_confidence_score_margin_weight")]
    pub confidence_score_margin_weight: f64,
    #[serde(default = "default_confidence_channel_agreement_weight")]
    pub confidence_channel_agreement_weight: f64,
    #[serde(default = "default_local_rerank_phrase_boost")]
    pub local_rerank_phrase_boost: f64,
    #[serde(default = "default_local_rerank_token_overlap_weight")]
    pub local_rerank_token_overlap_weight: f64,
    #[serde(default = "default_semantic_limit_multiplier")]
    pub semantic_limit_multiplier: usize,
    #[serde(default = "default_lexical_fanout_multiplier")]
    pub lexical_fanout_multiplier: usize,
    #[serde(default = "default_semantic_fanout_multiplier")]
    pub semantic_fanout_multiplier: usize,
    #[serde(default = "default_profile_advisor_mode")]
    pub profile_advisor_mode: String,
    #[serde(default)]
    pub external_provider_enabled: bool,
    #[serde(default)]
    pub allow_code_payload_to_external: bool,
    #[serde(default)]
    pub embedding: SemanticEmbeddingConfig,
    #[serde(default)]
    pub rerank: SemanticRerankConfig,
    #[serde(default)]
    pub overrides: SemanticOverridesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalPolicyConfig {
    #[serde(default = "default_policy_mode")]
    pub mode: String,
    #[serde(default)]
    pub allow_request_override: bool,
    #[serde(default = "default_policy_allowed_override_modes")]
    pub allowed_override_modes: Vec<String>,
    #[serde(default)]
    pub path: PolicyPathConfig,
    #[serde(default)]
    pub kind: PolicyKindConfig,
    #[serde(default)]
    pub redaction: PolicyRedactionConfig,
    #[serde(default)]
    pub detect_secrets: DetectSecretsCompatConfig,
    #[serde(default)]
    pub opa: OpaPolicyConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyPathConfig {
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyKindConfig {
    #[serde(default)]
    pub deny_result_types: Vec<String>,
    #[serde(default)]
    pub allow_result_types: Vec<String>,
    #[serde(default)]
    pub deny_symbol_kinds: Vec<String>,
    #[serde(default)]
    pub allow_symbol_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRedactionConfig {
    #[serde(default = "default_policy_redaction_enabled")]
    pub enabled: bool,
    #[serde(default = "default_policy_email_masking")]
    pub email_masking: bool,
    #[serde(default = "default_policy_high_entropy_min_length")]
    pub high_entropy_min_length: usize,
    #[serde(default = "default_policy_high_entropy_threshold")]
    pub high_entropy_threshold: f64,
    #[serde(default)]
    pub custom_rules: Vec<PolicyRedactionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRedactionRule {
    pub name: String,
    pub category: String,
    pub pattern: String,
    #[serde(default = "default_policy_redaction_placeholder")]
    pub placeholder: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectSecretsCompatConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub custom_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpaPolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_policy_opa_command")]
    pub command: String,
    #[serde(default = "default_policy_opa_query")]
    pub query: String,
    #[serde(default)]
    pub policy_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticOverridesConfig {
    #[serde(default)]
    pub natural_language: SemanticQueryOverride,
    #[serde(default)]
    pub symbol: SemanticQueryOverride,
    #[serde(default)]
    pub path: SemanticQueryOverride,
    #[serde(default)]
    pub error: SemanticQueryOverride,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticQueryOverride {
    #[serde(default)]
    pub ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEmbeddingConfig {
    #[serde(default = "default_semantic_embedding_profile", alias = "profile")]
    pub profile: String,
    #[serde(default = "default_semantic_embedding_provider")]
    pub provider: String,
    #[serde(default = "default_semantic_embedding_model")]
    pub model: String,
    #[serde(default = "default_semantic_embedding_model_version")]
    pub model_version: String,
    #[serde(default = "default_semantic_embedding_dimensions")]
    pub dimensions: usize,
    #[serde(default = "default_semantic_embedding_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_semantic_vector_backend")]
    pub vector_backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticRerankConfig {
    #[serde(default = "default_semantic_rerank_provider")]
    pub provider: String,
    #[serde(default = "default_semantic_rerank_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub endpoint: Option<String>,
}

fn default_max_file_size() -> u64 {
    constants::MAX_FILE_SIZE
}
fn default_limit() -> usize {
    constants::DEFAULT_LIMIT
}
fn default_languages() -> Vec<String> {
    languages::supported_indexable_languages()
        .iter()
        .map(|language| (*language).to_string())
        .collect()
}
fn default_data_dir() -> String {
    "~/.cruxe".into()
}
fn default_busy_timeout() -> u32 {
    5000
}
fn default_cache_size() -> i32 {
    -64000
}
fn default_ref() -> String {
    "main".into()
}
fn default_freshness_policy() -> String {
    "balanced".into()
}
fn default_ranking_explain_level() -> String {
    "off".into()
}
fn default_max_response_bytes() -> usize {
    64 * 1024
}
fn default_intent_rule_order() -> Vec<String> {
    vec![
        "error_pattern".into(),
        "path".into(),
        "quoted_error".into(),
        "symbol".into(),
        "natural_language".into(),
    ]
}
fn default_intent_error_patterns() -> Vec<String> {
    vec![
        "error:".into(),
        "Error:".into(),
        "panic:".into(),
        "FATAL".into(),
        "exception".into(),
        "Exception".into(),
        "traceback".into(),
        "at line".into(),
        "thread '".into(),
    ]
}
fn default_intent_path_extensions() -> Vec<String> {
    vec![
        ".rs".into(),
        ".ts".into(),
        ".tsx".into(),
        ".js".into(),
        ".jsx".into(),
        ".py".into(),
        ".go".into(),
        ".java".into(),
        ".c".into(),
        ".h".into(),
        ".cpp".into(),
        ".rb".into(),
        ".swift".into(),
    ]
}
fn default_intent_symbol_kind_keywords() -> Vec<String> {
    vec![
        "fn".into(),
        "func".into(),
        "function".into(),
        "struct".into(),
        "class".into(),
        "enum".into(),
        "trait".into(),
        "interface".into(),
        "type".into(),
        "const".into(),
        "method".into(),
    ]
}
fn default_intent_enable_wrapped_quoted_error_literal() -> bool {
    true
}
fn default_semantic_mode() -> String {
    "off".into()
}
fn default_semantic_ratio() -> f64 {
    0.3
}
fn default_lexical_short_circuit_threshold() -> f64 {
    0.85
}
fn default_confidence_threshold() -> f64 {
    0.5
}
fn default_confidence_top_score_weight() -> f64 {
    0.55
}
fn default_confidence_score_margin_weight() -> f64 {
    0.30
}
fn default_confidence_channel_agreement_weight() -> f64 {
    0.15
}
fn default_local_rerank_phrase_boost() -> f64 {
    0.75
}
fn default_local_rerank_token_overlap_weight() -> f64 {
    2.5
}
fn default_semantic_limit_multiplier() -> usize {
    2
}
fn default_lexical_fanout_multiplier() -> usize {
    4
}
fn default_semantic_fanout_multiplier() -> usize {
    3
}
fn default_profile_advisor_mode() -> String {
    "off".into()
}
fn default_semantic_embedding_profile() -> String {
    "fast_local".into()
}
fn default_semantic_embedding_provider() -> String {
    "local".into()
}
fn default_semantic_embedding_model() -> String {
    "NomicEmbedTextV15Q".into()
}
fn default_semantic_embedding_model_version() -> String {
    "fastembed-1".into()
}
fn default_semantic_embedding_dimensions() -> usize {
    768
}
fn default_semantic_embedding_batch_size() -> usize {
    32
}
fn default_semantic_vector_backend() -> String {
    "sqlite".into()
}
fn default_semantic_rerank_provider() -> String {
    "none".into()
}
fn default_semantic_rerank_timeout_ms() -> u64 {
    5000
}
fn default_policy_mode() -> String {
    "balanced".into()
}
fn default_policy_allowed_override_modes() -> Vec<String> {
    vec![
        "balanced".to_string(),
        "off".to_string(),
        "audit_only".to_string(),
    ]
}
fn default_policy_redaction_enabled() -> bool {
    true
}
fn default_policy_email_masking() -> bool {
    true
}
fn default_policy_high_entropy_min_length() -> usize {
    20
}
fn default_policy_high_entropy_threshold() -> f64 {
    3.5
}
fn default_policy_redaction_placeholder() -> String {
    "[REDACTED]".to_string()
}
fn default_policy_opa_command() -> String {
    "opa".to_string()
}
fn default_policy_opa_query() -> String {
    "data.cruxe.allow".to_string()
}
fn default_log_level() -> String {
    "info".into()
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            max_file_size: default_max_file_size(),
            default_limit: default_limit(),
            languages: default_languages(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            busy_timeout_ms: default_busy_timeout(),
            cache_size: default_cache_size(),
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_ref: default_ref(),
            freshness_policy: default_freshness_policy(),
            ranking_explain_level: default_ranking_explain_level(),
            max_response_bytes: default_max_response_bytes(),
            intent: SearchIntentConfig::default(),
            semantic: SemanticConfig::default(),
            policy: RetrievalPolicyConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl Default for SearchIntentConfig {
    fn default() -> Self {
        Self {
            rule_order: default_intent_rule_order(),
            error_patterns: default_intent_error_patterns(),
            path_extensions: default_intent_path_extensions(),
            symbol_kind_keywords: default_intent_symbol_kind_keywords(),
            enable_wrapped_quoted_error_literal: default_intent_enable_wrapped_quoted_error_literal(
            ),
        }
    }
}

impl SearchIntentConfig {
    /// Return a normalized copy with canonical rule names, trimmed lists, and
    /// fallback defaults when user-provided values are invalid/empty.
    pub fn normalized(&self) -> Self {
        Self {
            rule_order: normalize_intent_rule_order(&self.rule_order),
            error_patterns: normalize_intent_error_patterns(&self.error_patterns),
            path_extensions: normalize_intent_path_extensions(&self.path_extensions),
            symbol_kind_keywords: normalize_intent_symbol_kind_keywords(&self.symbol_kind_keywords),
            enable_wrapped_quoted_error_literal: self.enable_wrapped_quoted_error_literal,
        }
    }
}

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            mode: default_semantic_mode(),
            ratio: default_semantic_ratio(),
            lexical_short_circuit_threshold: default_lexical_short_circuit_threshold(),
            confidence_threshold: default_confidence_threshold(),
            confidence_top_score_weight: default_confidence_top_score_weight(),
            confidence_score_margin_weight: default_confidence_score_margin_weight(),
            confidence_channel_agreement_weight: default_confidence_channel_agreement_weight(),
            local_rerank_phrase_boost: default_local_rerank_phrase_boost(),
            local_rerank_token_overlap_weight: default_local_rerank_token_overlap_weight(),
            semantic_limit_multiplier: default_semantic_limit_multiplier(),
            lexical_fanout_multiplier: default_lexical_fanout_multiplier(),
            semantic_fanout_multiplier: default_semantic_fanout_multiplier(),
            profile_advisor_mode: default_profile_advisor_mode(),
            external_provider_enabled: false,
            allow_code_payload_to_external: false,
            embedding: SemanticEmbeddingConfig::default(),
            rerank: SemanticRerankConfig::default(),
            overrides: SemanticOverridesConfig::default(),
        }
    }
}

impl Default for SemanticEmbeddingConfig {
    fn default() -> Self {
        Self {
            profile: default_semantic_embedding_profile(),
            provider: default_semantic_embedding_provider(),
            model: default_semantic_embedding_model(),
            model_version: default_semantic_embedding_model_version(),
            dimensions: default_semantic_embedding_dimensions(),
            batch_size: default_semantic_embedding_batch_size(),
            vector_backend: default_semantic_vector_backend(),
        }
    }
}

impl Default for SemanticRerankConfig {
    fn default() -> Self {
        Self {
            provider: default_semantic_rerank_provider(),
            timeout_ms: default_semantic_rerank_timeout_ms(),
            endpoint: None,
        }
    }
}

impl Default for RetrievalPolicyConfig {
    fn default() -> Self {
        Self {
            mode: default_policy_mode(),
            allow_request_override: false,
            allowed_override_modes: default_policy_allowed_override_modes(),
            path: PolicyPathConfig::default(),
            kind: PolicyKindConfig::default(),
            redaction: PolicyRedactionConfig::default(),
            detect_secrets: DetectSecretsCompatConfig::default(),
            opa: OpaPolicyConfig::default(),
        }
    }
}

impl Default for PolicyRedactionConfig {
    fn default() -> Self {
        Self {
            enabled: default_policy_redaction_enabled(),
            email_masking: default_policy_email_masking(),
            high_entropy_min_length: default_policy_high_entropy_min_length(),
            high_entropy_threshold: default_policy_high_entropy_threshold(),
            custom_rules: Vec::new(),
        }
    }
}

impl Default for OpaPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_policy_opa_command(),
            query: default_policy_opa_query(),
            policy_path: None,
        }
    }
}

impl SearchConfig {
    pub fn freshness_policy_typed(&self) -> FreshnessPolicy {
        parse_freshness_policy(&self.freshness_policy).unwrap_or(FreshnessPolicy::Balanced)
    }

    pub fn ranking_explain_level_typed(&self) -> RankingExplainLevel {
        parse_ranking_explain_level(&self.ranking_explain_level).unwrap_or(RankingExplainLevel::Off)
    }

    pub fn semantic_mode_typed(&self) -> SemanticMode {
        parse_semantic_mode(&self.semantic.mode).unwrap_or(SemanticMode::Off)
    }

    pub fn semantic_enabled(&self) -> bool {
        self.semantic_mode_typed() != SemanticMode::Off
    }

    pub fn policy_mode_typed(&self) -> PolicyMode {
        parse_policy_mode(&self.policy.mode).unwrap_or(PolicyMode::Balanced)
    }

    pub fn semantic_ratio_for_intent(
        &self,
        intent: QueryIntent,
        request_override: Option<f64>,
    ) -> f64 {
        if let Some(value) = request_override {
            return clamp_unit_f64_with_warning(
                value,
                default_semantic_ratio(),
                "search.request.semantic_ratio",
            );
        }

        let (config_override, field) = match intent {
            QueryIntent::NaturalLanguage => (
                self.semantic.overrides.natural_language.ratio,
                "search.semantic.overrides.natural_language.ratio",
            ),
            QueryIntent::Symbol => (
                self.semantic.overrides.symbol.ratio,
                "search.semantic.overrides.symbol.ratio",
            ),
            QueryIntent::Path => (
                self.semantic.overrides.path.ratio,
                "search.semantic.overrides.path.ratio",
            ),
            QueryIntent::Error => (
                self.semantic.overrides.error.ratio,
                "search.semantic.overrides.error.ratio",
            ),
        };
        clamp_unit_f64_with_warning(
            config_override.unwrap_or(self.semantic.ratio),
            default_semantic_ratio(),
            field,
        )
    }

    pub fn confidence_threshold(&self, request_override: Option<f64>) -> f64 {
        clamp_unit_f64_with_warning(
            request_override.unwrap_or(self.semantic.confidence_threshold),
            default_confidence_threshold(),
            "search.request.confidence_threshold",
        )
    }

    pub fn resolve_policy_mode(
        &self,
        request_override: Option<PolicyMode>,
    ) -> Result<(PolicyMode, Vec<String>), String> {
        let mut warnings = Vec::new();
        let config_mode = self.policy_mode_typed();
        let Some(request_mode) = request_override else {
            return Ok((config_mode, warnings));
        };
        if !self.policy.allow_request_override {
            return Err("request policy override is disabled by configuration".to_string());
        }

        let allowed = normalized_policy_mode_list(
            &self.policy.allowed_override_modes,
            default_policy_allowed_override_modes(),
        );
        if allowed.iter().any(|mode| mode == request_mode.as_str()) {
            warnings.push(format!(
                "policy_mode_override_applied: {} -> {}",
                config_mode, request_mode
            ));
            return Ok((request_mode, warnings));
        }

        Err(format!(
            "requested policy mode `{}` is not allowed by configuration",
            request_mode
        ))
    }
}

impl SemanticConfig {
    pub fn allow_external_provider_calls(&self) -> bool {
        self.external_provider_enabled && self.allow_code_payload_to_external
    }

    /// Return the configured vector backend as `Option<&str>` for the dispatch API.
    /// Returns `None` for the default `"sqlite"` backend.
    pub fn vector_backend_opt(&self) -> Option<&str> {
        let backend = self.embedding.vector_backend.as_str();
        if backend.is_empty() || backend == "sqlite" {
            None
        } else {
            Some(backend)
        }
    }
}

impl Config {
    /// Load configuration with three-layer precedence:
    /// 1. Explicit config file (from `--config` flag, highest priority)
    /// 2. Project config: `<repo_root>/.cruxe/config.toml`
    /// 3. Global config: `~/.cruxe/config.toml`
    /// 4. Built-in defaults (lowest priority)
    ///
    /// Only fields explicitly set in a higher-priority file override lower layers.
    pub fn load(repo_root: Option<&Path>) -> Result<Self, ConfigError> {
        Self::load_with_file(repo_root, None)
    }

    /// Load configuration with an explicit config file path (highest priority layer).
    pub fn load_with_file(
        repo_root: Option<&Path>,
        config_file: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        // Start with empty TOML value, then layer on each config file.
        // This ensures only explicitly-set fields override previous layers.
        let mut merged = toml::Value::Table(toml::map::Map::new());

        // Layer 4 (lowest priority): Global config
        if let Some(home) = dirs::home_dir() {
            let global_path = home.join(constants::DEFAULT_DATA_DIR).join("config.toml");
            if global_path.exists() {
                let raw = load_toml_value(&global_path)?;
                merge_toml_values(&mut merged, &raw);
            }
        }

        // Layer 3: Project config
        if let Some(root) = repo_root {
            let project_path = root.join(constants::PROJECT_CONFIG_FILE);
            if project_path.exists() {
                let raw = load_toml_value(&project_path)?;
                merge_toml_values(&mut merged, &raw);
            }
        }

        // Layer 1 (highest priority): Explicit config file from --config flag
        if let Some(cf) = config_file {
            let raw = load_toml_value(cf)?;
            merge_toml_values(&mut merged, &raw);
        }

        // Compatibility: older docs/configs may use [query] instead of [search].
        promote_query_section(&mut merged);
        // Compatibility: spec-kit 008 uses top-level [semantic].
        promote_semantic_section(&mut merged);

        // Deserialize the merged value into Config (fills remaining fields with defaults)
        let config_str =
            toml::to_string(&merged).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        let mut config: Config =
            toml::from_str(&config_str).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        // Layer 0 (highest priority): Environment variable overrides
        // Convention: CRUXE_<SECTION>_<KEY> in UPPER_SNAKE_CASE
        apply_env_overrides(&mut config);

        config.search.freshness_policy =
            normalize_freshness_policy(&config.search.freshness_policy);
        config.search.ranking_explain_level =
            normalize_ranking_explain_level(&config.search.ranking_explain_level);
        config.search.intent = config.search.intent.normalized();
        config.search.semantic.mode = normalize_semantic_mode(&config.search.semantic.mode);
        config.search.semantic.ratio = clamp_unit_f64_with_warning(
            config.search.semantic.ratio,
            default_semantic_ratio(),
            "search.semantic.ratio",
        );
        config.search.semantic.lexical_short_circuit_threshold = clamp_unit_f64_with_warning(
            config.search.semantic.lexical_short_circuit_threshold,
            default_lexical_short_circuit_threshold(),
            "search.semantic.lexical_short_circuit_threshold",
        );
        config.search.semantic.confidence_threshold = clamp_unit_f64_with_warning(
            config.search.semantic.confidence_threshold,
            default_confidence_threshold(),
            "search.semantic.confidence_threshold",
        );
        config.search.semantic.confidence_top_score_weight = clamp_non_negative_f64_with_warning(
            config.search.semantic.confidence_top_score_weight,
            default_confidence_top_score_weight(),
            "search.semantic.confidence_top_score_weight",
        );
        config.search.semantic.confidence_score_margin_weight = clamp_non_negative_f64_with_warning(
            config.search.semantic.confidence_score_margin_weight,
            default_confidence_score_margin_weight(),
            "search.semantic.confidence_score_margin_weight",
        );
        config.search.semantic.confidence_channel_agreement_weight =
            clamp_non_negative_f64_with_warning(
                config.search.semantic.confidence_channel_agreement_weight,
                default_confidence_channel_agreement_weight(),
                "search.semantic.confidence_channel_agreement_weight",
            );
        config.search.semantic.local_rerank_phrase_boost = clamp_non_negative_f64_with_warning(
            config.search.semantic.local_rerank_phrase_boost,
            default_local_rerank_phrase_boost(),
            "search.semantic.local_rerank_phrase_boost",
        );
        config.search.semantic.local_rerank_token_overlap_weight =
            clamp_non_negative_f64_with_warning(
                config.search.semantic.local_rerank_token_overlap_weight,
                default_local_rerank_token_overlap_weight(),
                "search.semantic.local_rerank_token_overlap_weight",
            );
        config.search.semantic.semantic_limit_multiplier = clamp_min_usize_with_warning(
            config.search.semantic.semantic_limit_multiplier,
            1,
            default_semantic_limit_multiplier(),
            "search.semantic.semantic_limit_multiplier",
        );
        config.search.semantic.lexical_fanout_multiplier = clamp_min_usize_with_warning(
            config.search.semantic.lexical_fanout_multiplier,
            1,
            default_lexical_fanout_multiplier(),
            "search.semantic.lexical_fanout_multiplier",
        );
        config.search.semantic.semantic_fanout_multiplier = clamp_min_usize_with_warning(
            config.search.semantic.semantic_fanout_multiplier,
            1,
            default_semantic_fanout_multiplier(),
            "search.semantic.semantic_fanout_multiplier",
        );
        config.search.semantic.embedding.profile =
            normalize_embedding_profile(&config.search.semantic.embedding.profile);
        config.search.semantic.embedding.provider =
            normalize_embedding_provider(&config.search.semantic.embedding.provider);
        if config.search.semantic.embedding.model.trim().is_empty() {
            config.search.semantic.embedding.model = default_semantic_embedding_model();
        }
        if config
            .search
            .semantic
            .embedding
            .model_version
            .trim()
            .is_empty()
        {
            config.search.semantic.embedding.model_version =
                default_semantic_embedding_model_version();
        }
        if config.search.semantic.embedding.dimensions == 0 {
            config.search.semantic.embedding.dimensions = default_semantic_embedding_dimensions();
        }
        if config.search.semantic.embedding.batch_size == 0 {
            config.search.semantic.embedding.batch_size = default_semantic_embedding_batch_size();
        }
        config.search.semantic.embedding.vector_backend =
            normalize_vector_backend(&config.search.semantic.embedding.vector_backend);
        config.search.semantic.rerank.provider =
            normalize_rerank_provider(&config.search.semantic.rerank.provider);
        if config.search.semantic.rerank.timeout_ms == 0 {
            config.search.semantic.rerank.timeout_ms = default_semantic_rerank_timeout_ms();
        }
        config.search.semantic.rerank.endpoint = config
            .search
            .semantic
            .rerank
            .endpoint
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        config.search.semantic.overrides.natural_language.ratio = config
            .search
            .semantic
            .overrides
            .natural_language
            .ratio
            .map(|v| {
                clamp_unit_f64_with_warning(
                    v,
                    default_semantic_ratio(),
                    "search.semantic.overrides.natural_language.ratio",
                )
            });
        config.search.semantic.overrides.symbol.ratio =
            config.search.semantic.overrides.symbol.ratio.map(|v| {
                clamp_unit_f64_with_warning(
                    v,
                    default_semantic_ratio(),
                    "search.semantic.overrides.symbol.ratio",
                )
            });
        config.search.semantic.overrides.path.ratio =
            config.search.semantic.overrides.path.ratio.map(|v| {
                clamp_unit_f64_with_warning(
                    v,
                    default_semantic_ratio(),
                    "search.semantic.overrides.path.ratio",
                )
            });
        config.search.semantic.overrides.error.ratio =
            config.search.semantic.overrides.error.ratio.map(|v| {
                clamp_unit_f64_with_warning(
                    v,
                    default_semantic_ratio(),
                    "search.semantic.overrides.error.ratio",
                )
            });
        if config.search.max_response_bytes == 0 {
            config.search.max_response_bytes = default_max_response_bytes();
        }
        config.search.policy.mode = normalize_policy_mode(&config.search.policy.mode);
        config.search.policy.allowed_override_modes = normalized_policy_mode_list(
            &config.search.policy.allowed_override_modes,
            default_policy_allowed_override_modes(),
        );
        config.search.policy.path.deny = normalize_non_empty_list(&config.search.policy.path.deny);
        config.search.policy.path.allow =
            normalize_non_empty_list(&config.search.policy.path.allow);
        config.search.policy.kind.deny_result_types =
            normalize_policy_result_type_list(&config.search.policy.kind.deny_result_types);
        config.search.policy.kind.allow_result_types =
            normalize_policy_result_type_list(&config.search.policy.kind.allow_result_types);
        config.search.policy.kind.deny_symbol_kinds =
            normalize_non_empty_list_lowercase(&config.search.policy.kind.deny_symbol_kinds);
        config.search.policy.kind.allow_symbol_kinds =
            normalize_non_empty_list_lowercase(&config.search.policy.kind.allow_symbol_kinds);
        config.search.policy.redaction.high_entropy_min_length = clamp_min_usize_with_warning(
            config.search.policy.redaction.high_entropy_min_length,
            8,
            default_policy_high_entropy_min_length(),
            "search.policy.redaction.high_entropy_min_length",
        );
        config.search.policy.redaction.high_entropy_threshold = clamp_range_f64_with_warning(
            config.search.policy.redaction.high_entropy_threshold,
            1.0,
            8.0,
            default_policy_high_entropy_threshold(),
            "search.policy.redaction.high_entropy_threshold",
        );
        config.search.policy.redaction.custom_rules = config
            .search
            .policy
            .redaction
            .custom_rules
            .iter()
            .filter_map(|rule| {
                let name = rule.name.trim().to_string();
                let category = rule.category.trim().to_string();
                let pattern = rule.pattern.trim().to_string();
                if name.is_empty() || category.is_empty() || pattern.is_empty() {
                    return None;
                }
                let placeholder = if rule.placeholder.trim().is_empty() {
                    default_policy_redaction_placeholder()
                } else {
                    rule.placeholder.trim().to_string()
                };
                Some(PolicyRedactionRule {
                    name,
                    category,
                    pattern,
                    placeholder,
                })
            })
            .collect();
        config.search.policy.detect_secrets.plugins =
            normalize_non_empty_list_lowercase(&config.search.policy.detect_secrets.plugins);
        config.search.policy.detect_secrets.custom_patterns =
            normalize_non_empty_list(&config.search.policy.detect_secrets.custom_patterns);
        if config.search.policy.opa.command.trim().is_empty() {
            config.search.policy.opa.command = default_policy_opa_command();
        }
        if config.search.policy.opa.query.trim().is_empty() {
            config.search.policy.opa.query = default_policy_opa_query();
        }
        config.search.policy.opa.policy_path = config
            .search
            .policy
            .opa
            .policy_path
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        // Legacy compatibility fallback.
        if config.search.ranking_explain_level == "off" && config.debug.ranking_reasons {
            config.search.ranking_explain_level = "full".to_string();
        }

        // Expand ~ in data_dir
        config.storage.data_dir = expand_tilde(&config.storage.data_dir);

        Ok(config)
    }

    /// Resolve the data directory for a project.
    pub fn project_data_dir(&self, project_id: &str) -> PathBuf {
        PathBuf::from(&self.storage.data_dir)
            .join("data")
            .join(project_id)
    }
}

/// Load a TOML file as a raw `toml::Value` (preserving only explicitly-set fields).
fn load_toml_value(path: &Path) -> Result<toml::Value, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    content
        .parse::<toml::Value>()
        .map_err(|e| ConfigError::ParseError(e.to_string()))
}

/// Deep-merge `overlay` into `base`. Only keys present in `overlay` are written.
fn merge_toml_values(base: &mut toml::Value, overlay: &toml::Value) {
    if let (toml::Value::Table(base_map), toml::Value::Table(overlay_map)) = (base, overlay) {
        for (key, overlay_val) in overlay_map {
            if let Some(base_val) = base_map.get_mut(key) {
                // Both have this key — recurse if both are tables, otherwise overwrite
                if base_val.is_table() && overlay_val.is_table() {
                    merge_toml_values(base_val, overlay_val);
                } else {
                    *base_val = overlay_val.clone();
                }
            } else {
                base_map.insert(key.clone(), overlay_val.clone());
            }
        }
    }
}

/// Apply environment variable overrides to config fields.
/// Convention: `CRUXE_<SECTION>_<KEY>` in UPPER_SNAKE_CASE.
fn apply_env_overrides(config: &mut Config) {
    if let Ok(v) = std::env::var("CRUXE_STORAGE_DATA_DIR") {
        config.storage.data_dir = v;
    }
    if let Ok(v) = std::env::var("CRUXE_STORAGE_BUSY_TIMEOUT_MS")
        && let Ok(n) = v.parse()
    {
        config.storage.busy_timeout_ms = n;
    }
    if let Ok(v) = std::env::var("CRUXE_STORAGE_CACHE_SIZE")
        && let Ok(n) = v.parse()
    {
        config.storage.cache_size = n;
    }
    if let Ok(v) = std::env::var("CRUXE_INDEX_MAX_FILE_SIZE")
        && let Ok(n) = v.parse()
    {
        config.index.max_file_size = n;
    }
    if let Ok(v) = std::env::var("CRUXE_INDEX_DEFAULT_LIMIT")
        && let Ok(n) = v.parse()
    {
        config.index.default_limit = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_DEFAULT_REF") {
        config.search.default_ref = v;
    }
    if let Ok(v) = std::env::var("CRUXE_LOGGING_LEVEL") {
        config.logging.level = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_FRESHNESS_POLICY") {
        config.search.freshness_policy = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_RANKING_EXPLAIN_LEVEL") {
        config.search.ranking_explain_level = v;
    } else if let Ok(v) = std::env::var("CRUXE_QUERY_RANKING_EXPLAIN_LEVEL") {
        config.search.ranking_explain_level = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_MAX_RESPONSE_BYTES")
        && let Ok(n) = v.parse()
    {
        config.search.max_response_bytes = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_MODE") {
        config.search.policy.mode = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_ALLOW_REQUEST_OVERRIDE")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.policy.allow_request_override = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_ALLOWED_OVERRIDE_MODES") {
        config.search.policy.allowed_override_modes = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_PATH_DENY") {
        config.search.policy.path.deny = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_PATH_ALLOW") {
        config.search.policy.path.allow = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_KIND_DENY_RESULT_TYPES") {
        config.search.policy.kind.deny_result_types = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_KIND_ALLOW_RESULT_TYPES") {
        config.search.policy.kind.allow_result_types = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_KIND_DENY_SYMBOL_KINDS") {
        config.search.policy.kind.deny_symbol_kinds = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_KIND_ALLOW_SYMBOL_KINDS") {
        config.search.policy.kind.allow_symbol_kinds = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_REDACTION_ENABLED")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.policy.redaction.enabled = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_REDACTION_EMAIL_MASKING")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.policy.redaction.email_masking = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_REDACTION_HIGH_ENTROPY_MIN_LENGTH")
        && let Ok(n) = v.parse()
    {
        config.search.policy.redaction.high_entropy_min_length = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_REDACTION_HIGH_ENTROPY_THRESHOLD")
        && let Ok(n) = v.parse()
    {
        config.search.policy.redaction.high_entropy_threshold = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_DETECT_SECRETS_ENABLED")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.policy.detect_secrets.enabled = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_DETECT_SECRETS_PLUGINS") {
        config.search.policy.detect_secrets.plugins = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_DETECT_SECRETS_CUSTOM_PATTERNS") {
        config.search.policy.detect_secrets.custom_patterns = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_OPA_ENABLED")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.policy.opa.enabled = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_OPA_COMMAND") {
        config.search.policy.opa.command = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_OPA_QUERY") {
        config.search.policy.opa.query = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_POLICY_OPA_POLICY_PATH") {
        config.search.policy.opa.policy_path = Some(v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_INTENT_RULE_ORDER") {
        config.search.intent.rule_order = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_INTENT_ERROR_PATTERNS") {
        config.search.intent.error_patterns = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_INTENT_PATH_EXTENSIONS") {
        config.search.intent.path_extensions = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_INTENT_SYMBOL_KIND_KEYWORDS") {
        config.search.intent.symbol_kind_keywords = parse_csv_env_list(&v);
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_INTENT_ENABLE_WRAPPED_QUOTED_ERROR_LITERAL")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.intent.enable_wrapped_quoted_error_literal = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_MODE") {
        config.search.semantic.mode = v;
    } else if let Ok(v) = std::env::var("CRUXE_SEARCH_SEMANTIC_MODE") {
        config.search.semantic.mode = v;
    } else if let Ok(v) = std::env::var("CRUXE_QUERY_SEMANTIC_MODE") {
        config.search.semantic.mode = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_RATIO")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.ratio = n;
    } else if let Ok(v) = std::env::var("CRUXE_SEARCH_SEMANTIC_RATIO")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.ratio = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_CONFIDENCE_THRESHOLD")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.confidence_threshold = n;
    } else if let Ok(v) = std::env::var("CRUXE_SEARCH_CONFIDENCE_THRESHOLD")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.confidence_threshold = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_CONFIDENCE_TOP_SCORE_WEIGHT")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.confidence_top_score_weight = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_CONFIDENCE_SCORE_MARGIN_WEIGHT")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.confidence_score_margin_weight = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_CONFIDENCE_CHANNEL_AGREEMENT_WEIGHT")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.confidence_channel_agreement_weight = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_LOCAL_RERANK_PHRASE_BOOST")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.local_rerank_phrase_boost = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_LOCAL_RERANK_TOKEN_OVERLAP_WEIGHT")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.local_rerank_token_overlap_weight = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_LIMIT_MULTIPLIER")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.semantic_limit_multiplier = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_LEXICAL_FANOUT_MULTIPLIER")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.lexical_fanout_multiplier = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_SEMANTIC_FANOUT_MULTIPLIER")
        && let Ok(n) = v.parse()
    {
        config.search.semantic.semantic_fanout_multiplier = n;
    }
    if let Ok(v) = std::env::var("CRUXE_SEARCH_SEMANTIC_PROFILE") {
        config.search.semantic.embedding.profile = v;
    } else if let Ok(v) = std::env::var("CRUXE_SEMANTIC_EMBEDDING_PROFILE") {
        config.search.semantic.embedding.profile = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_VECTOR_BACKEND") {
        config.search.semantic.embedding.vector_backend = v;
    } else if let Ok(v) = std::env::var("CRUXE_SEARCH_SEMANTIC_VECTOR_BACKEND") {
        config.search.semantic.embedding.vector_backend = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_RERANK_PROVIDER") {
        config.search.semantic.rerank.provider = v;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_EXTERNAL_PROVIDER_ENABLED")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.semantic.external_provider_enabled = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_SEMANTIC_ALLOW_CODE_PAYLOAD_TO_EXTERNAL")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.search.semantic.allow_code_payload_to_external = parsed;
    }
    if let Ok(v) = std::env::var("CRUXE_DEBUG_RANKING_REASONS")
        && let Some(parsed) = parse_env_bool(&v)
    {
        config.debug.ranking_reasons = parsed;
    }
}

fn parse_csv_env_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_env_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn promote_query_section(merged: &mut toml::Value) {
    let Some(root) = merged.as_table_mut() else {
        return;
    };

    let Some(query_table) = root.get("query").and_then(|v| v.as_table()).cloned() else {
        return;
    };

    let search_value = root
        .entry("search")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(search_table) = search_value.as_table_mut() else {
        return;
    };

    for key in [
        "default_ref",
        "freshness_policy",
        "ranking_explain_level",
        "max_response_bytes",
    ] {
        if !search_table.contains_key(key)
            && let Some(value) = query_table.get(key)
        {
            search_table.insert(key.to_string(), value.clone());
        }
    }
}

fn promote_semantic_section(merged: &mut toml::Value) {
    let Some(root) = merged.as_table_mut() else {
        return;
    };

    let Some(semantic_value) = root.get("semantic").cloned() else {
        return;
    };

    let search_value = root
        .entry("search")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(search_table) = search_value.as_table_mut() else {
        return;
    };

    let search_semantic = search_table
        .entry("semantic")
        .or_insert_with(|| semantic_value.clone());
    merge_missing_toml_values(search_semantic, &semantic_value);
}

fn merge_missing_toml_values(base: &mut toml::Value, overlay: &toml::Value) {
    if let (toml::Value::Table(base_map), toml::Value::Table(overlay_map)) = (base, overlay) {
        for (key, overlay_val) in overlay_map {
            if let Some(base_val) = base_map.get_mut(key) {
                if base_val.is_table() && overlay_val.is_table() {
                    merge_missing_toml_values(base_val, overlay_val);
                }
            } else {
                base_map.insert(key.clone(), overlay_val.clone());
            }
        }
    }
}

fn parse_freshness_policy(raw: &str) -> Option<FreshnessPolicy> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "strict" => Some(FreshnessPolicy::Strict),
        "balanced" => Some(FreshnessPolicy::Balanced),
        "best_effort" | "besteffort" => Some(FreshnessPolicy::BestEffort),
        _ => None,
    }
}

fn freshness_policy_to_str(policy: FreshnessPolicy) -> &'static str {
    match policy {
        FreshnessPolicy::Strict => "strict",
        FreshnessPolicy::Balanced => "balanced",
        FreshnessPolicy::BestEffort => "best_effort",
    }
}

fn normalize_freshness_policy(raw: &str) -> String {
    let policy = parse_freshness_policy(raw).unwrap_or(FreshnessPolicy::Balanced);
    freshness_policy_to_str(policy).to_string()
}

fn parse_ranking_explain_level(raw: &str) -> Option<RankingExplainLevel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(RankingExplainLevel::Off),
        "basic" => Some(RankingExplainLevel::Basic),
        "full" => Some(RankingExplainLevel::Full),
        _ => None,
    }
}

fn ranking_explain_level_to_str(level: RankingExplainLevel) -> &'static str {
    match level {
        RankingExplainLevel::Off => "off",
        RankingExplainLevel::Basic => "basic",
        RankingExplainLevel::Full => "full",
    }
}

fn normalize_ranking_explain_level(raw: &str) -> String {
    let level = parse_ranking_explain_level(raw).unwrap_or(RankingExplainLevel::Off);
    ranking_explain_level_to_str(level).to_string()
}

fn parse_semantic_mode(raw: &str) -> Option<SemanticMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(SemanticMode::Off),
        "rerank_only" | "rerankonly" | "shadow" => Some(SemanticMode::RerankOnly),
        "hybrid" | "on" | "enabled" => Some(SemanticMode::Hybrid),
        _ => None,
    }
}

fn semantic_mode_to_str(mode: SemanticMode) -> &'static str {
    match mode {
        SemanticMode::Off => "off",
        SemanticMode::RerankOnly => "rerank_only",
        SemanticMode::Hybrid => "hybrid",
    }
}

fn normalize_semantic_mode(raw: &str) -> String {
    let mode = parse_semantic_mode(raw).unwrap_or(SemanticMode::Off);
    semantic_mode_to_str(mode).to_string()
}

fn parse_policy_mode(raw: &str) -> Option<PolicyMode> {
    raw.parse::<PolicyMode>().ok()
}

fn policy_mode_to_str(mode: PolicyMode) -> &'static str {
    mode.as_str()
}

fn normalize_policy_mode(raw: &str) -> String {
    let mode = parse_policy_mode(raw).unwrap_or(PolicyMode::Balanced);
    policy_mode_to_str(mode).to_string()
}

fn normalized_policy_mode_list(values: &[String], fallback: Vec<String>) -> Vec<String> {
    let mut out = values
        .iter()
        .filter_map(|value| parse_policy_mode(value).map(policy_mode_to_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    out.sort();
    out.dedup();
    if out.is_empty() { fallback } else { out }
}

fn normalize_non_empty_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalize_non_empty_list_lowercase(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn normalize_policy_result_type_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter_map(|value| match value.as_str() {
            "symbol" | "snippet" | "file" => Some(value),
            _ => None,
        })
        .collect()
}

fn normalize_embedding_profile(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fast_local" => "fast_local".to_string(),
        "code_quality" => "code_quality".to_string(),
        "high_quality" => "high_quality".to_string(),
        "external" => "external".to_string(),
        _ => default_semantic_embedding_profile(),
    }
}

fn normalize_embedding_provider(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "local" => "local".to_string(),
        "voyage" => "voyage".to_string(),
        "openai" => "openai".to_string(),
        _ => default_semantic_embedding_provider(),
    }
}

fn normalize_rerank_provider(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" => "none".to_string(),
        "cohere" => "cohere".to_string(),
        "voyage" => "voyage".to_string(),
        _ => default_semantic_rerank_provider(),
    }
}

fn normalize_vector_backend(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "lancedb" | "lance" => "lancedb".to_string(),
        "sqlite" | "" => "sqlite".to_string(),
        _ => default_semantic_vector_backend(),
    }
}

/// Normalize user-provided intent rule names/aliases into canonical identifiers.
pub fn canonical_intent_rule_name(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "error_pattern" | "error" => Some("error_pattern"),
        "path" => Some("path"),
        "quoted_error" | "quoted" => Some("quoted_error"),
        "symbol" => Some("symbol"),
        "natural_language" | "nl" | "default" => Some("natural_language"),
        _ => None,
    }
}

fn normalize_intent_rule_order(raw: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in raw {
        let Some(rule) = canonical_intent_rule_name(value) else {
            continue;
        };
        if !normalized.iter().any(|existing| existing == rule) {
            normalized.push(rule.to_string());
        }
    }

    if normalized.is_empty() {
        return default_intent_rule_order();
    }
    if !normalized.iter().any(|rule| rule == "natural_language") {
        normalized.push("natural_language".to_string());
    }
    normalized
}

fn normalize_intent_error_patterns(raw: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in raw.iter().map(|value| value.trim()) {
        if value.is_empty() {
            continue;
        }
        let value = value.to_string();
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        default_intent_error_patterns()
    } else {
        normalized
    }
}

fn normalize_intent_path_extensions(raw: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in raw
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        let value = if value.starts_with('.') {
            value
        } else {
            format!(".{value}")
        };
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        default_intent_path_extensions()
    } else {
        normalized
    }
}

fn normalize_intent_symbol_kind_keywords(raw: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in raw
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        default_intent_symbol_kind_keywords()
    } else {
        normalized
    }
}

fn clamp_unit_f64_with_warning(value: f64, fallback: f64, field: &str) -> f64 {
    if !value.is_finite() {
        tracing::warn!(
            field,
            value,
            fallback,
            "invalid non-finite config value; falling back to default"
        );
        return fallback;
    }
    let clamped = value.clamp(0.0, 1.0);
    if (clamped - value).abs() > f64::EPSILON {
        tracing::warn!(
            field,
            value,
            clamped,
            "config value out of range; clamped to [0.0, 1.0]"
        );
    }
    clamped
}

fn clamp_non_negative_f64_with_warning(value: f64, fallback: f64, field: &str) -> f64 {
    if !value.is_finite() {
        tracing::warn!(
            field,
            value,
            fallback,
            "invalid non-finite config value; falling back to default"
        );
        return fallback;
    }
    if value < 0.0 {
        tracing::warn!(
            field,
            value,
            fallback,
            "config value below 0.0; falling back to default"
        );
        return fallback;
    }
    value
}

fn clamp_range_f64_with_warning(value: f64, min: f64, max: f64, fallback: f64, field: &str) -> f64 {
    if !value.is_finite() {
        tracing::warn!(
            field,
            value,
            fallback,
            "invalid non-finite config value; falling back to default"
        );
        return fallback;
    }
    if value < min || value > max {
        tracing::warn!(
            field,
            value,
            min,
            max,
            fallback,
            "config value out of range; falling back to default"
        );
        return fallback;
    }
    value
}

fn clamp_min_usize_with_warning(value: usize, min: usize, fallback: usize, field: &str) -> usize {
    if value < min {
        tracing::warn!(
            field,
            value,
            min,
            fallback,
            "config value below minimum; falling back to default"
        );
        fallback
    } else {
        value
    }
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with('~')
        && let Some(home) = dirs::home_dir()
    {
        return path.replacen('~', &home.to_string_lossy(), 1);
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_freshness_policy_values() {
        assert_eq!(normalize_freshness_policy("strict"), "strict");
        assert_eq!(normalize_freshness_policy("BALANCED"), "balanced");
        assert_eq!(normalize_freshness_policy("bestEffort"), "best_effort");
        assert_eq!(normalize_freshness_policy("unknown"), "balanced");
    }

    #[test]
    fn normalize_ranking_explain_level_values() {
        assert_eq!(normalize_ranking_explain_level("off"), "off");
        assert_eq!(normalize_ranking_explain_level("BASIC"), "basic");
        assert_eq!(normalize_ranking_explain_level("full"), "full");
        assert_eq!(normalize_ranking_explain_level("unknown"), "off");
    }

    #[test]
    fn normalize_semantic_mode_values() {
        assert_eq!(normalize_semantic_mode("off"), "off");
        assert_eq!(normalize_semantic_mode("shadow"), "rerank_only");
        assert_eq!(normalize_semantic_mode("ENABLED"), "hybrid");
        assert_eq!(normalize_semantic_mode("unknown"), "off");
    }

    #[test]
    fn normalize_policy_mode_values() {
        assert_eq!(normalize_policy_mode("strict"), "strict");
        assert_eq!(normalize_policy_mode("BALANCED"), "balanced");
        assert_eq!(normalize_policy_mode("audit"), "audit_only");
        assert_eq!(normalize_policy_mode("unknown"), "balanced");
    }

    #[test]
    fn search_policy_override_resolution_respects_allowlist() {
        let mut config = SearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.allow_request_override = true;
        config.policy.allowed_override_modes = vec!["off".to_string(), "audit_only".to_string()];

        let (effective, warnings) = config
            .resolve_policy_mode(Some(PolicyMode::AuditOnly))
            .expect("audit_only should be allowed");
        assert_eq!(effective, PolicyMode::AuditOnly);
        assert!(!warnings.is_empty());

        let err = config
            .resolve_policy_mode(Some(PolicyMode::Strict))
            .expect_err("strict should be denied by allowlist");
        assert!(err.contains("not allowed"));
    }

    #[test]
    fn normalize_semantic_provider_values() {
        assert_eq!(normalize_embedding_profile("high_quality"), "high_quality");
        assert_eq!(normalize_embedding_profile("unknown"), "fast_local");
        assert_eq!(normalize_embedding_provider("VOYAGE"), "voyage");
        assert_eq!(normalize_embedding_provider(""), "local");
        assert_eq!(normalize_rerank_provider("cohere"), "cohere");
        assert_eq!(normalize_rerank_provider("oops"), "none");
    }

    #[test]
    fn normalize_intent_policy_values() {
        assert_eq!(
            normalize_intent_rule_order(&[
                "path".to_string(),
                "unknown".to_string(),
                "path".to_string(),
            ]),
            vec!["path".to_string(), "natural_language".to_string()]
        );
        assert_eq!(
            normalize_intent_error_patterns(&[
                "".to_string(),
                "panic payload".to_string(),
                "panic payload".to_string(),
            ]),
            vec!["panic payload".to_string()]
        );
        assert_eq!(
            normalize_intent_path_extensions(&[
                "rs".to_string(),
                " .KT ".to_string(),
                "rs".to_string(),
                "".to_string(),
            ]),
            vec![".rs".to_string(), ".kt".to_string()]
        );
        assert_eq!(
            normalize_intent_symbol_kind_keywords(&[
                "Fn".to_string(),
                "".to_string(),
                "METHOD".to_string(),
                "fn".to_string(),
            ]),
            vec!["fn".to_string(), "method".to_string()]
        );
    }

    #[test]
    fn parse_env_bool_supports_common_truthy_falsey_values() {
        assert_eq!(parse_env_bool("true"), Some(true));
        assert_eq!(parse_env_bool("TRUE"), Some(true));
        assert_eq!(parse_env_bool(" yes "), Some(true));
        assert_eq!(parse_env_bool("on"), Some(true));
        assert_eq!(parse_env_bool("1"), Some(true));

        assert_eq!(parse_env_bool("false"), Some(false));
        assert_eq!(parse_env_bool("FALSE"), Some(false));
        assert_eq!(parse_env_bool(" no "), Some(false));
        assert_eq!(parse_env_bool("off"), Some(false));
        assert_eq!(parse_env_bool("0"), Some(false));

        assert_eq!(parse_env_bool("maybe"), None);
    }

    #[test]
    fn search_intent_config_normalized_produces_canonical_values() {
        let raw = SearchIntentConfig {
            rule_order: vec![
                " PATH ".to_string(),
                "path".to_string(),
                "unknown".to_string(),
            ],
            error_patterns: vec![
                "".to_string(),
                "panic payload".to_string(),
                "panic payload".to_string(),
            ],
            path_extensions: vec!["rs".to_string(), " .RS ".to_string()],
            symbol_kind_keywords: vec!["Fn".to_string(), "fn".to_string(), "METHOD".to_string()],
            enable_wrapped_quoted_error_literal: false,
        };

        let normalized = raw.normalized();
        assert_eq!(
            normalized.rule_order,
            vec!["path".to_string(), "natural_language".to_string()]
        );
        assert_eq!(normalized.error_patterns, vec!["panic payload".to_string()]);
        assert_eq!(normalized.path_extensions, vec![".rs".to_string()]);
        assert_eq!(
            normalized.symbol_kind_keywords,
            vec!["fn".to_string(), "method".to_string()]
        );
        assert!(!normalized.enable_wrapped_quoted_error_literal);
    }

    #[test]
    fn promote_query_section_copies_missing_fields() {
        let mut merged: toml::Value = toml::from_str(
            r#"
            [query]
            freshness_policy = "strict"
            ranking_explain_level = "basic"
            max_response_bytes = 2048
            "#,
        )
        .unwrap();

        promote_query_section(&mut merged);
        let search = merged.get("search").and_then(|v| v.as_table()).unwrap();
        assert_eq!(
            search
                .get("freshness_policy")
                .and_then(|v| v.as_str())
                .unwrap(),
            "strict"
        );
        assert_eq!(
            search
                .get("ranking_explain_level")
                .and_then(|v| v.as_str())
                .unwrap(),
            "basic"
        );
        assert_eq!(
            search
                .get("max_response_bytes")
                .and_then(|v| v.as_integer())
                .unwrap(),
            2048
        );
    }

    #[test]
    fn promote_semantic_section_copies_missing_fields() {
        let mut merged: toml::Value = toml::from_str(
            r#"
            [search]
            default_ref = "main"

            [semantic]
            mode = "hybrid"
            ratio = 0.7

            [semantic.rerank]
            provider = "cohere"
            timeout_ms = 1500
            "#,
        )
        .unwrap();

        promote_semantic_section(&mut merged);
        let semantic = merged
            .get("search")
            .and_then(|v| v.get("semantic"))
            .and_then(|v| v.as_table())
            .unwrap();
        assert_eq!(
            semantic.get("mode").and_then(|v| v.as_str()),
            Some("hybrid")
        );
        assert_eq!(semantic.get("ratio").and_then(|v| v.as_float()), Some(0.7));
        let rerank = semantic.get("rerank").and_then(|v| v.as_table()).unwrap();
        assert_eq!(
            rerank.get("provider").and_then(|v| v.as_str()),
            Some("cohere")
        );
    }

    #[test]
    fn load_with_file_normalizes_invalid_values_clamps_ratios_and_legacy_debug_flag() {
        let temp = tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
            [search]
            freshness_policy = "invalid"
            ranking_explain_level = "verbose"
            max_response_bytes = 0

            [search.semantic]
            mode = "future_mode"
            ratio = 9.0
            lexical_short_circuit_threshold = -1.0
            confidence_threshold = 2.0
            confidence_top_score_weight = -0.2
            confidence_score_margin_weight = -0.3
            confidence_channel_agreement_weight = -0.4
            local_rerank_phrase_boost = -1.0
            local_rerank_token_overlap_weight = -2.0
            semantic_limit_multiplier = 0
            lexical_fanout_multiplier = 0
            semantic_fanout_multiplier = 0

            [search.semantic.embedding]
            profile = ""
            provider = "unknown"
            model = ""
            model_version = ""
            dimensions = 0
            batch_size = 0

            [search.semantic.rerank]
            provider = "invalid"
            timeout_ms = 0

            [debug]
            ranking_reasons = true
            "#,
        )
        .unwrap();

        let loaded = Config::load_with_file(None, Some(&config_path)).unwrap();
        assert_eq!(loaded.search.freshness_policy, "balanced");
        assert_eq!(loaded.search.ranking_explain_level, "full");
        assert_eq!(loaded.search.max_response_bytes, 64 * 1024);
        assert_eq!(loaded.search.semantic.mode, "off");
        assert_eq!(loaded.search.semantic.ratio, 1.0);
        assert_eq!(loaded.search.semantic.lexical_short_circuit_threshold, 0.0);
        assert_eq!(loaded.search.semantic.confidence_threshold, 1.0);
        assert_eq!(loaded.search.semantic.confidence_top_score_weight, 0.55);
        assert_eq!(loaded.search.semantic.confidence_score_margin_weight, 0.30);
        assert_eq!(
            loaded.search.semantic.confidence_channel_agreement_weight,
            0.15
        );
        assert_eq!(loaded.search.semantic.local_rerank_phrase_boost, 0.75);
        assert_eq!(
            loaded.search.semantic.local_rerank_token_overlap_weight,
            2.5
        );
        assert_eq!(loaded.search.semantic.semantic_limit_multiplier, 2);
        assert_eq!(loaded.search.semantic.lexical_fanout_multiplier, 4);
        assert_eq!(loaded.search.semantic.semantic_fanout_multiplier, 3);
        assert_eq!(loaded.search.semantic.embedding.profile, "fast_local");
        assert_eq!(loaded.search.semantic.embedding.provider, "local");
        assert_eq!(loaded.search.semantic.embedding.model, "NomicEmbedTextV15Q");
        assert_eq!(
            loaded.search.semantic.embedding.model_version,
            "fastembed-1"
        );
        assert_eq!(loaded.search.semantic.embedding.dimensions, 768);
        assert_eq!(loaded.search.semantic.embedding.batch_size, 32);
        assert_eq!(loaded.search.semantic.rerank.provider, "none");
        assert_eq!(loaded.search.semantic.rerank.timeout_ms, 5000);
        assert_eq!(
            loaded.search.freshness_policy_typed(),
            FreshnessPolicy::Balanced
        );
        assert_eq!(
            loaded.search.ranking_explain_level_typed(),
            RankingExplainLevel::Full
        );
        assert_eq!(loaded.search.semantic_mode_typed(), SemanticMode::Off);
    }

    #[test]
    fn semantic_ratio_override_precedence_is_request_then_intent_then_default() {
        let mut cfg = SearchConfig::default();
        cfg.semantic.ratio = 0.3;
        cfg.semantic.overrides.natural_language.ratio = Some(0.6);

        assert_eq!(
            cfg.semantic_ratio_for_intent(QueryIntent::NaturalLanguage, None),
            0.6
        );
        assert_eq!(
            cfg.semantic_ratio_for_intent(QueryIntent::NaturalLanguage, Some(0.9)),
            0.9
        );
        assert_eq!(cfg.semantic_ratio_for_intent(QueryIntent::Path, None), 0.3);
    }

    #[test]
    fn semantic_external_provider_gate_requires_both_flags() {
        let semantic = SemanticConfig {
            external_provider_enabled: true,
            allow_code_payload_to_external: false,
            ..Default::default()
        };
        assert!(!semantic.allow_external_provider_calls());
        let semantic = SemanticConfig {
            external_provider_enabled: true,
            allow_code_payload_to_external: true,
            ..Default::default()
        };
        assert!(semantic.allow_external_provider_calls());
    }

    #[test]
    fn semantic_tuning_defaults_match_legacy_constants() {
        let semantic = SemanticConfig::default();
        assert_eq!(semantic.confidence_top_score_weight, 0.55);
        assert_eq!(semantic.confidence_score_margin_weight, 0.30);
        assert_eq!(semantic.confidence_channel_agreement_weight, 0.15);
        assert_eq!(semantic.local_rerank_phrase_boost, 0.75);
        assert_eq!(semantic.local_rerank_token_overlap_weight, 2.5);
        assert_eq!(semantic.semantic_limit_multiplier, 2);
        assert_eq!(semantic.lexical_fanout_multiplier, 4);
        assert_eq!(semantic.semantic_fanout_multiplier, 3);
    }

    #[test]
    fn load_with_file_normalizes_intent_config_fields() {
        let temp = tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            r#"
            [search.intent]
            rule_order = ["path", "unknown", "path"]
            error_patterns = ["", "panic payload"]
            path_extensions = ["rs", " .kt "]
            symbol_kind_keywords = ["Fn", "", "METHOD"]
            enable_wrapped_quoted_error_literal = false
            "#,
        )
        .unwrap();

        let loaded = Config::load_with_file(None, Some(&config_path)).unwrap();
        assert_eq!(
            loaded.search.intent.rule_order,
            vec!["path".to_string(), "natural_language".to_string()]
        );
        assert_eq!(
            loaded.search.intent.error_patterns,
            vec!["panic payload".to_string()]
        );
        assert_eq!(
            loaded.search.intent.path_extensions,
            vec![".rs".to_string(), ".kt".to_string()]
        );
        assert_eq!(
            loaded.search.intent.symbol_kind_keywords,
            vec!["fn".to_string(), "method".to_string()]
        );
        assert!(!loaded.search.intent.enable_wrapped_quoted_error_literal);
    }
}
