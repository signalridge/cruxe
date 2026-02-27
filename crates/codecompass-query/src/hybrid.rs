use crate::search::{RRF_K, SearchResult};
use codecompass_core::config::SearchConfig;
use codecompass_core::error::StateError;
use codecompass_state::embedding;
use codecompass_state::vector_index::{self, VectorQuery};
use rusqlite::Connection;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Lexical results arrive pre-scored (RRF per-index already applied in search.rs).
/// `blend_hybrid_results` treats them as *rank-ordered* from that scoring, so it
/// re-computes a fresh RRF rank over the sorted input — this is intentional: the
/// per-index RRF merges three Tantivy indices while hybrid RRF fuses lexical vs
/// semantic *channels*, which are conceptually distinct fusion stages.

#[derive(Debug, Clone)]
pub struct SemanticQueryOutput {
    pub results: Vec<SearchResult>,
    pub external_provider_blocked: bool,
}

pub fn semantic_query(
    conn: &Connection,
    search_config: &SearchConfig,
    query: &str,
    ref_name: &str,
    project_id: &str,
    semantic_limit: usize,
) -> Result<SemanticQueryOutput, StateError> {
    if semantic_limit == 0 || query.trim().is_empty() {
        return Ok(SemanticQueryOutput {
            results: Vec::new(),
            external_provider_blocked: false,
        });
    }

    // Provider construction is lightweight for the local (FastEmbed) backend
    // since model weights are internally cached by the fastembed crate.
    let built_provider = embedding::build_embedding_provider(&search_config.semantic)?;
    let external_provider_blocked = built_provider.external_provider_blocked;
    let mut provider = built_provider.provider;
    let query_embedding = provider.embed_batch(&[query.to_string()])?;
    let query_vector = query_embedding.into_iter().next().unwrap_or_default();
    if query_vector.is_empty() {
        return Ok(SemanticQueryOutput {
            results: Vec::new(),
            external_provider_blocked,
        });
    }

    let matches = vector_index::query_nearest_with_backend(
        conn,
        &VectorQuery {
            project_id: project_id.to_string(),
            ref_name: ref_name.to_string(),
            embedding_model_version: provider.model_version().to_string(),
            query_vector,
            limit: semantic_limit,
        },
        search_config.semantic.vector_backend_opt(),
    )?;

    Ok(SemanticQueryOutput {
        results: matches
            .into_iter()
            .map(|matched| {
                let result_id = semantic_result_id(
                    &matched.symbol_stable_id,
                    &matched.path,
                    matched.line_start,
                    matched.line_end,
                );
                SearchResult {
                    repo: project_id.to_string(),
                    result_id,
                    symbol_id: None,
                    symbol_stable_id: Some(matched.symbol_stable_id),
                    result_type: "symbol".to_string(),
                    path: matched.path,
                    line_start: matched.line_start,
                    line_end: matched.line_end,
                    kind: None,
                    name: None,
                    qualified_name: None,
                    language: matched.language,
                    signature: None,
                    visibility: None,
                    score: matched.score as f32,
                    snippet: Some(matched.snippet_text),
                    chunk_type: matched.chunk_type,
                    source_layer: None,
                    provenance: "semantic".to_string(),
                }
            })
            .collect(),
        external_provider_blocked,
    })
}

pub fn blend_hybrid_results(
    lexical_results: Vec<SearchResult>,
    semantic_results: Vec<SearchResult>,
    semantic_ratio_used: f64,
    lexical_fanout: usize,
    semantic_fanout: usize,
) -> Vec<SearchResult> {
    if lexical_results.is_empty() {
        return semantic_results;
    }
    if semantic_results.is_empty() || semantic_ratio_used <= 0.0 {
        return lexical_results;
    }

    let semantic_ratio = semantic_ratio_used.clamp(0.0, 1.0);
    let lexical_weight = (1.0 - semantic_ratio).max(0.0);
    let semantic_weight = semantic_ratio;

    let lexical = dedup_branch(lexical_results, lexical_fanout);
    let semantic = dedup_branch(semantic_results, semantic_fanout);

    let mut by_key: HashMap<String, HybridAccumulator> = HashMap::new();
    for (rank, result) in lexical.into_iter().enumerate() {
        let key = dedup_key(&result);
        let lexical_score = lexical_weight / (RRF_K + (rank + 1) as f64);
        let entry = by_key.entry(key).or_default();
        entry.lexical_score = lexical_score;
        if entry.lexical.is_none() {
            entry.lexical = Some(result);
        }
    }
    for (rank, result) in semantic.into_iter().enumerate() {
        let key = dedup_key(&result);
        let semantic_score = semantic_weight / (RRF_K + (rank + 1) as f64);
        let entry = by_key.entry(key).or_default();
        entry.semantic_score = semantic_score;
        if entry.semantic.is_none() {
            entry.semantic = Some(result);
        }
    }

    let mut blended = Vec::with_capacity(by_key.len());
    for mut entry in by_key.into_values() {
        let blended_score = entry.lexical_score + entry.semantic_score;
        let mut result = match (entry.lexical.take(), entry.semantic.take()) {
            (Some(mut lexical), Some(semantic)) => {
                if lexical.snippet.as_deref().unwrap_or_default().is_empty() {
                    lexical.snippet = semantic.snippet.clone();
                }
                lexical.provenance = "hybrid".to_string();
                lexical
            }
            (Some(mut lexical), None) => {
                lexical.provenance = "lexical".to_string();
                lexical
            }
            (None, Some(mut semantic)) => {
                semantic.provenance = "semantic".to_string();
                semantic
            }
            (None, None) => continue,
        };
        result.score = blended_score as f32;
        blended.push(result);
    }

    blended.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.result_id.cmp(&right.result_id))
    });
    blended
}

fn dedup_branch(results: Vec<SearchResult>, fanout: usize) -> Vec<SearchResult> {
    if fanout == 0 {
        return Vec::new();
    }
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for result in results.into_iter().take(fanout) {
        let key = dedup_key(&result);
        if seen.insert(key) {
            deduped.push(result);
        }
    }
    deduped
}

fn dedup_key(result: &SearchResult) -> String {
    if let Some(symbol_stable_id) = result
        .symbol_stable_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return format!("symbol:{symbol_stable_id}");
    }
    format!(
        "fallback:{}:{}:{}",
        result.path, result.line_start, result.line_end
    )
}

fn semantic_result_id(
    symbol_stable_id: &str,
    path: &str,
    line_start: u32,
    line_end: u32,
) -> String {
    let payload = format!(
        "semantic:v1|{}|{}|{}|{}",
        symbol_stable_id, path, line_start, line_end
    );
    format!("res_{}", blake3::hash(payload.as_bytes()).to_hex())
}

#[derive(Default)]
struct HybridAccumulator {
    lexical: Option<SearchResult>,
    semantic: Option<SearchResult>,
    lexical_score: f64,
    semantic_score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(
        result_id: &str,
        symbol_stable_id: Option<&str>,
        path: &str,
        score: f32,
        provenance: &str,
    ) -> SearchResult {
        SearchResult {
            repo: "proj".to_string(),
            result_id: result_id.to_string(),
            symbol_id: None,
            symbol_stable_id: symbol_stable_id.map(ToString::to_string),
            result_type: "symbol".to_string(),
            path: path.to_string(),
            line_start: 1,
            line_end: 1,
            kind: None,
            name: None,
            qualified_name: None,
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
    fn ratio_zero_returns_lexical_only() {
        let lexical = vec![
            make_result("l1", Some("sym-1"), "a.rs", 1.0, "lexical"),
            make_result("l2", Some("sym-2"), "b.rs", 0.9, "lexical"),
        ];
        let semantic = vec![make_result("s1", Some("sym-9"), "z.rs", 0.8, "semantic")];
        let blended = blend_hybrid_results(lexical.clone(), semantic, 0.0, 20, 20);
        assert_eq!(blended.len(), lexical.len());
        assert_eq!(blended[0].result_id, "l1");
        assert_eq!(blended[0].provenance, "lexical");
    }

    #[test]
    fn ratio_one_allows_semantic_dominant_ordering() {
        let lexical = vec![
            make_result("l1", Some("sym-1"), "a.rs", 1.0, "lexical"),
            make_result("l2", Some("sym-2"), "b.rs", 0.9, "lexical"),
        ];
        let semantic = vec![
            make_result("s1", Some("sym-9"), "z.rs", 1.0, "semantic"),
            make_result("s2", Some("sym-2"), "b.rs", 0.8, "semantic"),
        ];
        let blended = blend_hybrid_results(lexical, semantic, 1.0, 20, 20);
        assert_eq!(blended[0].symbol_stable_id.as_deref(), Some("sym-9"));
        assert_eq!(blended[0].provenance, "semantic");
    }

    #[test]
    fn ratio_half_balances_and_marks_hybrid_provenance() {
        let lexical = vec![
            make_result("l1", Some("sym-1"), "a.rs", 1.0, "lexical"),
            make_result("l2", Some("sym-2"), "b.rs", 0.9, "lexical"),
        ];
        let semantic = vec![
            make_result("s1", Some("sym-2"), "b.rs", 1.0, "semantic"),
            make_result("s2", Some("sym-3"), "c.rs", 0.95, "semantic"),
        ];
        let blended = blend_hybrid_results(lexical, semantic, 0.5, 20, 20);
        let sym2 = blended
            .iter()
            .find(|result| result.symbol_stable_id.as_deref() == Some("sym-2"))
            .unwrap();
        assert_eq!(sym2.provenance, "hybrid");
    }
}
