use crate::locate::LocateResult;
use crate::search::SearchResult;
use cruxe_core::config::{RankingSignalBudgetConfig, RankingSignalBudgetRange};
use cruxe_core::types::{
    BasicRankingReasons, RankingPrecedenceAudit, RankingReasons, RankingSignalContribution,
    SymbolKind, SymbolRole,
};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use tracing::warn;

const SIGNAL_BM25: &str = "bm25_score";
const SIGNAL_EXACT_MATCH: &str = "exact_match_boost";
const SIGNAL_QUALIFIED_NAME: &str = "qualified_name_boost";
const SIGNAL_PATH_AFFINITY: &str = "path_affinity";
const SIGNAL_DEFINITION_BOOST: &str = "definition_boost";
const SIGNAL_KIND_MATCH: &str = "kind_match";
const SIGNAL_TEST_FILE_PENALTY: &str = "test_file_penalty";
const SIGNAL_ROLE_WEIGHT: &str = "role_weight";
const SIGNAL_KIND_ADJUSTMENT: &str = "kind_adjustment";
const SIGNAL_ADAPTIVE_PRIOR: &str = "adaptive_prior";
const SIGNAL_PUBLIC_SURFACE_SALIENCE: &str = "public_surface_salience";
const SCORE_EPSILON: f64 = 1e-9;
const KIND_ADJUSTMENT_BOUND: f64 = 0.2;
const ADAPTIVE_PRIOR_BOUND: f64 = 0.25;
const ADAPTIVE_PRIOR_MIN_SAMPLE: usize = 12;
const PUBLIC_SURFACE_SALIENCE_BOUND: f64 = 0.3;

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

#[derive(Debug, Clone, Copy, Default)]
struct UniversalPriorComponents {
    role_weight: f64,
    kind_adjustment: f64,
    adaptive_prior: f64,
    public_surface_salience: f64,
}

#[derive(Debug, Clone, Default)]
struct RepositoryPriorStats {
    sample_count: usize,
    kind_counts: HashMap<String, usize>,
    median_frequency: f64,
    adaptive_enabled: bool,
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
                signal_contribution(SIGNAL_TEST_FILE_PENALTY, self.test_file_penalty),
            ],
            precedence_audit: Some(self.precedence_audit.clone()),
        }
    }
}

pub fn role_weight(role: SymbolRole) -> f64 {
    match role {
        SymbolRole::Type => 2.0,
        SymbolRole::Callable => 1.6,
        SymbolRole::Namespace => 1.2,
        SymbolRole::Value => 0.9,
        SymbolRole::Alias => 0.8,
    }
}

pub fn kind_adjustment(kind: &str) -> f64 {
    let normalized_kind = kind.trim().to_ascii_lowercase();
    let raw: f64 = match normalized_kind.as_str() {
        "class" | "interface" | "trait" => 0.20,
        "struct" | "enum" => 0.15,
        "function" | "method" => 0.10,
        "module" => 0.05,
        "type_alias" => -0.05,
        "constant" => 0.00,
        "variable" => -0.10,
        _ => {
            log_unknown_kind_once(&normalized_kind);
            0.0
        }
    };
    raw.clamp(-KIND_ADJUSTMENT_BOUND, KIND_ADJUSTMENT_BOUND)
}

pub fn kind_weight(kind: &str) -> f64 {
    role_weight_for_kind(kind) + kind_adjustment(kind)
}

fn log_unknown_kind_once(kind: &str) {
    if kind.is_empty() {
        return;
    }
    static SEEN_UNKNOWN_KINDS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN_UNKNOWN_KINDS.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut guard) = seen.lock()
        && guard.insert(kind.to_string())
    {
        warn!(
            kind,
            "unknown symbol kind encountered during ranking; defaulting kind weight to 0.0"
        );
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

pub(crate) fn semantic_signal_adjustment(
    kind: Option<&str>,
    query: &str,
    path: &str,
    budgets: &RankingSignalBudgetConfig,
) -> f64 {
    let kind_match_raw = kind
        .map(|kind| {
            let adjustment = (kind_adjustment(kind) + query_intent_boost(query, kind))
                .clamp(-KIND_ADJUSTMENT_BOUND, KIND_ADJUSTMENT_BOUND);
            role_weight_for_kind(kind) + adjustment
        })
        .unwrap_or(0.0);
    let test_file_penalty_raw = test_file_penalty(path);

    score_with_budget(kind_match_raw, &budgets.kind_match).effective
        + score_with_budget(test_file_penalty_raw, &budgets.test_file_penalty).effective
}

fn role_weight_for_kind(kind: &str) -> f64 {
    SymbolKind::parse_kind(kind)
        .map(|kind| role_weight(kind.role()))
        .unwrap_or(0.0)
}

impl RepositoryPriorStats {
    fn from_results(results: &[SearchResult]) -> Self {
        let mut stats = RepositoryPriorStats::default();
        for result in results {
            if result.result_type != "symbol" {
                continue;
            }
            let Some(kind) = result.kind.as_deref() else {
                continue;
            };
            let normalized = kind.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            *stats.kind_counts.entry(normalized).or_insert(0) += 1;
            stats.sample_count += 1;
        }
        if stats.sample_count < ADAPTIVE_PRIOR_MIN_SAMPLE || stats.kind_counts.is_empty() {
            stats.adaptive_enabled = false;
            stats.median_frequency = 0.0;
            return stats;
        }

        let mut frequencies: Vec<f64> = stats
            .kind_counts
            .values()
            .map(|count| *count as f64 / stats.sample_count as f64)
            .collect();
        frequencies.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
        let mid = frequencies.len() / 2;
        stats.median_frequency = if frequencies.len().is_multiple_of(2) {
            (frequencies[mid - 1] + frequencies[mid]) / 2.0
        } else {
            frequencies[mid]
        };
        stats.adaptive_enabled = stats.median_frequency > SCORE_EPSILON;
        stats
    }

    fn rarity_boost(&self, kind: Option<&str>) -> f64 {
        if !self.adaptive_enabled {
            return 0.0;
        }
        let Some(kind) = kind else {
            return 0.0;
        };
        let normalized = kind.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return 0.0;
        }
        let count = self.kind_counts.get(&normalized).copied().unwrap_or(0);
        if count == 0 || self.sample_count == 0 {
            return 0.0;
        }
        let frequency = count as f64 / self.sample_count as f64;
        if self.median_frequency <= SCORE_EPSILON {
            return 0.0;
        }
        let rarity_score =
            ((self.median_frequency - frequency) / self.median_frequency).clamp(-1.0, 1.0);
        (rarity_score * ADAPTIVE_PRIOR_BOUND).clamp(-ADAPTIVE_PRIOR_BOUND, ADAPTIVE_PRIOR_BOUND)
    }
}

fn public_surface_boost(result: &SearchResult) -> f64 {
    let path_lower = result.path.to_ascii_lowercase();
    if path_lower.contains("/test/")
        || path_lower.contains("/tests/")
        || path_lower.contains(".test.")
        || path_lower.contains(".spec.")
        || path_lower.contains("/internal/")
    {
        return 0.0;
    }

    let mut boost = 0.0_f64;
    let top_level = result
        .qualified_name
        .as_deref()
        .is_some_and(is_top_level_symbol_name);
    if top_level {
        boost += 0.10;
    }
    if result
        .visibility
        .as_deref()
        .is_some_and(is_public_visibility)
    {
        boost += 0.05;
    }
    boost += result.file_centrality.clamp(0.0, 1.0) * 0.15;
    if path_lower.contains("/api/") || path_lower.contains("/public/") {
        boost += 0.05;
    }
    boost.clamp(0.0, PUBLIC_SURFACE_SALIENCE_BOUND)
}

fn is_top_level_symbol_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && !trimmed.contains("::") && !trimmed.contains('.')
}

fn is_public_visibility(visibility: &str) -> bool {
    matches!(
        visibility.trim().to_ascii_lowercase().as_str(),
        "pub" | "public" | "export"
    )
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
    let prior_stats = RepositoryPriorStats::from_results(results);
    let mut reasons = Vec::with_capacity(results.len());
    let mut exact_match_flags = Vec::with_capacity(results.len());

    for (idx, result) in results.iter_mut().enumerate() {
        let bm25_score = result.score as f64;
        let mut exact_match_raw = 0.0_f64;
        let mut qualified_name_raw = 0.0_f64;
        let mut definition_boost_raw = 0.0_f64;
        let mut path_affinity_raw = 0.0_f64;
        let role_weight_raw = result
            .kind
            .as_deref()
            .map(role_weight_for_kind)
            .unwrap_or(0.0);
        let kind_adjustment_raw = result
            .kind
            .as_deref()
            .map(|kind| {
                (kind_adjustment(kind) + query_intent_boost(query, kind))
                    .clamp(-KIND_ADJUSTMENT_BOUND, KIND_ADJUSTMENT_BOUND)
            })
            .unwrap_or(0.0);
        let adaptive_prior_raw = prior_stats.rarity_boost(result.kind.as_deref());
        let public_surface_salience_raw = public_surface_boost(result);
        let kind_match_raw = role_weight_raw
            + kind_adjustment_raw
            + adaptive_prior_raw
            + public_surface_salience_raw;
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
            let mut reason = breakdown.to_reason(idx, result.result_id.clone());
            add_universal_prior_signal_contributions(
                &mut reason.signal_contributions,
                UniversalPriorComponents {
                    role_weight: role_weight_raw,
                    kind_adjustment: kind_adjustment_raw,
                    adaptive_prior: adaptive_prior_raw,
                    public_surface_salience: public_surface_salience_raw,
                },
                breakdown.kind_match,
            );
            reasons.push(reason);
        }
    }

    let mut sort_order: Vec<usize> = (0..results.len()).collect();
    sort_order.sort_by(|&a, &b| {
        let score_a = finite_or_default(results[a].score as f64, f64::NEG_INFINITY);
        let score_b = finite_or_default(results[b].score as f64, f64::NEG_INFINITY);
        exact_match_flags[b]
            .cmp(&exact_match_flags[a])
            .then_with(|| score_b.partial_cmp(&score_a).unwrap_or(Ordering::Equal))
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
            let role_weight_raw = role_weight_for_kind(&r.kind);
            let kind_adjustment_raw = (kind_adjustment(&r.kind)
                + query_intent_boost(query, &r.kind))
            .clamp(-KIND_ADJUSTMENT_BOUND, KIND_ADJUSTMENT_BOUND);
            let adaptive_prior_raw = 0.0;
            let public_surface_salience_raw = 0.0;
            let kind_match_raw = role_weight_raw
                + kind_adjustment_raw
                + adaptive_prior_raw
                + public_surface_salience_raw;
            let test_file_penalty_raw = test_file_penalty(&r.path);
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
            let mut reason =
                breakdown.to_reason(idx, format!("{}:{}:{}", r.path, r.line_start, r.name));
            add_universal_prior_signal_contributions(
                &mut reason.signal_contributions,
                UniversalPriorComponents {
                    role_weight: role_weight_raw,
                    kind_adjustment: kind_adjustment_raw,
                    adaptive_prior: adaptive_prior_raw,
                    public_surface_salience: public_surface_salience_raw,
                },
                breakdown.kind_match,
            );
            reason
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
    let mut qualified_name = score_with_budget(raw.qualified_name, &budgets.qualified_name);
    let mut path_affinity = score_with_budget(raw.path_affinity, &budgets.path_affinity);
    let mut definition_boost = score_with_budget(raw.definition_boost, &budgets.definition_boost);
    let mut kind_match = score_with_budget(raw.kind_match, &budgets.kind_match);
    let test_file_penalty = score_with_budget(raw.test_file_penalty, &budgets.test_file_penalty);

    let exact_match_present = exact_match.effective > SCORE_EPSILON;
    let mut lexical_dominance_applied = false;
    let raw_secondary_total =
        positive_secondary_total(qualified_name, path_affinity, definition_boost, kind_match);
    let mut secondary_cap = raw_secondary_total;

    if exact_match_present {
        secondary_cap = score_with_budget(
            budgets.secondary_cap_when_exact.default,
            &budgets.secondary_cap_when_exact,
        )
        .clamped
        .max(0.0);
        let raw_secondary = raw_secondary_total;
        if raw_secondary > secondary_cap && raw_secondary > SCORE_EPSILON {
            let scale = secondary_cap / raw_secondary;
            qualified_name.effective = scale_positive(qualified_name.clamped, scale);
            path_affinity.effective = scale_positive(path_affinity.clamped, scale);
            definition_boost.effective = scale_positive(definition_boost.clamped, scale);
            kind_match.effective = scale_positive(kind_match.clamped, scale);
            lexical_dominance_applied = true;
        }
    }

    let secondary_effective_total = if lexical_dominance_applied {
        positive_secondary_total(qualified_name, path_affinity, definition_boost, kind_match)
    } else {
        raw_secondary_total
    };
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
    let raw = finite_or_default(raw, 0.0);
    let min = finite_or_default(budget.min, 0.0);
    let max = finite_or_default(budget.max, min);
    let (min, max) = if min <= max {
        (min, max)
    } else {
        tracing::warn!(
            budget_min = budget.min,
            budget_max = budget.max,
            "invalid ranking budget bounds in reranker; coercing to zero-range guard"
        );
        (0.0, 0.0)
    };
    let clamped = raw.clamp(min, max);
    SignalScore {
        raw,
        clamped,
        effective: clamped,
    }
}

fn score_without_clamp(raw: f64) -> SignalScore {
    let raw = finite_or_default(raw, 0.0);
    SignalScore {
        raw,
        clamped: raw,
        effective: raw,
    }
}

fn finite_or_default(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn signal_contribution(signal: &str, score: SignalScore) -> RankingSignalContribution {
    RankingSignalContribution {
        signal: signal.to_string(),
        raw_value: score.raw,
        clamped_value: score.clamped,
        effective_value: score.effective,
    }
}

fn add_universal_prior_signal_contributions(
    signal_contributions: &mut Vec<RankingSignalContribution>,
    components: UniversalPriorComponents,
    kind_match: SignalScore,
) {
    let raw_total = components.role_weight
        + components.kind_adjustment
        + components.adaptive_prior
        + components.public_surface_salience;

    // Keep an aggregate kind-match signal for compatibility while exposing
    // universal-prior sub-signals as decomposed contributors.
    signal_contributions.push(RankingSignalContribution {
        signal: SIGNAL_KIND_MATCH.to_string(),
        raw_value: raw_total,
        clamped_value: kind_match.clamped,
        effective_value: kind_match.effective,
    });

    for signal in [
        SIGNAL_ROLE_WEIGHT,
        SIGNAL_KIND_ADJUSTMENT,
        SIGNAL_ADAPTIVE_PRIOR,
        SIGNAL_PUBLIC_SURFACE_SALIENCE,
    ] {
        signal_contributions.push(RankingSignalContribution {
            signal: signal.to_string(),
            // Informational decomposition: avoid double-counting against the
            // aggregate kind_match signal in accounting totals.
            raw_value: 0.0,
            clamped_value: 0.0,
            effective_value: 0.0,
        });
    }
}

fn positive_secondary_total(
    qualified_name: SignalScore,
    path: SignalScore,
    definition: SignalScore,
    kind: SignalScore,
) -> f64 {
    qualified_name.effective.max(0.0)
        + path.effective.max(0.0)
        + definition.effective.max(0.0)
        + kind.effective.max(0.0)
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
            chunk_origin: None,
            file_centrality: 0.0,
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
    fn role_weight_drives_deterministic_order_when_lexical_signals_tie() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.exact_match.default = 0.0;
        budgets.qualified_name.default = 0.0;
        budgets.path_affinity.default = 0.0;
        budgets.definition_boost.default = 0.0;
        budgets.kind_match.default = 3.0;
        budgets.test_file_penalty.default = 0.0;
        let mut results = vec![
            search_result("value", "value", "value", "src/value.rs", "variable", 1.0),
            search_result("type", "type", "type", "src/type.rs", "class", 1.0),
        ];
        rerank_with_budget(&mut results, "nomatch", &budgets);
        assert_eq!(results[0].result_id, "type");
    }

    #[test]
    fn kind_adjustment_is_bounded_and_language_agnostic() {
        let fn_adjustment = kind_adjustment("function");
        let method_adjustment = kind_adjustment("method");
        assert!(fn_adjustment.abs() <= KIND_ADJUSTMENT_BOUND + SCORE_EPSILON);
        assert!(method_adjustment.abs() <= KIND_ADJUSTMENT_BOUND + SCORE_EPSILON);
        assert_eq!(kind_adjustment("function"), kind_adjustment("function"));
    }

    #[test]
    fn query_intent_boost_detects_type_and_callable_hints() {
        assert_eq!(query_intent_boost("AuthService", "class"), 1.0);
        assert_eq!(query_intent_boost("validate_token", "function"), 0.5);
        assert_eq!(query_intent_boost("auth", "class"), 0.0);
    }

    #[test]
    fn adaptive_prior_boosts_rare_kinds_and_penalizes_common_kinds() {
        let mut results = Vec::new();
        for idx in 0..9 {
            results.push(search_result(
                &format!("v{idx}"),
                "value",
                "value",
                "src/value.rs",
                "variable",
                0.1,
            ));
        }
        for idx in 0..3 {
            results.push(search_result(
                &format!("c{idx}"),
                "classy",
                "classy",
                "src/type.rs",
                "class",
                0.1,
            ));
        }
        let stats = RepositoryPriorStats::from_results(&results);
        assert!(stats.adaptive_enabled);
        let rare = stats.rarity_boost(Some("class"));
        let common = stats.rarity_boost(Some("variable"));
        assert!(rare > 0.0);
        assert!(common <= 0.0);
        assert!(rare <= ADAPTIVE_PRIOR_BOUND + SCORE_EPSILON);
        assert!(common >= -ADAPTIVE_PRIOR_BOUND - SCORE_EPSILON);
    }

    #[test]
    fn adaptive_prior_disables_with_small_sample_guard() {
        let results = vec![
            search_result("a", "a", "a", "src/a.rs", "class", 0.1),
            search_result("b", "b", "b", "src/b.rs", "variable", 0.1),
        ];
        let stats = RepositoryPriorStats::from_results(&results);
        assert!(!stats.adaptive_enabled);
        assert_eq!(stats.rarity_boost(Some("class")), 0.0);
        assert_eq!(stats.rarity_boost(Some("variable")), 0.0);
    }

    #[test]
    fn public_surface_salience_boosts_api_symbols_only() {
        let api = search_result(
            "api",
            "AuthService",
            "AuthService",
            "src/api/auth_service.rs",
            "class",
            0.1,
        );
        let mut api = api;
        api.visibility = Some("pub".to_string());
        api.file_centrality = 1.0;
        let internal = search_result(
            "internal",
            "helper",
            "auth::helper",
            "src/internal/auth_test.rs",
            "function",
            0.1,
        );
        assert!(public_surface_boost(&api) > 0.0);
        assert_eq!(public_surface_boost(&internal), 0.0);
    }

    #[test]
    fn file_centrality_breaks_lexical_ties_when_structure_differs() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.exact_match.default = 0.0;
        budgets.qualified_name.default = 0.0;
        budgets.path_affinity.default = 0.0;
        budgets.definition_boost.default = 0.0;
        budgets.kind_match.default = 0.0;
        budgets.test_file_penalty.default = 0.0;

        let mut low_centrality = search_result(
            "low_centrality",
            "helper",
            "module::helper",
            "src/internal/helper_low.rs",
            "function",
            1.0,
        );
        low_centrality.visibility = Some("private".to_string());
        low_centrality.file_centrality = 0.0;

        let mut high_centrality = search_result(
            "high_centrality",
            "helper",
            "module::helper",
            "src/internal/helper_high.rs",
            "function",
            1.0,
        );
        high_centrality.visibility = Some("private".to_string());
        high_centrality.file_centrality = 1.0;

        let mut results = vec![low_centrality, high_centrality];
        rerank_with_budget(&mut results, "nomatch", &budgets);
        assert_eq!(results[0].result_id, "high_centrality");
        assert_eq!(results[1].result_id, "low_centrality");
    }

    #[test]
    fn test_file_penalty_triggers_once_per_path() {
        assert_eq!(test_file_penalty("src/auth/user_test.rs"), -0.5);
        assert_eq!(test_file_penalty("src/auth/user.spec.ts"), -0.5);
        assert_eq!(test_file_penalty("src/auth/user.rs"), 0.0);
    }

    #[test]
    fn semantic_signal_adjustment_respects_kind_budget_caps() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.kind_match.min = 0.0;
        budgets.kind_match.max = 0.2;
        budgets.kind_match.default = 0.2;
        budgets.test_file_penalty.min = 0.0;
        budgets.test_file_penalty.max = 0.0;
        budgets.test_file_penalty.default = 0.0;

        let adjustment =
            semantic_signal_adjustment(Some("class"), "AuthService", "src/auth.rs", &budgets);
        assert!((adjustment - 0.2).abs() < SCORE_EPSILON);
    }

    #[test]
    fn semantic_signal_adjustment_respects_test_penalty_budget_caps() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.kind_match.min = 0.0;
        budgets.kind_match.max = 0.0;
        budgets.kind_match.default = 0.0;
        budgets.test_file_penalty.min = -0.1;
        budgets.test_file_penalty.max = 0.0;
        budgets.test_file_penalty.default = -0.1;

        let adjustment = semantic_signal_adjustment(
            Some("function"),
            "validate_token",
            "src/auth/user_test.rs",
            &budgets,
        );
        assert!((adjustment + 0.1).abs() < SCORE_EPSILON);
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

    #[test]
    fn nan_budget_values_do_not_poison_ranking_scores() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.exact_match.default = f64::NAN;
        budgets.path_affinity.min = f64::NAN;
        budgets.path_affinity.max = f64::NAN;

        let mut results = vec![search_result(
            "nan-guard",
            "validate_token",
            "auth::validate_token",
            "src/auth.rs",
            "function",
            2.0,
        )];
        rerank_with_budget(&mut results, "validate_token", &budgets);
        assert!(results[0].score.is_finite());
    }

    #[test]
    fn inf_budget_values_do_not_poison_ranking_scores() {
        let mut budgets = RankingSignalBudgetConfig::default();
        budgets.exact_match.default = f64::INFINITY;
        budgets.qualified_name.max = f64::INFINITY;
        budgets.path_affinity.min = f64::NEG_INFINITY;

        let mut results = vec![search_result(
            "inf-guard",
            "validate_token",
            "auth::validate_token",
            "src/auth.rs",
            "function",
            2.0,
        )];
        rerank_with_budget(&mut results, "validate_token", &budgets);
        assert!(results[0].score.is_finite());
    }

    #[test]
    fn nan_input_scores_sort_after_finite_scores() {
        let mut results = vec![
            search_result(
                "finite",
                "validate_token",
                "auth::validate_token",
                "src/auth.rs",
                "function",
                1.0,
            ),
            search_result(
                "nan",
                "other",
                "other::symbol",
                "src/other.rs",
                "function",
                f32::NAN,
            ),
        ];
        rerank_with_budget(
            &mut results,
            "validate_token",
            &RankingSignalBudgetConfig::default(),
        );
        assert_eq!(results[0].result_id, "finite");
    }
}
