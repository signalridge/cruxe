use crate::locate::LocateResult;
use crate::search::SearchResult;
use cruxe_core::types::{BasicRankingReasons, RankingReasons};

/// Apply rule-based reranking boosts to search results.
pub fn rerank(results: &mut [SearchResult], query: &str) {
    rerank_inner(results, query, false);
}

/// Apply reranking and collect per-result ranking explanations.
pub fn rerank_with_reasons(results: &mut [SearchResult], query: &str) -> Vec<RankingReasons> {
    rerank_inner(results, query, true)
}

fn rerank_inner(
    results: &mut [SearchResult],
    query: &str,
    collect_reasons: bool,
) -> Vec<RankingReasons> {
    let query_lower = query.to_lowercase();
    let mut reasons = Vec::new();

    for (idx, result) in results.iter_mut().enumerate() {
        let bm25_score = result.score as f64;
        let mut exact_match_boost = 0.0_f64;
        let mut qualified_name_boost = 0.0_f64;
        let mut definition_boost = 0.0_f64;
        let mut path_affinity = 0.0_f64;
        let kind_match = 0.0_f64; // Reserved for future kind-based scoring

        // Exact symbol name match boost
        if let Some(ref name) = result.name
            && name.to_lowercase() == query_lower
        {
            exact_match_boost = 5.0;
        }

        // Qualified name match boost
        if let Some(ref qn) = result.qualified_name
            && qn.to_lowercase().contains(&query_lower)
        {
            qualified_name_boost = 2.0;
        }

        // Definition-over-reference boost (definitions are kind != "reference")
        if result.result_type == "symbol" {
            definition_boost = 1.0;
        }

        // Path affinity boost (if query partially matches path)
        if result.path.to_lowercase().contains(&query_lower) {
            path_affinity = 1.0;
        }

        let boost =
            (exact_match_boost + qualified_name_boost + definition_boost + path_affinity) as f32;
        result.score += boost;

        if collect_reasons {
            reasons.push(RankingReasons {
                result_index: idx,
                exact_match_boost,
                qualified_name_boost,
                path_affinity,
                definition_boost,
                kind_match,
                bm25_score,
                final_score: result.score as f64,
            });
        }
    }

    // Re-sort by score, with stable tiebreaker on result_id for determinism
    if collect_reasons {
        // When collecting reasons, we need to keep result_index in sync after sorting
        let mut indexed: Vec<(usize, &mut SearchResult)> = results.iter_mut().enumerate().collect();
        indexed.sort_by(|(_, a), (_, b)| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.result_id.cmp(&b.result_id))
        });

        // Update result_index in reasons to reflect post-sort order
        let sort_order: Vec<usize> = indexed.iter().map(|(orig_idx, _)| *orig_idx).collect();

        // Remap reasons to match final sorted order
        let mut sorted_reasons = Vec::with_capacity(reasons.len());
        for (new_idx, &orig_idx) in sort_order.iter().enumerate() {
            let mut r = reasons[orig_idx].clone();
            r.result_index = new_idx;
            sorted_reasons.push(r);
        }
        reasons = sorted_reasons;
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.result_id.cmp(&b.result_id))
    });

    reasons
}

/// Generate ranking reasons for locate_symbol results.
///
/// `locate_symbol` uses exact-match queries, so all results have
/// `exact_match_boost = 1.0` and `definition_boost = 1.0` by definition.
pub fn locate_ranking_reasons(results: &[LocateResult], query: &str) -> Vec<RankingReasons> {
    let query_lower = query.to_lowercase();
    results
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let bm25_score = r.score as f64;
            let exact_match_boost = if r.name.to_lowercase() == query_lower {
                5.0
            } else {
                0.0
            };
            let qualified_name_boost = if r.qualified_name.to_lowercase().contains(&query_lower) {
                2.0
            } else {
                0.0
            };
            let definition_boost = 1.0; // locate always returns definitions
            let path_affinity = if r.path.to_lowercase().contains(&query_lower) {
                1.0
            } else {
                0.0
            };
            let kind_match = 0.0;
            let final_score = bm25_score
                + exact_match_boost
                + qualified_name_boost
                + definition_boost
                + path_affinity;
            RankingReasons {
                result_index: idx,
                exact_match_boost,
                qualified_name_boost,
                path_affinity,
                definition_boost,
                kind_match,
                bm25_score,
                final_score,
            }
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
