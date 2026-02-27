use crate::scoring::normalize_relevance_score;
use crate::search::SearchResult;
use codecompass_core::types::{ConfidenceGuidance, QueryIntent};

pub fn evaluate_confidence(
    results: &[SearchResult],
    query: &str,
    intent: QueryIntent,
    threshold: f64,
) -> ConfidenceGuidance {
    let threshold = threshold.clamp(0.0, 1.0);
    let top_score = results
        .first()
        .map(|result| normalize_relevance_score(result.score as f64))
        .unwrap_or(0.0);
    let score_margin = match (results.first(), results.get(1)) {
        (Some(first), Some(second)) => {
            normalize_relevance_score((first.score as f64 - second.score as f64).max(0.0))
        }
        (Some(_), None) => top_score,
        _ => 0.0,
    };
    let channel_agreement = channel_agreement(results);

    // Weighted composite with preference for top-score confidence.
    let composite_confidence =
        (top_score * 0.55 + score_margin * 0.30 + channel_agreement * 0.15).clamp(0.0, 1.0);
    let low_confidence = composite_confidence < threshold;

    let suggested_action = if low_confidence {
        Some(suggested_action(query, intent, results))
    } else {
        None
    };

    ConfidenceGuidance {
        low_confidence,
        suggested_action,
        threshold,
        top_score,
        score_margin,
        channel_agreement,
    }
}

/// Measures how many of the top results are corroborated by multiple search
/// channels (lexical, semantic, or both via "hybrid"). A higher ratio of
/// hybrid results means stronger cross-channel agreement.
fn channel_agreement(results: &[SearchResult]) -> f64 {
    let top = results.iter().take(5);
    let mut hybrid_count = 0u32;
    let mut total = 0u32;
    let mut saw_lexical = false;
    let mut saw_semantic = false;

    for result in top {
        total += 1;
        match result.provenance.to_ascii_lowercase().as_str() {
            "hybrid" => hybrid_count += 1,
            "semantic" => saw_semantic = true,
            _ => saw_lexical = true,
        }
    }

    if total == 0 {
        return 0.0;
    }

    // Proportion of top results that are hybrid (confirmed by both channels).
    let hybrid_ratio = hybrid_count as f64 / total as f64;
    if hybrid_ratio > 0.0 {
        // Scale: 1 hybrid in 5 → 0.5, all 5 hybrid → 1.0
        (0.5 + hybrid_ratio * 0.5).min(1.0)
    } else if saw_lexical && saw_semantic {
        // Both channels represented but no overlap → moderate agreement.
        0.4
    } else {
        0.0
    }
}

fn suggested_action(query: &str, intent: QueryIntent, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results found. Try broader search terms or check index status.".to_string();
    }

    match intent {
        QueryIntent::NaturalLanguage => {
            let identifier = extract_identifier(query).unwrap_or_else(|| "target_symbol".into());
            format!("Try locate_symbol with '{identifier}'")
        }
        QueryIntent::Symbol => {
            format!("Try search_code with natural language: 'where is {query} defined'")
        }
        QueryIntent::Path => {
            "Check file path spelling or try search_code with filename".to_string()
        }
        QueryIntent::Error => {
            "Try search_code with exact error substring or stack-frame snippet".to_string()
        }
    }
}

fn extract_identifier(query: &str) -> Option<String> {
    const STOP_WORDS: &[&str] = &[
        "where", "what", "when", "how", "is", "the", "in", "to", "for", "and", "of", "a", "an",
    ];

    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .find(|token| {
            let lowered = token.to_ascii_lowercase();
            token.len() >= 3
                && token
                    .chars()
                    .next()
                    .map(|ch| ch.is_alphabetic() || ch == '_')
                    .unwrap_or(false)
                && !STOP_WORDS.contains(&lowered.as_str())
        })
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(score: f32, provenance: &str) -> SearchResult {
        SearchResult {
            repo: "repo".to_string(),
            result_id: "id".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "symbol".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 1,
            kind: Some("function".to_string()),
            name: Some("demo".to_string()),
            qualified_name: Some("demo".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score,
            snippet: None,
            chunk_type: None,
            source_layer: None,
            provenance: provenance.to_string(),
        }
    }

    #[test]
    fn low_confidence_true_when_composite_below_threshold() {
        let results = vec![make_result(0.1, "lexical")];
        let guidance = evaluate_confidence(
            &results,
            "where is auth handled",
            QueryIntent::NaturalLanguage,
            0.5,
        );
        assert!(guidance.low_confidence);
        assert!(guidance.suggested_action.is_some());
    }

    #[test]
    fn low_confidence_false_when_composite_above_threshold() {
        let results = vec![make_result(8.0, "lexical")];
        let guidance = evaluate_confidence(&results, "AuthHandler", QueryIntent::Symbol, 0.3);
        assert!(!guidance.low_confidence);
        assert!(guidance.suggested_action.is_none());
    }

    #[test]
    fn suggested_action_varies_by_intent() {
        let nl = evaluate_confidence(
            &[make_result(0.1, "lexical")],
            "where is rate_limit",
            QueryIntent::NaturalLanguage,
            0.6,
        );
        assert!(
            nl.suggested_action
                .as_deref()
                .unwrap_or_default()
                .contains("locate_symbol")
        );

        let path = evaluate_confidence(
            &[make_result(0.1, "lexical")],
            "src/auth/mod.rs",
            QueryIntent::Path,
            0.9,
        );
        assert_eq!(
            path.suggested_action.as_deref(),
            Some("Check file path spelling or try search_code with filename")
        );
    }
}
