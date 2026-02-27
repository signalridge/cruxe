use codecompass_core::config::SearchIntentConfig;
use codecompass_core::types::QueryIntent;

#[derive(Debug, Clone)]
pub struct IntentClassification {
    pub intent: QueryIntent,
    pub confidence: f64,
    pub escalation_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentRule {
    ErrorPattern,
    Path,
    QuotedError,
    Symbol,
    NaturalLanguage,
}

#[derive(Debug, Clone)]
pub struct IntentPolicy {
    rule_order: Vec<IntentRule>,
    error_patterns: Vec<String>,
    path_extensions: Vec<String>,
    symbol_kind_keywords: Vec<String>,
    enable_wrapped_quoted_error_literal: bool,
}

impl Default for IntentPolicy {
    fn default() -> Self {
        Self::from(&SearchIntentConfig::default())
    }
}

impl From<&SearchIntentConfig> for IntentPolicy {
    fn from(config: &SearchIntentConfig) -> Self {
        let config = config.normalized();
        let rule_order = config
            .rule_order
            .iter()
            .filter_map(|value| parse_intent_rule(value))
            .collect();

        IntentPolicy {
            rule_order,
            error_patterns: config.error_patterns.clone(),
            path_extensions: config.path_extensions.clone(),
            symbol_kind_keywords: config.symbol_kind_keywords.clone(),
            enable_wrapped_quoted_error_literal: config.enable_wrapped_quoted_error_literal,
        }
    }
}

/// Classify a search query into an intent category.
pub fn classify_intent(query: &str) -> QueryIntent {
    classify_intent_with_confidence(query).intent
}

pub fn classify_intent_with_confidence(query: &str) -> IntentClassification {
    classify_intent_with_policy(query, &IntentPolicy::default())
}

pub fn classify_intent_with_policy(query: &str, policy: &IntentPolicy) -> IntentClassification {
    let trimmed = query.trim();

    for rule in &policy.rule_order {
        match rule {
            IntentRule::ErrorPattern => {
                if let Some(confidence) =
                    error_pattern_intent_confidence(trimmed, &policy.error_patterns)
                {
                    return build_classification(QueryIntent::Error, confidence);
                }
            }
            IntentRule::Path => {
                if let Some(confidence) = path_intent_confidence(trimmed, &policy.path_extensions) {
                    return build_classification(QueryIntent::Path, confidence);
                }
            }
            IntentRule::QuotedError => {
                if let Some(confidence) = quoted_error_intent_confidence(
                    trimmed,
                    policy.enable_wrapped_quoted_error_literal,
                ) {
                    return build_classification(QueryIntent::Error, confidence);
                }
            }
            IntentRule::Symbol => {
                if let Some(confidence) =
                    symbol_intent_confidence(trimmed, &policy.symbol_kind_keywords)
                {
                    return build_classification(QueryIntent::Symbol, confidence);
                }
            }
            IntentRule::NaturalLanguage => {
                return build_classification(
                    QueryIntent::NaturalLanguage,
                    natural_language_confidence(trimmed),
                );
            }
        }
    }

    build_classification(
        QueryIntent::NaturalLanguage,
        natural_language_confidence(trimmed),
    )
}

fn build_classification(intent: QueryIntent, confidence: f64) -> IntentClassification {
    let confidence = confidence.clamp(0.0, 1.0);
    let escalation_hint = if confidence >= 0.65 {
        None
    } else {
        Some(match intent {
            QueryIntent::NaturalLanguage => {
                "Intent confidence is low; retry as symbol/path if you know exact identifiers."
                    .to_string()
            }
            QueryIntent::Symbol => {
                "Intent confidence is low; retry with natural-language wording or include file path."
                    .to_string()
            }
            QueryIntent::Path => {
                "Intent confidence is low; retry with exact path or broaden to filename-only search."
                    .to_string()
            }
            QueryIntent::Error => {
                "Intent confidence is low; include exact error text or stack-frame snippet."
                    .to_string()
            }
        })
    };

    IntentClassification {
        intent,
        confidence,
        escalation_hint,
    }
}

fn natural_language_confidence(query: &str) -> f64 {
    if query.split_whitespace().count() <= 1 {
        0.55
    } else {
        0.72
    }
}

fn path_intent_confidence(query: &str, path_extensions: &[String]) -> Option<f64> {
    if query.contains('/') || query.contains('\\') {
        return Some(0.95);
    }

    let lowered = query.to_ascii_lowercase();
    for ext in path_extensions {
        if lowered.ends_with(ext) {
            return Some(0.85);
        }
    }
    None
}

fn error_pattern_intent_confidence(query: &str, error_patterns: &[String]) -> Option<f64> {
    for pattern in error_patterns {
        if query.contains(pattern) {
            return Some(0.9);
        }
    }
    None
}

fn quoted_error_intent_confidence(
    query: &str,
    enable_wrapped_quoted_error_literal: bool,
) -> Option<f64> {
    if enable_wrapped_quoted_error_literal && looks_like_quoted_error_literal(query) {
        return Some(0.9);
    }
    None
}

fn looks_like_quoted_error_literal(query: &str) -> bool {
    // Treat complete quoted literals as error intent (e.g. `"connection refused"`).
    // Avoid apostrophe-based false positives from natural-language contractions
    // (e.g. "where's auth handled").
    let trimmed = query.trim();
    trimmed.len() > 1
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('`') && trimmed.ends_with('`')))
}

fn symbol_intent_confidence(query: &str, symbol_kind_keywords: &[String]) -> Option<f64> {
    let words: Vec<&str> = query.split_whitespace().collect();

    if words.len() == 1 {
        let word = words[0];
        if word.len() > 1 && word.chars().skip(1).any(|c| c.is_uppercase()) {
            return Some(0.88);
        }
        if word.contains('_') {
            return Some(0.85);
        }
        if word.contains("::") || (word.contains('.') && !is_path_like(word)) {
            return Some(0.9);
        }
        if word.chars().all(|c| c.is_alphanumeric() || c == '_') && word.len() > 2 {
            return Some(0.6);
        }
    }

    if words.len() == 2 {
        let first_word = words[0].to_ascii_lowercase();
        if symbol_kind_keywords
            .iter()
            .any(|keyword| keyword == &first_word)
        {
            return Some(0.76);
        }
    }

    None
}

fn is_path_like(query: &str) -> bool {
    query.contains('/') || query.contains('\\')
}

fn parse_intent_rule(raw: &str) -> Option<IntentRule> {
    match codecompass_core::config::canonical_intent_rule_name(raw)? {
        "error_pattern" => Some(IntentRule::ErrorPattern),
        "path" => Some(IntentRule::Path),
        "quoted_error" => Some(IntentRule::QuotedError),
        "symbol" => Some(IntentRule::Symbol),
        "natural_language" => Some(IntentRule::NaturalLanguage),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_intent() {
        assert_eq!(classify_intent("validate_token"), QueryIntent::Symbol);
        assert_eq!(classify_intent("AuthHandler"), QueryIntent::Symbol);
        assert_eq!(classify_intent("auth::jwt::validate"), QueryIntent::Symbol);
    }

    #[test]
    fn test_path_intent() {
        assert_eq!(classify_intent("src/auth/handler.rs"), QueryIntent::Path);
        assert_eq!(classify_intent("handler.rs"), QueryIntent::Path);
    }

    #[test]
    fn test_error_intent() {
        assert_eq!(
            classify_intent("\"connection refused\""),
            QueryIntent::Error
        );
        assert_eq!(
            classify_intent("error: cannot find module"),
            QueryIntent::Error
        );
        assert_eq!(
            classify_intent("thread 'main' panicked at line 12"),
            QueryIntent::Error
        );
        assert_eq!(
            classify_intent("thread 'main' panicked at src/lib.rs:12"),
            QueryIntent::Error
        );
    }

    #[test]
    fn test_apostrophe_does_not_force_error_intent() {
        assert_eq!(
            classify_intent("where's rate limiting implemented"),
            QueryIntent::NaturalLanguage
        );
    }

    #[test]
    fn test_natural_language_intent() {
        assert_eq!(
            classify_intent("where is rate limiting implemented"),
            QueryIntent::NaturalLanguage
        );
        assert_eq!(
            classify_intent("how does authentication work"),
            QueryIntent::NaturalLanguage
        );
    }

    #[test]
    fn test_intent_confidence_and_escalation_hint() {
        let classification = classify_intent_with_confidence("abc");
        assert_eq!(classification.intent, QueryIntent::Symbol);
        assert!(classification.confidence < 0.75);
        assert!(classification.escalation_hint.is_some());

        let classification = classify_intent_with_confidence("src/auth/handler.rs");
        assert_eq!(classification.intent, QueryIntent::Path);
        assert!(classification.confidence > 0.9);
        assert!(classification.escalation_hint.is_none());
    }

    #[test]
    fn custom_rule_order_can_prioritize_path_over_error_pattern() {
        let config = SearchIntentConfig {
            rule_order: vec![
                "path".to_string(),
                "error_pattern".to_string(),
                "symbol".to_string(),
                "natural_language".to_string(),
            ],
            ..Default::default()
        };
        let policy = IntentPolicy::from(&config);

        let classification =
            classify_intent_with_policy("thread 'main' panicked at src/lib.rs:12", &policy);
        assert_eq!(classification.intent, QueryIntent::Path);
    }

    #[test]
    fn custom_error_patterns_are_respected() {
        let config = SearchIntentConfig {
            error_patterns: vec!["FAILED_ASSERT".to_string()],
            ..Default::default()
        };
        let policy = IntentPolicy::from(&config);

        let classification =
            classify_intent_with_policy("FAILED_ASSERT in request validator", &policy);
        assert_eq!(classification.intent, QueryIntent::Error);
    }

    #[test]
    fn wrapped_quote_error_literal_can_be_disabled() {
        let config = SearchIntentConfig {
            enable_wrapped_quoted_error_literal: false,
            ..Default::default()
        };
        let policy = IntentPolicy::from(&config);

        let classification = classify_intent_with_policy("\"connection refused\"", &policy);
        assert_eq!(classification.intent, QueryIntent::NaturalLanguage);
    }

    #[test]
    fn intent_rule_aliases_use_core_canonical_mapping() {
        assert_eq!(parse_intent_rule("error"), Some(IntentRule::ErrorPattern));
        assert_eq!(parse_intent_rule("quoted"), Some(IntentRule::QuotedError));
        assert_eq!(parse_intent_rule("nl"), Some(IntentRule::NaturalLanguage));
        assert_eq!(
            parse_intent_rule("default"),
            Some(IntentRule::NaturalLanguage)
        );
    }
}
