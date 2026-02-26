use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupRequest {
    pub previous_query_tool: String,
    pub previous_query_params: Value,
    pub previous_results: Value,
    pub confidence_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowupSuggestion {
    pub tool: String,
    pub params: Value,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FollowupAnalysis {
    pub previous_confidence: String,
    pub top_score: f64,
    pub threshold: f64,
    pub extracted_identifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FollowupResult {
    pub suggestions: Vec<FollowupSuggestion>,
    pub analysis: FollowupAnalysis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn suggest_followup_queries(request: &FollowupRequest) -> FollowupResult {
    let threshold = request.confidence_threshold.clamp(0.0, 1.0);
    let top_score = request
        .previous_results
        .get("top_score")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let total_candidates = extract_total_candidates(&request.previous_results);
    let total_edges = extract_total_edges(&request.previous_results);
    let query_intent = request
        .previous_results
        .get("query_intent")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let query_text = extract_query_text(&request.previous_query_params);
    let extracted_identifiers = query_text.map(extract_identifiers).unwrap_or_default();

    let low_confidence = match request.previous_query_tool.as_str() {
        "get_call_graph" => total_edges == 0,
        _ => total_candidates == 0 || top_score < threshold,
    };
    let analysis = FollowupAnalysis {
        previous_confidence: if low_confidence {
            "low".to_string()
        } else {
            "sufficient".to_string()
        },
        top_score,
        threshold,
        extracted_identifiers: extracted_identifiers.clone(),
    };

    if !low_confidence {
        return FollowupResult {
            suggestions: Vec::new(),
            analysis,
            reason: Some("results are above confidence threshold".to_string()),
        };
    }

    let mut suggestions = Vec::new();
    let mut dedup = HashSet::<String>::new();

    match request.previous_query_tool.as_str() {
        "search_code" => {
            if let Some(identifier) = extracted_identifiers.first() {
                push_suggestion(
                    &mut suggestions,
                    &mut dedup,
                    "locate_symbol",
                    json!({
                        "name": identifier,
                        "limit": 10
                    }),
                    format!(
                        "Extracted identifier '{}' from prior query; symbol lookup is likely more precise.",
                        identifier
                    ),
                );
                push_suggestion(
                    &mut suggestions,
                    &mut dedup,
                    "get_call_graph",
                    json!({
                        "symbol_name": identifier,
                        "direction": "both",
                        "depth": 1,
                        "limit": 20
                    }),
                    "Call graph traversal can reveal relationships around the likely target symbol."
                        .to_string(),
                );
            }

            if query_intent == "natural_language"
                && let Some(query) = query_text
            {
                let fallback_query = if extracted_identifiers.is_empty() {
                    query.to_string()
                } else {
                    extracted_identifiers.join(" ")
                };
                push_suggestion(
                    &mut suggestions,
                    &mut dedup,
                    "search_code",
                    json!({
                        "query": fallback_query
                    }),
                    "Rewrite the natural-language query into identifiers to improve lexical recall."
                        .to_string(),
                );
            }
        }
        "locate_symbol" => {
            if total_candidates == 0 {
                let symbol_name = request
                    .previous_query_params
                    .get("name")
                    .or_else(|| request.previous_query_params.get("symbol_name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if !symbol_name.trim().is_empty() {
                    push_suggestion(
                        &mut suggestions,
                        &mut dedup,
                        "search_code",
                        json!({
                            "query": symbol_name
                        }),
                        "No exact symbol match found; broaden search to raw code text.".to_string(),
                    );
                    push_suggestion(
                        &mut suggestions,
                        &mut dedup,
                        "get_call_graph",
                        json!({
                            "symbol_name": symbol_name,
                            "direction": "both",
                            "depth": 1,
                            "limit": 20
                        }),
                        "If the symbol exists under a variant path/name, call graph lookup may still surface adjacent symbols."
                            .to_string(),
                    );
                }
            }
        }
        "get_call_graph" => {
            if total_edges == 0 {
                let symbol_name = request
                    .previous_query_params
                    .get("symbol_name")
                    .or_else(|| request.previous_query_params.get("name"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if !symbol_name.trim().is_empty() {
                    push_suggestion(
                        &mut suggestions,
                        &mut dedup,
                        "locate_symbol",
                        json!({
                            "name": symbol_name,
                            "limit": 10
                        }),
                        "Call graph returned no edges; first verify the symbol resolves in this ref."
                            .to_string(),
                    );
                    push_suggestion(
                        &mut suggestions,
                        &mut dedup,
                        "search_code",
                        json!({
                            "query": symbol_name
                        }),
                        "No graph edges found; broaden lookup to alternate names or nearby call sites."
                            .to_string(),
                    );
                }
            }
        }
        _ => {
            if let Some(query) = query_text {
                push_suggestion(
                    &mut suggestions,
                    &mut dedup,
                    "search_code",
                    json!({
                        "query": query
                    }),
                    "Fallback to direct code search to recover from low-confidence results."
                        .to_string(),
                );
            }
        }
    }

    if suggestions.is_empty() {
        push_suggestion(
            &mut suggestions,
            &mut dedup,
            "search_code",
            json!({
                "query": query_text.unwrap_or("")
            }),
            "Fallback: retry with direct search_code to gather broader candidate context."
                .to_string(),
        );
    }

    FollowupResult {
        suggestions,
        analysis,
        reason: None,
    }
}

fn push_suggestion(
    suggestions: &mut Vec<FollowupSuggestion>,
    dedup: &mut HashSet<String>,
    tool: &str,
    params: Value,
    reason: String,
) {
    let key = format!("{tool}:{}", params);
    if dedup.insert(key) {
        suggestions.push(FollowupSuggestion {
            tool: tool.to_string(),
            params,
            reason,
        });
    }
}

fn extract_total_candidates(previous_results: &Value) -> usize {
    if let Some(total) = previous_results
        .get("total_candidates")
        .and_then(|value| value.as_u64())
    {
        return total as usize;
    }
    if let Some(results) = previous_results
        .get("results")
        .and_then(|value| value.as_array())
    {
        return results.len();
    }
    0
}

fn extract_total_edges(previous_results: &Value) -> usize {
    if let Some(total) = previous_results
        .get("total_edges")
        .and_then(|value| value.as_u64())
    {
        return total as usize;
    }

    let callers = previous_results
        .get("callers")
        .and_then(|value| value.as_array())
        .map_or(0, Vec::len);
    let callees = previous_results
        .get("callees")
        .and_then(|value| value.as_array())
        .map_or(0, Vec::len);
    callers + callees
}

fn extract_query_text(previous_query_params: &Value) -> Option<&str> {
    previous_query_params
        .get("query")
        .and_then(|value| value.as_str())
        .or_else(|| {
            previous_query_params
                .get("name")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            previous_query_params
                .get("symbol_name")
                .and_then(|value| value.as_str())
        })
}

fn extract_identifiers(input: &str) -> Vec<String> {
    let stopwords = [
        "where",
        "what",
        "when",
        "which",
        "with",
        "without",
        "from",
        "into",
        "implemented",
        "implementation",
        "function",
        "method",
        "class",
        "module",
        "code",
        "the",
        "and",
        "for",
        "that",
    ];
    let stopwords: HashSet<&str> = stopwords.into_iter().collect();

    let mut seen = HashSet::<String>::new();
    let mut identifiers = Vec::new();
    let mut token = String::new();

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch.to_ascii_lowercase());
        } else {
            push_identifier(&mut token, &stopwords, &mut seen, &mut identifiers);
        }
    }
    push_identifier(&mut token, &stopwords, &mut seen, &mut identifiers);
    identifiers
}

fn push_identifier(
    token: &mut String,
    stopwords: &HashSet<&str>,
    seen: &mut HashSet<String>,
    identifiers: &mut Vec<String>,
) {
    if token.len() >= 3
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && !stopwords.contains(token.as_str())
        && seen.insert(token.clone())
    {
        identifiers.push(token.clone());
    }
    token.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_followup_queries_low_confidence_search_adds_locate_symbol() {
        let request = FollowupRequest {
            previous_query_tool: "search_code".to_string(),
            previous_query_params: json!({
                "query": "where is rate limiting implemented"
            }),
            previous_results: json!({
                "query_intent": "natural_language",
                "top_score": 0.25,
                "total_candidates": 3
            }),
            confidence_threshold: 0.5,
        };

        let result = suggest_followup_queries(&request);
        assert!(!result.suggestions.is_empty());
        assert!(
            result
                .suggestions
                .iter()
                .any(|suggestion| suggestion.tool == "locate_symbol")
        );
    }

    #[test]
    fn suggest_followup_queries_zero_result_locate_suggests_search_and_call_graph() {
        let request = FollowupRequest {
            previous_query_tool: "locate_symbol".to_string(),
            previous_query_params: json!({
                "name": "validate_token"
            }),
            previous_results: json!({
                "top_score": 0.0,
                "total_candidates": 0,
                "results": []
            }),
            confidence_threshold: 0.5,
        };

        let result = suggest_followup_queries(&request);
        let tools: HashSet<&str> = result
            .suggestions
            .iter()
            .map(|suggestion| suggestion.tool.as_str())
            .collect();
        assert!(tools.contains("search_code"));
        assert!(tools.contains("get_call_graph"));
    }

    #[test]
    fn suggest_followup_queries_above_threshold_returns_empty_suggestions() {
        let request = FollowupRequest {
            previous_query_tool: "search_code".to_string(),
            previous_query_params: json!({
                "query": "validate_token"
            }),
            previous_results: json!({
                "top_score": 0.91,
                "total_candidates": 4
            }),
            confidence_threshold: 0.5,
        };

        let result = suggest_followup_queries(&request);
        assert!(result.suggestions.is_empty());
        assert_eq!(
            result.reason.as_deref(),
            Some("results are above confidence threshold")
        );
    }

    #[test]
    fn suggest_followup_queries_zero_edges_get_call_graph_suggests_locate_and_search() {
        let request = FollowupRequest {
            previous_query_tool: "get_call_graph".to_string(),
            previous_query_params: json!({
                "symbol_name": "validate_token",
                "direction": "both",
                "depth": 2
            }),
            previous_results: json!({
                "total_edges": 0,
                "callers": [],
                "callees": []
            }),
            confidence_threshold: 0.5,
        };

        let result = suggest_followup_queries(&request);
        let tools: HashSet<&str> = result
            .suggestions
            .iter()
            .map(|suggestion| suggestion.tool.as_str())
            .collect();

        assert!(tools.contains("locate_symbol"));
        assert!(tools.contains("search_code"));
    }
}
