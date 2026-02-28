use crate::locate::LocateResult;
use crate::search::SearchResult;
use cruxe_core::config::{RankingSignalBudgetConfig, RankingSignalBudgetRange};
use cruxe_core::types::{
    BasicRankingReasons, RankingPrecedenceAudit, RankingReasons, RankingSignalContribution,
};
use std::cmp::Ordering;

const SIGNAL_BM25: &str = "bm25_score";
const SIGNAL_EXACT_MATCH: &str = "exact_match_boost";
const SIGNAL_QUALIFIED_NAME: &str = "qualified_name_boost";
const SIGNAL_PATH_AFFINITY: &str = "path_affinity";
const SIGNAL_DEFINITION_BOOST: &str = "definition_boost";
const SIGNAL_KIND_MATCH: &str = "kind_match";
const SIGNAL_TEST_FILE_PENALTY: &str = "test_file_penalty";
const SCORE_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy)]
struct SignalScore {
    raw: f64,
    clamped: f64,
    effective: f64,
}

#[derive(Debug, Clone)]
struct BudgetedScoreBreakdown {
    bm25: SignalScore,
    exact_match: SignalScore,
    qualified_name: SignalScore,
    path_affinity: SignalScore,
    definition_boost: SignalScore,
    kind_match: SignalScore,
    test_file_penalty: SignalScore,
    precedence_audit: RankingPrecedenceAudit,
}

#[derive(Debug, Clone, Copy)]
struct RawSignalInputs {
    bm25: f64,
    exact_match: f64,
    qualified_name: f64,
    path_affinity: f64,
    definition_boost: f64,
    kind_match: f64,
    test_file_penalty: f64,
}

impl BudgetedScoreBreakdown {
    fn final_score(&self) -> f64 {
        self.bm25.effective
            + self.exact_match.effective
            + self.qualified_name.effective
            + self.path_affinity.effective
            + self.definition_boost.effective
            + self.kind_match.effective
            + self.test_file_penalty.effective
    }

    fn exact_match_present(&self) -> bool {
        self.precedence_audit.exact_match_present
    }

    fn to_reason(&self, result_index: usize, result_id: String) -> RankingReasons {
        RankingReasons {
            result_index,
            result_id,
            // Keep legacy fields aligned with pre-budget semantics (raw signal values)
            // for backward compatibility with existing consumers.
            exact_match_boost: self.exact_match.raw,
            qualified_name_boost: self.qualified_name.raw,
            path_affinity: self.path_affinity.raw,
            definition_boost: self.definition_boost.raw,
            kind_match: self.kind_match.raw,
            test_file_penalty: self.test_file_penalty.raw,
            confidence_structural_boost: 0.0,
            structural_weighted_centrality: 0.0,
            structural_raw_centrality: 0.0,
            structural_guardrail_multiplier: 1.0,
            confidence_coverage: 1.0,
            bm25_score: self.bm25.raw,
            final_score: self.final_score(),
            signal_contributions: vec![
                signal_contribution(SIGNAL_BM25, self.bm25),
                signal_contribution(SIGNAL_EXACT_MATCH, self.exact_match),
                signal_contribution(SIGNAL_QUALIFIED_NAME, self.qualified_name),
                signal_contribution(SIGNAL_PATH_AFFINITY, self.path_affinity),
                signal_contribution(SIGNAL_DEFINITION_BOOST, self.definition_boost),
                signal_contribution(SIGNAL_KIND_MATCH, self.kind_match),
                signal_contribution(SIGNAL_TEST_FILE_PENALTY, self.test_file_penalty),
            ],
            precedence_audit: Some(self.precedence_audit.clone()),
        }
    }
}

pub fn kind_weight(kind: &str) -> f64 {
    match kind.trim().to_ascii_lowercase().as_str() {
        "class" | "interface" | "trait" => 2.0,
        "struct" | "enum" => 1.8,
        "type_alias" | "function" | "method" => 1.5,
        "constant" => 1.0,
        "module" => 0.8,
        "variable" => 0.5,
        _ => 0.0,
    }
}

pub fn query_intent_boost(query: &str, kind: &str) -> f64 {
    let query = query.trim();
    if query.is_empty() {
        return 0.0;
    }
    let query_starts_upper = query.chars().next().is_some_and(char::is_uppercase);
    let query_has_underscore = query.contains('_');
    let query_starts_lower = query.chars().next().is_some_and(char::is_lowercase);
    let kind_lower = kind.trim().to_ascii_lowercase();

    let is_type_kind = matches!(
        kind_lower.as_str(),
        "class" | "struct" | "enum" | "trait" | "interface" | "type_alias"
    );
    let is_callable_kind = matches!(kind_lower.as_str(), "function" | "method");

    if query_starts_upper && !query_has_underscore && is_type_kind {
        return 1.0;
    }

    if (query_starts_lower || query_has_underscore) && is_callable_kind {
        return 0.5;
    }

    0.0
}

pub fn test_file_penalty(path: &str) -> f64 {
    let lower = path.to_ascii_lowercase();
    const TEST_FILE_PATTERNS: [&str; 6] =
        ["_test.", ".test.", ".spec.", "/test/", "/tests/", "test_"];
    if TEST_FILE_PATTERNS.iter().any(|pat| lower.contains(pat)) {
        -0.5
    } else {
        0.0
    }
}

/// Apply rule-based reranking boosts to search results.
pub fn rerank(results: &mut [SearchResult], query: &str) {
    rerank_with_budget(results, query, &RankingSignalBudgetConfig::default());
}

/// Apply rule-based reranking with explicit signal-budget configuration.
pub fn rerank_with_budget(
    results: &mut [SearchResult],
    query: &str,
    budgets: &RankingSignalBudgetConfig,
) {
    let _ = rerank_inner(results, query, budgets, false);
}

/// Apply reranking and collect per-result ranking explanations.
pub fn rerank_with_reasons(results: &mut [SearchResult], query: &str) -> Vec<RankingReasons> {
    rerank_with_reasons_with_budget(results, query, &RankingSignalBudgetConfig::default())
}

/// Apply reranking with explicit signal-budget configuration and collect explain payloads.
pub fn rerank_with_reasons_with_budget(
    results: &mut [SearchResult],
    query: &str,
    budgets: &RankingSignalBudgetConfig,
) -> Vec<RankingReasons> {
    rerank_inner(results, query, budgets, true)
}

fn rerank_inner(
    results: &mut [SearchResult],
    query: &str,
    budgets: &RankingSignalBudgetConfig,
    collect_reasons: bool,
) -> Vec<RankingReasons> {
    let query_lower = query.to_lowercase();
    let mut reasons = Vec::with_capacity(results.len());
    let mut exact_match_flags = Vec::with_capacity(results.len());

    for (idx, result) in results.iter_mut().enumerate() {
        let bm25_score = result.score as f64;
        let mut exact_match_raw = 0.0_f64;
        let mut qualified_name_raw = 0.0_f64;
        let mut definition_boost_raw = 0.0_f64;
        let mut path_affinity_raw = 0.0_f64;
        let kind_match_raw = result
            .kind
            .as_deref()
            .map(|kind| kind_weight(kind) + query_intent_boost(query, kind))
            .unwrap_or(0.0);
        let test_file_penalty_raw = test_file_penalty(&result.path);

        // Exact symbol name match boost
        if let Some(ref name) = result.name
            && name.to_lowercase() == query_lower
        {
            exact_match_raw = budgets.exact_match.default;
        }

        // Qualified name match boost
        if let Some(ref qn) = result.qualified_name
            && qn.to_lowercase().contains(&query_lower)
        {
            qualified_name_raw = budgets.qualified_name.default;
        }

        // Definition-over-reference boost (definitions are kind != "reference")
        if result.result_type == "symbol" {
            definition_boost_raw = budgets.definition_boost.default;
        }

        // Path affinity boost (if query partially matches path)
        if result.path.to_lowercase().contains(&query_lower) {
            path_affinity_raw = budgets.path_affinity.default;
        }

        let breakdown = budgeted_breakdown(
            RawSignalInputs {
                bm25: bm25_score,
                exact_match: exact_match_raw,
                qualified_name: qualified_name_raw,
                path_affinity: path_affinity_raw,
                definition_boost: definition_boost_raw,
                kind_match: kind_match_raw,
                test_file_penalty: test_file_penalty_raw,
            },
            budgets,
        );
        result.score = breakdown.final_score() as f32;
        exact_match_flags.push(breakdown.exact_match_present());
        if collect_reasons {
            reasons.push(breakdown.to_reason(idx, result.result_id.clone()));
        }
    }

    let mut sort_order: Vec<usize> = (0..results.len()).collect();
    sort_order.sort_by(|&a, &b| {
        exact_match_flags[b]
            .cmp(&exact_match_flags[a])
            .then_with(|| {
                results[b]
                    .score
                    .partial_cmp(&results[a].score)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| results[a].result_id.cmp(&results[b].result_id))
    });

    let mut old_to_new = vec![0usize; results.len()];
    for (new_idx, &old_idx) in sort_order.iter().enumerate() {
        old_to_new[old_idx] = new_idx;
    }
    reorder_in_place(results, old_to_new);

    if !collect_reasons {
        return Vec::new();
    }

    let mut sorted_reasons = Vec::with_capacity(reasons.len());
    for (new_idx, &orig_idx) in sort_order.iter().enumerate() {
        let mut reason = reasons[orig_idx].clone();
        reason.result_index = new_idx;
        sorted_reasons.push(reason);
    }
    sorted_reasons
}

/// Generate ranking reasons for locate_symbol results.
///
/// `locate_symbol` uses exact-match queries, so all results have
/// `exact_match_boost = 1.0` and `definition_boost = 1.0` by definition.
pub fn locate_ranking_reasons(results: &[LocateResult], query: &str) -> Vec<RankingReasons> {
    locate_ranking_reasons_with_budget(results, query, &RankingSignalBudgetConfig::default())
}

/// Generate ranking reasons for locate_symbol results with explicit signal budgets.
pub fn locate_ranking_reasons_with_budget(
    results: &[LocateResult],
    query: &str,
    budgets: &RankingSignalBudgetConfig,
) -> Vec<RankingReasons> {
    let query_lower = query.to_lowercase();
    results
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let bm25_score = r.score as f64;
            let exact_match_raw = if r.name.to_lowercase() == query_lower {
                budgets.exact_match.default
            } else {
                0.0
            };
            let qualified_name_raw = if r.qualified_name.to_lowercase().contains(&query_lower) {
                budgets.qualified_name.default
            } else {
                0.0
            };
            let definition_boost_raw = budgets.definition_boost.default; // locate always returns definitions
            let path_affinity_raw = if r.path.to_lowercase().contains(&query_lower) {
                budgets.path_affinity.default
            } else {
                0.0
            };
            let kind_match_raw = kind_weight(&r.kind) + query_intent_boost(query, &r.kind);
            let test_file_penalty_raw = test_file_penalty(&r.path);
            budgeted_breakdown(
                RawSignalInputs {
                    bm25: bm25_score,
                    exact_match: exact_match_raw,
                    qualified_name: qualified_name_raw,
                    path_affinity: path_affinity_raw,
                    definition_boost: definition_boost_raw,
                    kind_match: kind_match_raw,
                    test_file_penalty: test_file_penalty_raw,
                },
                budgets,
            )
            .to_reason(idx, format!("{}:{}:{}", r.path, r.line_start, r.name))
        })
        .collect()
}

/// Convert full ranking reasons to compact normalized factors used by
/// `ranking_explain_level = "basic"`.
pub fn to_basic_ranking_reasons(reasons: &[RankingReasons]) -> Vec<BasicRankingReasons> {
    reasons
        .iter()
        .map(|r| BasicRankingReasons {
            result_index: r.result_index,
            exact_match: r.exact_match_boost,
            path_boost: r.path_affinity,
            definition_boost: r.definition_boost,
            // 008 semantic retrieval is out of scope for this change. Use the
            // existing qualified-name lexical signal as the semantic proxy in
            // the basic explainability payload.
            semantic_similarity: r.qualified_name_boost,
            final_score: r.final_score,
        })
        .collect()
}

fn budgeted_breakdown(
    raw: RawSignalInputs,
    budgets: &RankingSignalBudgetConfig,
) -> BudgetedScoreBreakdown {
    let bm25 = score_without_clamp(raw.bm25);
    let exact_match = score_with_budget(raw.exact_match, &budgets.exact_match);
    let qualified_name = score_with_budget(raw.qualified_name, &budgets.qualified_name);
    let mut path_affinity = score_with_budget(raw.path_affinity, &budgets.path_affinity);
    let mut definition_boost = score_with_budget(raw.definition_boost, &budgets.definition_boost);
    let mut kind_match = score_with_budget(raw.kind_match, &budgets.kind_match);
    let test_file_penalty = score_with_budget(raw.test_file_penalty, &budgets.test_file_penalty);

    let exact_match_present = exact_match.effective > SCORE_EPSILON;
    let mut lexical_dominance_applied = false;
    let mut secondary_cap = positive_secondary_total(path_affinity, definition_boost, kind_match);

    if exact_match_present {
        secondary_cap = budgets.secondary_cap_when_exact.default.clamp(
            budgets.secondary_cap_when_exact.min,
            budgets.secondary_cap_when_exact.max,
        );
        let raw_secondary = positive_secondary_total(path_affinity, definition_boost, kind_match);
        if raw_secondary > secondary_cap && raw_secondary > SCORE_EPSILON {
            let scale = secondary_cap / raw_secondary;
            path_affinity.effective = scale_positive(path_affinity.clamped, scale);
            definition_boost.effective = scale_positive(definition_boost.clamped, scale);
            kind_match.effective = scale_positive(kind_match.clamped, scale);
            lexical_dominance_applied = true;
        }
    }

    let secondary_effective_total =
        positive_secondary_total(path_affinity, definition_boost, kind_match);
    let precedence_audit = RankingPrecedenceAudit {
        lexical_dominance_applied,
        exact_match_present,
        secondary_effective_total,
        secondary_effective_cap: if exact_match_present {
            secondary_cap
        } else {
            secondary_effective_total
        },
    };

    BudgetedScoreBreakdown {
        bm25,
        exact_match,
        qualified_name,
        path_affinity,
        definition_boost,
        kind_match,
        test_file_penalty,
        precedence_audit,
    }
}

fn score_with_budget(raw: f64, budget: &RankingSignalBudgetRange) -> SignalScore {
    let clamped = raw.clamp(budget.min, budget.max);
    SignalScore {
        raw,
        clamped,
        effective: clamped,
    }
}

fn score_without_clamp(raw: f64) -> SignalScore {
    SignalScore {
        raw,
        clamped: raw,
        effective: raw,
    }
}

fn signal_contribution(signal: &str, score: SignalScore) -> RankingSignalContribution {
    RankingSignalContribution {
        signal: signal.to_string(),
        raw_value: score.raw,
        clamped_value: score.clamped,
        effective_value: score.effective,
    }
}

fn positive_secondary_total(path: SignalScore, definition: SignalScore, kind: SignalScore) -> f64 {
    path.effective.max(0.0) + definition.effective.max(0.0) + kind.effective.max(0.0)
}

fn scale_positive(value: f64, scale: f64) -> f64 {
    if value > 0.0 { value * scale } else { value }
}

fn reorder_in_place<T>(items: &mut [T], mut old_to_new: Vec<usize>) {
    debug_assert_eq!(items.len(), old_to_new.len());
    for index in 0..items.len() {
        while old_to_new[index] != index {
            let target = old_to_new[index];
            items.swap(index, target);
            old_to_new.swap(index, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_result(
        id: &str,
        name: &str,
        qn: &str,
        path: &str,
        kind: &str,
        score: f32,
    ) -> SearchResult {
        SearchResult {
            repo: "repo".to_string(),
            result_id: id.to_string(),
            symbol_id: Some(format!("sym-{id}")),
            symbol_stable_id: Some(format!("stable-{id}")),
            result_type: "symbol".to_string(),
            path: path.to_string(),
            line_start: 1,
            line_end: 2,
            kind: Some(kind.to_string()),
            name: Some(name.to_string()),
            qualified_name: Some(qn.to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score,
            snippet: None,
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        }
    }

    #[test]
    fn kind_weight_prefers_type_symbols_over_values() {
        assert!(kind_weight("class") > kind_weight("function"));
        assert!(kind_weight("function") > kind_weight("variable"));
    }

    #[test]
    fn query_intent_boost_detects_type_and_callable_hints() {
        assert_eq!(query_intent_boost("AuthService", "class"), 1.0);
        assert_eq!(query_intent_boost("validate_token", "function"), 0.5);
        assert_eq!(query_intent_boost("auth", "class"), 0.0);
    }

    #[test]
    fn test_file_penalty_triggers_once_per_path() {
        assert_eq!(test_file_penalty("src/auth/user_test.rs"), -0.5);
        assert_eq!(test_file_penalty("src/auth/user.spec.ts"), -0.5);
        assert_eq!(test_file_penalty("src/auth/user.rs"), 0.0);
    }

    #[test]
    fn budget_registry_defaults_are_canonical() {
        let budgets = RankingSignalBudgetConfig::default();
        assert!(budgets.exact_match.min <= budgets.exact_match.default);
        assert!(budgets.exact_match.default <= budgets.exact_match.max);
        assert!(budgets.test_file_penalty.min < 0.0);
        assert_eq!(budgets.exact_match.default, 5.0);
        assert_eq!(budgets.secondary_cap_when_exact.default, 2.0);
    }

    #[test]
    fn budgeted_scoring_clamps_out_of_range_signals() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.kind_match.min = 0.0;
        budgets.kind_match.max = 0.75;
        budgets.kind_match.default = 0.75;
        budgets.qualified_name.default = 10.0; // raw > max -> clamped
        budgets.qualified_name.max = 1.25;

        let mut results = vec![search_result(
            "a",
            "validate_token",
            "auth::validate_token",
            "src/auth/validate.rs",
            "class",
            0.1,
        )];
        let reasons = rerank_with_reasons_with_budget(&mut results, "validate_token", &budgets);
        let reason = reasons.first().unwrap();
        let kind = reason
            .signal_contributions
            .iter()
            .find(|c| c.signal == SIGNAL_KIND_MATCH)
            .unwrap();
        assert!(kind.clamped_value <= 0.75 + SCORE_EPSILON);
        assert!(reason.kind_match >= kind.clamped_value);
        let qualified = reason
            .signal_contributions
            .iter()
            .find(|c| c.signal == SIGNAL_QUALIFIED_NAME)
            .unwrap();
        assert!(qualified.raw_value > qualified.clamped_value);
        assert!((qualified.clamped_value - 1.25).abs() < SCORE_EPSILON);
    }

    #[test]
    fn exact_lexical_match_remains_dominant_under_precedence_guard() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.exact_match.default = 1.0;
        budgets.secondary_cap_when_exact.default = 0.5;
        budgets.kind_match.default = 3.0;
        budgets.path_affinity.default = 2.0;
        budgets.definition_boost.default = 2.0;

        let exact = search_result(
            "exact",
            "validate_token",
            "auth::validate_token",
            "src/auth/validate.rs",
            "function",
            0.01,
        );
        let structural = search_result(
            "structural",
            "helper",
            "auth::helper",
            "src/auth/validate_token_helpers.rs",
            "class",
            0.01,
        );
        let mut results = vec![structural, exact];
        let reasons = rerank_with_reasons_with_budget(&mut results, "validate_token", &budgets);

        assert_eq!(results[0].result_id, "exact");
        assert_eq!(reasons[0].result_index, 0);
        assert!(
            reasons[0]
                .precedence_audit
                .as_ref()
                .unwrap()
                .exact_match_present
        );
    }

    #[test]
    fn zoekt_style_conservative_secondary_boost_fixture() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.secondary_cap_when_exact.min = 0.0;
        budgets.secondary_cap_when_exact.default = 0.25;
        budgets.kind_match.max = 3.0;
        budgets.path_affinity.max = 2.0;
        budgets.definition_boost.max = 2.0;

        let mut results = vec![search_result(
            "a",
            "validate_token",
            "auth::validate_token",
            "src/auth/validate.rs",
            "class",
            0.0,
        )];
        let reasons = rerank_with_reasons_with_budget(&mut results, "validate_token", &budgets);
        let audit = reasons[0].precedence_audit.as_ref().unwrap();
        assert!(audit.lexical_dominance_applied);
        assert!(
            audit.secondary_effective_total <= 0.25 + 1e-6,
            "secondary_effective_total={}",
            audit.secondary_effective_total
        );
    }

    #[test]
    fn deterministic_order_uses_result_id_on_equal_precedence_and_score() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.path_affinity.default = 0.0;
        budgets.definition_boost.default = 0.0;
        budgets.kind_match.default = 0.0;
        budgets.qualified_name.default = 0.0;
        budgets.exact_match.default = 0.0;

        let mut results = vec![
            search_result("b-id", "foo", "foo", "src/a.rs", "function", 1.0),
            search_result("a-id", "bar", "bar", "src/b.rs", "function", 1.0),
        ];
        rerank_with_budget(&mut results, "nomatch", &budgets);
        assert_eq!(results[0].result_id, "a-id");
        assert_eq!(results[1].result_id, "b-id");
    }
}
