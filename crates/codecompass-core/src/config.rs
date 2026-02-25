use crate::constants;
use crate::error::ConfigError;
use crate::types::{FreshnessPolicy, RankingExplainLevel, SemanticMode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    pub semantic: SemanticConfig,
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
pub struct SemanticConfig {
    #[serde(default = "default_semantic_mode")]
    pub semantic_mode: String,
    #[serde(default = "default_semantic_profile")]
    pub profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, SemanticProfileOverrides>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticProfileOverrides {
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(default)]
    pub max_candidates: Option<usize>,
    #[serde(default)]
    pub blend_weight: Option<f64>,
}

fn default_max_file_size() -> u64 {
    constants::MAX_FILE_SIZE
}
fn default_limit() -> usize {
    constants::DEFAULT_LIMIT
}
fn default_languages() -> Vec<String> {
    vec![
        "rust".into(),
        "typescript".into(),
        "python".into(),
        "go".into(),
    ]
}
fn default_data_dir() -> String {
    "~/.codecompass".into()
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
fn default_semantic_mode() -> String {
    "off".into()
}
fn default_semantic_profile() -> String {
    "default".into()
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
            semantic: SemanticConfig::default(),
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

impl Default for SemanticConfig {
    fn default() -> Self {
        Self {
            semantic_mode: default_semantic_mode(),
            profile: default_semantic_profile(),
            profiles: BTreeMap::new(),
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
        parse_semantic_mode(&self.semantic.semantic_mode).unwrap_or(SemanticMode::Off)
    }

    pub fn semantic_profile_overrides(&self) -> Option<&SemanticProfileOverrides> {
        self.semantic.profiles.get(&self.semantic.profile)
    }
}

impl Config {
    /// Load configuration with three-layer precedence:
    /// 1. Explicit config file (from `--config` flag, highest priority)
    /// 2. Project config: `<repo_root>/.codecompass/config.toml`
    /// 3. Global config: `~/.codecompass/config.toml`
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

        // Deserialize the merged value into Config (fills remaining fields with defaults)
        let config_str =
            toml::to_string(&merged).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        let mut config: Config =
            toml::from_str(&config_str).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        // Layer 0 (highest priority): Environment variable overrides
        // Convention: CODECOMPASS_<SECTION>_<KEY> in UPPER_SNAKE_CASE
        apply_env_overrides(&mut config);

        config.search.freshness_policy =
            normalize_freshness_policy(&config.search.freshness_policy);
        config.search.ranking_explain_level =
            normalize_ranking_explain_level(&config.search.ranking_explain_level);
        config.search.semantic.semantic_mode =
            normalize_semantic_mode(&config.search.semantic.semantic_mode);
        if config.search.semantic.profile.trim().is_empty() {
            config.search.semantic.profile = default_semantic_profile();
        }
        if config.search.max_response_bytes == 0 {
            config.search.max_response_bytes = default_max_response_bytes();
        }

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
/// Convention: `CODECOMPASS_<SECTION>_<KEY>` in UPPER_SNAKE_CASE.
fn apply_env_overrides(config: &mut Config) {
    if let Ok(v) = std::env::var("CODECOMPASS_STORAGE_DATA_DIR") {
        config.storage.data_dir = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_STORAGE_BUSY_TIMEOUT_MS")
        && let Ok(n) = v.parse()
    {
        config.storage.busy_timeout_ms = n;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_STORAGE_CACHE_SIZE")
        && let Ok(n) = v.parse()
    {
        config.storage.cache_size = n;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_INDEX_MAX_FILE_SIZE")
        && let Ok(n) = v.parse()
    {
        config.index.max_file_size = n;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_INDEX_DEFAULT_LIMIT")
        && let Ok(n) = v.parse()
    {
        config.index.default_limit = n;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_DEFAULT_REF") {
        config.search.default_ref = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_LOGGING_LEVEL") {
        config.logging.level = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_FRESHNESS_POLICY") {
        config.search.freshness_policy = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_RANKING_EXPLAIN_LEVEL") {
        config.search.ranking_explain_level = v;
    } else if let Ok(v) = std::env::var("CODECOMPASS_QUERY_RANKING_EXPLAIN_LEVEL") {
        config.search.ranking_explain_level = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_MAX_RESPONSE_BYTES")
        && let Ok(n) = v.parse()
    {
        config.search.max_response_bytes = n;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_SEMANTIC_MODE") {
        config.search.semantic.semantic_mode = v;
    } else if let Ok(v) = std::env::var("CODECOMPASS_QUERY_SEMANTIC_MODE") {
        config.search.semantic.semantic_mode = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_SEARCH_SEMANTIC_PROFILE") {
        config.search.semantic.profile = v;
    }
    if let Ok(v) = std::env::var("CODECOMPASS_DEBUG_RANKING_REASONS") {
        config.debug.ranking_reasons = v == "true" || v == "1";
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
        "shadow" => Some(SemanticMode::Shadow),
        "on" | "enabled" => Some(SemanticMode::On),
        _ => None,
    }
}

fn semantic_mode_to_str(mode: SemanticMode) -> &'static str {
    match mode {
        SemanticMode::Off => "off",
        SemanticMode::Shadow => "shadow",
        SemanticMode::On => "on",
    }
}

fn normalize_semantic_mode(raw: &str) -> String {
    let mode = parse_semantic_mode(raw).unwrap_or(SemanticMode::Off);
    semantic_mode_to_str(mode).to_string()
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
        assert_eq!(normalize_semantic_mode("shadow"), "shadow");
        assert_eq!(normalize_semantic_mode("ENABLED"), "on");
        assert_eq!(normalize_semantic_mode("unknown"), "off");
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
    fn load_with_file_normalizes_invalid_values_and_legacy_debug_flag() {
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
            semantic_mode = "future_mode"
            profile = ""

            [debug]
            ranking_reasons = true
            "#,
        )
        .unwrap();

        let loaded = Config::load_with_file(None, Some(&config_path)).unwrap();
        assert_eq!(loaded.search.freshness_policy, "balanced");
        assert_eq!(loaded.search.ranking_explain_level, "full");
        assert_eq!(loaded.search.max_response_bytes, 64 * 1024);
        assert_eq!(loaded.search.semantic.semantic_mode, "off");
        assert_eq!(loaded.search.semantic.profile, "default");
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
    fn semantic_profile_override_lookup_uses_selected_profile() {
        let mut cfg = SearchConfig::default();
        cfg.semantic.profile = "high_recall".to_string();
        cfg.semantic.profiles.insert(
            "high_recall".to_string(),
            SemanticProfileOverrides {
                min_score: Some(0.12),
                max_candidates: Some(128),
                blend_weight: Some(0.7),
            },
        );
        let selected = cfg.semantic_profile_overrides().unwrap();
        assert_eq!(selected.max_candidates, Some(128));
        assert_eq!(selected.min_score, Some(0.12));
    }
}
