use crate::search::SearchResult;
use cruxe_core::config::{
    OpaPolicyConfig, PolicyRedactionRule, RetrievalPolicyConfig, SearchConfig as CoreSearchConfig,
};
use cruxe_core::error::StateError;
use cruxe_core::types::PolicyMode;
use globset::{Glob, GlobMatcher};
use regex::Regex;
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PolicyRuntime {
    mode: PolicyMode,
    audit_only: bool,
    path_deny: Vec<GlobMatcher>,
    path_allow: Vec<GlobMatcher>,
    deny_result_types: HashSet<String>,
    allow_result_types: HashSet<String>,
    deny_symbol_kinds: HashSet<String>,
    allow_symbol_kinds: HashSet<String>,
    redaction_enabled: bool,
    redaction_rules: Vec<CompiledRedactionRule>,
    redaction_categories: BTreeMap<String, usize>,
    high_entropy_min_length: usize,
    high_entropy_threshold: f64,
    warnings: Vec<String>,
    opa: Option<OpaPolicyRuntime>,
}

#[derive(Debug, Clone)]
pub struct PolicyApplication {
    pub mode: PolicyMode,
    pub results: Vec<SearchResult>,
    pub blocked_count: usize,
    pub redacted_count: usize,
    pub warnings: Vec<String>,
    pub audit_counts: BTreeMap<String, usize>,
    pub active_redaction_categories: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct TextRedactionResult {
    pub text: String,
    pub redacted_count: usize,
    pub category_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct CompiledRedactionRule {
    category: String,
    regex: Regex,
    placeholder: String,
}

#[derive(Debug, Clone)]
struct OpaPolicyRuntime {
    command: String,
    query: String,
    policy_path: String,
}

type RedactionBuildResult = (
    Vec<CompiledRedactionRule>,
    BTreeMap<String, usize>,
    Vec<String>,
);

const OPA_EVAL_TIMEOUT: Duration = Duration::from_secs(3);
static HIGH_ENTROPY_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9+/=_-]{16,}").expect("high entropy token regex must compile")
});

impl PolicyRuntime {
    pub fn from_search_config(
        search_config: &CoreSearchConfig,
        request_override: Option<PolicyMode>,
    ) -> Result<Self, StateError> {
        let config_mode = search_config.policy_mode_typed();
        let mut warnings = Vec::new();
        let mode = match search_config.resolve_policy_mode(request_override) {
            Ok((resolved, mut override_warnings)) => {
                warnings.append(&mut override_warnings);
                resolved
            }
            Err(err) => {
                if config_mode == PolicyMode::Strict {
                    return Err(StateError::policy(format!(
                        "policy override rejected: {err}"
                    )));
                }
                warnings.push(format!("policy override rejected; continuing: {err}"));
                config_mode
            }
        };

        let strict = mode == PolicyMode::Strict;
        let policy = &search_config.policy;
        let path_deny = compile_glob_patterns(&policy.path.deny, strict, &mut warnings)?;
        let path_allow = compile_glob_patterns(&policy.path.allow, strict, &mut warnings)?;
        let deny_result_types = policy
            .kind
            .deny_result_types
            .iter()
            .map(|v| v.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let allow_result_types = policy
            .kind
            .allow_result_types
            .iter()
            .map(|v| v.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let deny_symbol_kinds = policy
            .kind
            .deny_symbol_kinds
            .iter()
            .map(|v| v.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let allow_symbol_kinds = policy
            .kind
            .allow_symbol_kinds
            .iter()
            .map(|v| v.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let (redaction_rules, redaction_categories, mut redaction_warnings) =
            build_redaction_rules(policy, strict)?;
        warnings.append(&mut redaction_warnings);
        let opa = build_opa_runtime(&policy.opa, strict, &mut warnings)?;

        Ok(Self {
            mode,
            audit_only: mode == PolicyMode::AuditOnly,
            path_deny,
            path_allow,
            deny_result_types,
            allow_result_types,
            deny_symbol_kinds,
            allow_symbol_kinds,
            redaction_enabled: policy.redaction.enabled,
            redaction_rules,
            redaction_categories,
            high_entropy_min_length: policy.redaction.high_entropy_min_length,
            high_entropy_threshold: policy.redaction.high_entropy_threshold,
            warnings,
            opa,
        })
    }

    pub fn mode(&self) -> PolicyMode {
        self.mode
    }

    pub fn apply(&self, results: Vec<SearchResult>) -> Result<PolicyApplication, StateError> {
        if self.mode == PolicyMode::Off {
            return Ok(PolicyApplication {
                mode: self.mode,
                results,
                blocked_count: 0,
                redacted_count: 0,
                warnings: self.warnings.clone(),
                audit_counts: BTreeMap::new(),
                active_redaction_categories: self.redaction_categories.clone(),
            });
        }

        let mut warnings = self.warnings.clone();
        let mut blocked_count = 0usize;
        let mut redacted_count = 0usize;
        let mut audit_counts = BTreeMap::new();
        let mut output = Vec::with_capacity(results.len());

        for mut result in results {
            let block_reason = self.should_block(&result, &mut warnings)?;
            if let Some(reason) = block_reason {
                blocked_count += 1;
                *audit_counts.entry(reason).or_insert(0) += 1;
                if !self.audit_only {
                    continue;
                }
            }

            if let Some(snippet) = result.snippet.as_deref() {
                let redaction = self.redact_text(snippet);
                if redaction.redacted_count > 0 {
                    redacted_count += redaction.redacted_count;
                    for (category, count) in redaction.category_counts {
                        let key = format!("redacted:{category}");
                        *audit_counts.entry(key).or_insert(0) += count;
                    }
                    if !self.audit_only {
                        result.snippet = Some(redaction.text);
                    }
                }
            }
            output.push(result);
        }

        tracing::info!(
            policy_mode = %self.mode,
            policy_blocked_count = blocked_count,
            policy_redacted_count = redacted_count,
            policy_audit_only = self.audit_only,
            "retrieval policy decision counters"
        );

        Ok(PolicyApplication {
            mode: self.mode,
            results: output,
            blocked_count,
            redacted_count,
            warnings,
            audit_counts,
            active_redaction_categories: self.redaction_categories.clone(),
        })
    }

    pub fn redact_text(&self, text: &str) -> TextRedactionResult {
        if self.mode == PolicyMode::Off || !self.redaction_enabled {
            return TextRedactionResult {
                text: text.to_string(),
                redacted_count: 0,
                category_counts: BTreeMap::new(),
            };
        }

        let mut current = text.to_string();
        let mut redacted_count = 0usize;
        let mut category_counts = BTreeMap::new();
        for rule in &self.redaction_rules {
            let local_count = rule.regex.find_iter(&current).count();
            if local_count > 0 {
                let replaced = rule
                    .regex
                    .replace_all(&current, rule.placeholder.as_str())
                    .to_string();
                redacted_count += local_count;
                *category_counts.entry(rule.category.clone()).or_insert(0) += local_count;
                current = replaced;
            }
        }

        let (entropy_redacted, entropy_count) = redact_high_entropy_tokens(
            &current,
            self.high_entropy_min_length,
            self.high_entropy_threshold,
        );
        if entropy_count > 0 {
            redacted_count += entropy_count;
            *category_counts
                .entry("high_entropy".to_string())
                .or_insert(0) += entropy_count;
            current = entropy_redacted;
        }

        TextRedactionResult {
            text: current,
            redacted_count,
            category_counts,
        }
    }

    fn should_block(
        &self,
        result: &SearchResult,
        warnings: &mut Vec<String>,
    ) -> Result<Option<String>, StateError> {
        if self
            .path_deny
            .iter()
            .any(|pattern| pattern.is_match(&result.path))
        {
            return Ok(Some("blocked:path_deny".to_string()));
        }
        if !self.path_allow.is_empty() && !self.path_allow.iter().any(|p| p.is_match(&result.path))
        {
            return Ok(Some("blocked:path_allow_miss".to_string()));
        }

        let result_type = result.result_type.to_ascii_lowercase();
        if self.deny_result_types.contains(result_type.as_str()) {
            return Ok(Some("blocked:result_type_deny".to_string()));
        }
        if !self.allow_result_types.is_empty() && !self.allow_result_types.contains(&result_type) {
            return Ok(Some("blocked:result_type_allow_miss".to_string()));
        }

        if let Some(kind) = result.kind.as_deref() {
            let kind = kind.to_ascii_lowercase();
            if self.deny_symbol_kinds.contains(kind.as_str()) {
                return Ok(Some("blocked:symbol_kind_deny".to_string()));
            }
            if !self.allow_symbol_kinds.is_empty() && !self.allow_symbol_kinds.contains(&kind) {
                return Ok(Some("blocked:symbol_kind_allow_miss".to_string()));
            }
        }

        if let Some(opa) = &self.opa {
            match evaluate_opa_decision(opa, result) {
                Ok(true) => {}
                Ok(false) => return Ok(Some("blocked:opa_deny".to_string())),
                Err(err) => {
                    if self.mode == PolicyMode::Strict {
                        return Err(StateError::policy(format!(
                            "OPA policy evaluation failed in strict mode: {err}"
                        )));
                    }
                    warnings.push(format!(
                        "opa evaluation failed in {} mode; continuing open: {}",
                        self.mode, err
                    ));
                }
            }
        }

        Ok(None)
    }
}

fn compile_glob_patterns(
    patterns: &[String],
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<GlobMatcher>, StateError> {
    let mut out = Vec::new();
    for raw in patterns
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        match Glob::new(raw) {
            Ok(glob) => out.push(glob.compile_matcher()),
            Err(err) => {
                if strict {
                    return Err(StateError::policy(format!(
                        "invalid policy path glob `{raw}`: {err}"
                    )));
                }
                warnings.push(format!(
                    "invalid policy path glob `{raw}` ignored in balanced/audit mode: {err}"
                ));
            }
        }
    }
    Ok(out)
}

fn build_redaction_rules(
    policy: &RetrievalPolicyConfig,
    strict: bool,
) -> Result<RedactionBuildResult, StateError> {
    let mut warnings = Vec::new();
    if !policy.redaction.enabled {
        return Ok((Vec::new(), BTreeMap::new(), warnings));
    }

    let mut rules = Vec::new();
    let mut category_counts = BTreeMap::new();
    let mut seen_signatures = HashSet::new();

    let mut push_rule = |category: &str, pattern: &str, placeholder: &str, regex: Regex| {
        let signature = format!("{pattern}\u{001f}{placeholder}");
        if !seen_signatures.insert(signature) {
            return;
        }
        rules.push(CompiledRedactionRule {
            category: category.to_string(),
            regex,
            placeholder: placeholder.to_string(),
        });
        *category_counts.entry(category.to_string()).or_insert(0) += 1;
    };

    for (category, pattern, placeholder) in default_seeded_rules(policy.redaction.email_masking) {
        let regex = Regex::new(pattern).map_err(|err| {
            StateError::policy(format!(
                "failed to compile built-in redaction rule `{category}`: {err}"
            ))
        })?;
        push_rule(category, pattern, placeholder, regex);
    }

    if policy.detect_secrets.enabled {
        for plugin in policy
            .detect_secrets
            .plugins
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            for (category, pattern, placeholder) in detect_secrets_plugin_rules(&plugin) {
                match Regex::new(pattern) {
                    Ok(regex) => {
                        push_rule(category, pattern, placeholder, regex);
                    }
                    Err(err) => {
                        if strict {
                            return Err(StateError::policy(format!(
                                "invalid detect-secrets plugin pattern `{category}`: {err}"
                            )));
                        }
                        warnings.push(format!(
                            "invalid detect-secrets pattern `{category}` ignored: {err}"
                        ));
                    }
                }
            }
        }

        for pattern in &policy.detect_secrets.custom_patterns {
            match Regex::new(pattern) {
                Ok(regex) => {
                    push_rule(
                        "detect_secrets_custom",
                        pattern,
                        "[REDACTED:detect_secrets]",
                        regex,
                    );
                }
                Err(err) => {
                    if strict {
                        return Err(StateError::policy(format!(
                            "invalid detect-secrets custom pattern `{pattern}`: {err}"
                        )));
                    }
                    warnings.push(format!(
                        "invalid detect-secrets custom pattern `{pattern}` ignored: {err}"
                    ));
                }
            }
        }
    }

    for rule in &policy.redaction.custom_rules {
        match compile_custom_rule(rule) {
            Ok(compiled) => {
                let pattern = compiled.regex.as_str().to_string();
                let placeholder = compiled.placeholder.clone();
                let signature = format!("{pattern}\u{001f}{placeholder}");
                if !seen_signatures.insert(signature) {
                    continue;
                }
                *category_counts
                    .entry(compiled.category.clone())
                    .or_insert(0) += 1;
                rules.push(compiled);
            }
            Err(err) => {
                if strict {
                    return Err(StateError::policy(err));
                }
                warnings.push(err);
            }
        }
    }

    rules.sort_by(|left, right| left.category.cmp(&right.category));
    Ok((rules, category_counts, warnings))
}

fn compile_custom_rule(rule: &PolicyRedactionRule) -> Result<CompiledRedactionRule, String> {
    let regex = Regex::new(&rule.pattern)
        .map_err(|err| format!("invalid custom redaction pattern `{}`: {err}", rule.name))?;
    Ok(CompiledRedactionRule {
        category: rule.category.clone(),
        regex,
        placeholder: rule.placeholder.clone(),
    })
}

fn default_seeded_rules(email_masking: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rules = vec![
        (
            "pem_private_key",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            "[REDACTED:private_key]",
        ),
        (
            "aws_access_key_id",
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            "[REDACTED:aws_access_key]",
        ),
        (
            "gcp_service_account",
            r#"(?i)"type"\s*:\s*"service_account""#,
            "[REDACTED:gcp_service_account]",
        ),
        (
            "github_token",
            r"\bgh(?:p|o|u|s|r)_[A-Za-z0-9]{20,}\b",
            "[REDACTED:github_token]",
        ),
        (
            "slack_token",
            r"\bxox(?:b|p|a|r|s)-[A-Za-z0-9-]{10,48}\b",
            "[REDACTED:slack_token]",
        ),
    ];
    if email_masking {
        rules.push((
            "email_address",
            r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b",
            "[REDACTED:email]",
        ));
    }
    rules
}

fn detect_secrets_plugin_rules(plugin: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    match plugin {
        "awskeydetector" | "aws" => vec![(
            "detect_secrets_aws",
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            "[REDACTED:aws_access_key]",
        )],
        "githubtokendetector" | "github" => vec![(
            "detect_secrets_github",
            r"\bgh(?:p|o|u|s|r)_[A-Za-z0-9]{20,}\b",
            "[REDACTED:github_token]",
        )],
        "slackdetector" | "slack" => vec![(
            "detect_secrets_slack",
            r"\bxox(?:b|p|a|r|s)-[A-Za-z0-9-]{10,48}\b",
            "[REDACTED:slack_token]",
        )],
        "privatekeydetector" | "privatekey" => vec![(
            "detect_secrets_private_key",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            "[REDACTED:private_key]",
        )],
        _ => Vec::new(),
    }
}

fn build_opa_runtime(
    config: &OpaPolicyConfig,
    strict: bool,
    warnings: &mut Vec<String>,
) -> Result<Option<OpaPolicyRuntime>, StateError> {
    if !config.enabled {
        return Ok(None);
    }
    let Some(policy_path) = config
        .policy_path
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        if strict {
            return Err(StateError::policy(
                "OPA is enabled but search.policy.opa.policy_path is missing".to_string(),
            ));
        }
        warnings.push(
            "OPA is enabled but policy_path is missing; continuing without OPA decisions"
                .to_string(),
        );
        return Ok(None);
    };

    Ok(Some(OpaPolicyRuntime {
        command: config.command.trim().to_string(),
        query: config.query.trim().to_string(),
        policy_path: policy_path.to_string(),
    }))
}

fn evaluate_opa_decision(
    opa: &OpaPolicyRuntime,
    result: &SearchResult,
) -> Result<bool, StateError> {
    let mut command = Command::new(&opa.command);
    command
        .arg("eval")
        .arg("--format")
        .arg("json")
        .arg("--data")
        .arg(&opa.policy_path)
        .arg("--input")
        .arg("-")
        .arg(&opa.query)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| StateError::policy(format!("failed to spawn OPA command: {err}")))?;
    if let Some(stdin) = child.stdin.as_mut() {
        let input = serde_json::json!({
            "path": result.path,
            "result_type": result.result_type,
            "kind": result.kind,
            "language": result.language,
        });
        stdin
            .write_all(input.to_string().as_bytes())
            .map_err(|err| {
                StateError::policy(format!("failed to write OPA input payload: {err}"))
            })?;
    }
    let output = wait_for_opa_output(child)?;
    if !output.status.success() {
        return Err(StateError::policy(format!(
            "OPA command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| StateError::policy(format!("failed to parse OPA output JSON: {err}")))?;
    let allow = value
        .get("result")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("expressions"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_bool())
        .ok_or_else(|| {
            StateError::policy("OPA result did not contain boolean decision".to_string())
        })?;
    Ok(allow)
}

fn redact_high_entropy_tokens(text: &str, min_length: usize, threshold: f64) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut redacted = 0usize;
    for matched in HIGH_ENTROPY_TOKEN_RE.find_iter(text) {
        let token = matched.as_str();
        if token.len() < min_length || shannon_entropy(token) < threshold {
            continue;
        }
        out.push_str(&text[last..matched.start()]);
        out.push_str("[REDACTED:high_entropy]");
        last = matched.end();
        redacted += 1;
    }
    out.push_str(&text[last..]);
    if redacted == 0 {
        (text.to_string(), 0)
    } else {
        (out, redacted)
    }
}

fn wait_for_opa_output(mut child: std::process::Child) -> Result<std::process::Output, StateError> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut reader) = child.stdout.take() {
                    reader.read_to_end(&mut stdout).map_err(|err| {
                        StateError::policy(format!("failed to read OPA output: {err}"))
                    })?;
                }
                if let Some(mut reader) = child.stderr.take() {
                    reader.read_to_end(&mut stderr).map_err(|err| {
                        StateError::policy(format!("failed to read OPA stderr: {err}"))
                    })?;
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= OPA_EVAL_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(StateError::policy(format!(
                        "OPA command timed out after {}ms",
                        OPA_EVAL_TIMEOUT.as_millis()
                    )));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) => {
                return Err(StateError::policy(format!(
                    "failed to poll OPA process status: {err}"
                )));
            }
        }
    }
}

fn shannon_entropy(value: &str) -> f64 {
    let mut freq = BTreeMap::<u8, usize>::new();
    for byte in value.bytes() {
        *freq.entry(byte).or_insert(0) += 1;
    }
    let len = value.len() as f64;
    freq.values()
        .map(|count| {
            let p = (*count as f64) / len;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::config::SearchConfig as CoreSearchConfig;

    fn sample_result(path: &str, snippet: &str) -> SearchResult {
        SearchResult {
            repo: "repo".to_string(),
            result_id: "res-1".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "snippet".to_string(),
            path: path.to_string(),
            line_start: 1,
            line_end: 1,
            kind: Some("function".to_string()),
            name: Some("demo".to_string()),
            qualified_name: Some("demo".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score: 1.0,
            snippet: Some(snippet.to_string()),
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        }
    }

    fn aws_fixture_token() -> String {
        let suffix: String = (0..16).map(|idx| (b'A' + idx) as char).collect();
        let prefix: String = ['A', 'K', 'I', 'A'].iter().collect();
        format!("{prefix}{suffix}")
    }

    fn github_fixture_token() -> String {
        let suffix: String = (0..24).map(|idx| (b'a' + (idx % 26)) as char).collect();
        format!("ghp_{suffix}")
    }

    #[test]
    fn strict_mode_rejects_invalid_custom_regex() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "strict".to_string();
        config
            .policy
            .redaction
            .custom_rules
            .push(PolicyRedactionRule {
                name: "bad".to_string(),
                category: "custom".to_string(),
                pattern: "(".to_string(),
                placeholder: "[REDACTED]".to_string(),
            });
        let err = PolicyRuntime::from_search_config(&config, None).unwrap_err();
        assert!(format!("{err}").contains("invalid custom redaction pattern"));
    }

    #[test]
    fn balanced_mode_warns_and_fails_open_on_invalid_custom_regex() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config
            .policy
            .redaction
            .custom_rules
            .push(PolicyRedactionRule {
                name: "bad".to_string(),
                category: "custom".to_string(),
                pattern: "(".to_string(),
                placeholder: "[REDACTED]".to_string(),
            });
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();
        assert!(
            runtime
                .warnings
                .iter()
                .any(|w| w.contains("invalid custom redaction pattern"))
        );
    }

    #[test]
    fn apply_blocks_denied_paths_and_redacts_snippets() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.path.deny = vec!["**/secrets/**".to_string()];
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();

        let applied = runtime
            .apply(vec![
                sample_result("src/secrets/token.rs", &github_fixture_token()),
                sample_result("src/lib.rs", "contact me at security@example.com"),
            ])
            .unwrap();
        assert_eq!(applied.mode, PolicyMode::Balanced);
        assert_eq!(applied.blocked_count, 1);
        assert!(applied.redacted_count >= 1);
        assert_eq!(applied.results.len(), 1);
        let snippet = applied.results[0].snippet.as_deref().unwrap_or_default();
        assert!(snippet.contains("[REDACTED:email]"));
    }

    #[test]
    fn audit_only_records_but_does_not_mutate_results() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "audit_only".to_string();
        config.policy.path.deny = vec!["src/private/**".to_string()];
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();

        let original = sample_result("src/private/auth.rs", &aws_fixture_token());
        let applied = runtime.apply(vec![original.clone()]).unwrap();
        assert_eq!(applied.blocked_count, 1);
        assert_eq!(applied.results.len(), 1);
        assert_eq!(applied.results[0].path, original.path);
        assert_eq!(applied.results[0].snippet, original.snippet);
    }

    #[test]
    fn off_mode_passes_through_without_counts() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "off".to_string();
        config.policy.path.deny = vec!["src/private/**".to_string()];
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();

        let original = sample_result("src/private/auth.rs", &aws_fixture_token());
        let applied = runtime.apply(vec![original.clone()]).unwrap();
        assert_eq!(applied.mode, PolicyMode::Off);
        assert_eq!(applied.blocked_count, 0);
        assert_eq!(applied.redacted_count, 0);
        assert_eq!(applied.results[0].path, original.path);
        assert_eq!(applied.results[0].snippet, original.snippet);
    }

    #[test]
    fn glob_patterns_match_root_and_nested_paths() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.path.deny = vec!["**/.env*".to_string(), "**/secrets/**".to_string()];
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();

        let applied = runtime
            .apply(vec![
                sample_result(".env", "DATABASE_URL=sqlite://"),
                sample_result("secrets/keys.rs", "const TOKEN: &str = \"dev\";"),
                sample_result("src/secrets/keys.rs", "const TOKEN: &str = \"dev\";"),
                sample_result("src/lib.rs", "pub fn run() {}"),
            ])
            .unwrap();
        assert_eq!(applied.blocked_count, 3);
        assert_eq!(applied.results.len(), 1);
        assert_eq!(applied.results[0].path, "src/lib.rs");
    }

    #[test]
    fn redaction_disabled_skips_builtin_and_high_entropy_redaction() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.redaction.enabled = false;
        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();

        let snippet = format!(
            "contact security@example.com and use token {}",
            aws_fixture_token()
        );
        let applied = runtime
            .apply(vec![sample_result("src/notify.rs", &snippet)])
            .unwrap();
        assert_eq!(applied.redacted_count, 0);
        assert_eq!(
            applied.results[0].snippet.as_deref(),
            Some(snippet.as_str())
        );
    }

    #[test]
    fn detect_secrets_custom_patterns_require_enabled_flag() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.detect_secrets.custom_patterns = vec![r"CS_[0-9]{6}".to_string()];
        config.policy.detect_secrets.enabled = false;
        let snippet = r#"const SECRET: &str = "CS_123456";"#;

        let disabled_runtime = PolicyRuntime::from_search_config(&config, None).unwrap();
        let disabled = disabled_runtime
            .apply(vec![sample_result("src/feature.rs", snippet)])
            .unwrap();
        assert_eq!(disabled.redacted_count, 0);
        assert_eq!(disabled.results[0].snippet.as_deref(), Some(snippet));

        config.policy.detect_secrets.enabled = true;
        let enabled_runtime = PolicyRuntime::from_search_config(&config, None).unwrap();
        let enabled = enabled_runtime
            .apply(vec![sample_result("src/feature.rs", snippet)])
            .unwrap();
        assert_eq!(enabled.redacted_count, 1);
        assert!(
            enabled.results[0]
                .snippet
                .as_deref()
                .unwrap_or_default()
                .contains("[REDACTED:detect_secrets]")
        );
    }

    #[test]
    fn detect_secrets_plugins_do_not_duplicate_builtin_redaction_rules() {
        let mut config = CoreSearchConfig::default();
        config.policy.mode = "balanced".to_string();
        config.policy.detect_secrets.enabled = true;
        config.policy.detect_secrets.plugins = vec!["aws".to_string(), "github".to_string()];

        let runtime = PolicyRuntime::from_search_config(&config, None).unwrap();
        assert!(
            runtime
                .redaction_categories
                .contains_key("aws_access_key_id")
        );
        assert!(runtime.redaction_categories.contains_key("github_token"));
        assert!(
            !runtime
                .redaction_categories
                .contains_key("detect_secrets_aws")
        );
        assert!(
            !runtime
                .redaction_categories
                .contains_key("detect_secrets_github")
        );
    }

    #[cfg(unix)]
    #[test]
    fn opa_wait_timeout_guards_against_hanging_processes() {
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 10")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn long-running process");
        let err = wait_for_opa_output(child).unwrap_err();
        assert!(format!("{err}").contains("timed out"));
    }
}
