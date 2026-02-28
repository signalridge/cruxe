use cruxe_core::config::SearchConfig as CoreSearchConfig;
use cruxe_core::error::StateError;
use cruxe_core::types::{
    QueryIntent, RankingReasons, RefScope, SourceLayer, SymbolKind, SymbolRecord, SymbolRole,
};
use cruxe_state::tantivy_index::IndexSet;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::{IndexRecordOption, Value};
use tracing::{debug, warn};

use crate::confidence::evaluate_confidence;
use crate::hybrid::{blend_hybrid_results, semantic_query};
use crate::intent::{IntentPolicy, classify_intent_with_policy};
use crate::overlay_merge;
use crate::planner::build_plan_with_ref;
use crate::ranking::{
    kind_weight, query_intent_boost, rerank, rerank_with_reasons, test_file_penalty,
};
use crate::rerank::{RerankDocument, rerank_documents};
use crate::scoring::normalize_relevance_score;

/// Reciprocal Rank Fusion constant (standard value from the RRF paper).
/// Used by both per-index RRF in `search_code_with_options` and channel-level
/// RRF in `hybrid::blend_hybrid_results`.
pub(crate) const RRF_K: f64 = 60.0;

/// A search result from search_code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    #[serde(skip_serializing, skip_deserializing, default)]
    pub repo: String,
    pub result_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_stable_id: Option<String>,
    pub result_type: String,
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_name: Option<String>,
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub chunk_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_layer: Option<SourceLayer>,
    #[serde(default = "default_result_provenance")]
    pub provenance: String,
}

/// A suggested next action for the AI agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Response for search_code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query_intent: QueryIntent,
    pub total_candidates: usize,
    pub suggested_next_actions: Vec<SuggestedAction>,
    pub metadata: SearchMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<SearchDebugInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_reasons: Option<Vec<RankingReasons>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMetadata {
    pub semantic_mode: String,
    pub semantic_enabled: bool,
    pub semantic_ratio_used: f64,
    pub semantic_triggered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_skipped_reason: Option<String>,
    pub semantic_fallback: bool,
    pub semantic_degraded: bool,
    pub semantic_limit_used: usize,
    pub lexical_fanout_used: usize,
    pub semantic_fanout_used: usize,
    pub semantic_budget_exhausted: bool,
    pub external_provider_blocked: bool,
    pub embedding_model_version: String,
    pub rerank_provider: String,
    pub rerank_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerank_fallback_reason: Option<String>,
    pub low_confidence: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_action: Option<String>,
    pub confidence_threshold: f64,
    pub top_score: f64,
    pub score_margin: f64,
    pub channel_agreement: f64,
    pub query_intent_confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent_escalation_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchExecutionOptions {
    pub search_config: CoreSearchConfig,
    pub semantic_ratio_override: Option<f64>,
    pub confidence_threshold_override: Option<f64>,
    pub role: Option<String>,
}

/// Optional debug payload for search_code.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchDebugInfo {
    pub join_status: JoinStatus,
}

pub struct VcsSearchContext<'a> {
    pub base_index_set: &'a IndexSet,
    pub overlay_index_set: &'a IndexSet,
    pub tombstones: &'a HashSet<String>,
    pub base_ref: &'a str,
    pub target_ref: &'a str,
}

/// Join metrics for snippet -> symbol enrichment.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JoinStatus {
    pub hits: usize,
    pub misses: usize,
}

/// Execute a search across all indices.
pub fn search_code(
    index_set: &IndexSet,
    conn: Option<&Connection>,
    query: &str,
    r#ref: Option<&str>,
    language: Option<&str>,
    limit: usize,
    debug_ranking: bool,
) -> Result<SearchResponse, StateError> {
    search_code_with_options(
        index_set,
        conn,
        query,
        r#ref,
        language,
        limit,
        debug_ranking,
        SearchExecutionOptions::default(),
    )
}

/// Execute a search across all indices with runtime semantic/rerank overrides.
#[allow(clippy::too_many_arguments)]
pub fn search_code_with_options(
    index_set: &IndexSet,
    conn: Option<&Connection>,
    query: &str,
    r#ref: Option<&str>,
    language: Option<&str>,
    limit: usize,
    debug_ranking: bool,
    options: SearchExecutionOptions,
) -> Result<SearchResponse, StateError> {
    let mut debug = tracing::enabled!(tracing::Level::DEBUG).then_some(SearchDebugInfo::default());

    let intent_policy = IntentPolicy::from(&options.search_config.intent);
    let intent = classify_intent_with_policy(query, &intent_policy);
    let ref_scope = match r#ref {
        Some(explicit) => RefScope::explicit(explicit),
        None => RefScope::live(),
    };
    let plan = build_plan_with_ref(intent.intent, ref_scope);
    let effective_ref = plan.ref_scope.r#ref.clone();
    let search_ref = Some(effective_ref.as_str());
    let mut semantic_state = semantic_execution_state(&intent, &options);
    let mut semantic_limit_used = 0usize;
    let mut lexical_fanout_used = 0usize;
    let mut semantic_fanout_used = 0usize;
    let mut semantic_budget_exhausted = false;
    let mut response_warnings = Vec::new();

    let mut all_results = Vec::new();

    // Search each index and apply RRF (Reciprocal Rank Fusion) scoring.
    // RRF score per source = weight / (k + rank), where k=60 is the standard constant.
    // Results are already sorted by Tantivy relevance within each source.

    // Search symbols index
    if plan.search_symbols {
        let mut results = search_index(
            &index_set.symbols,
            &mut debug,
            conn,
            query,
            "symbol",
            SearchScope {
                ref_name: search_ref,
                language,
                role: options.role.as_deref(),
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.symbol_weight, RRF_K as f32);
        all_results.extend(results);
    }

    // Search snippets index
    if plan.search_snippets {
        let mut results = search_index(
            &index_set.snippets,
            &mut debug,
            conn,
            query,
            "snippet",
            SearchScope {
                ref_name: search_ref,
                language,
                role: options.role.as_deref(),
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.snippet_weight, RRF_K as f32);
        all_results.extend(results);
    }

    // Search files index
    if plan.search_files {
        let mut results = search_index(
            &index_set.files,
            &mut debug,
            conn,
            query,
            "file",
            SearchScope {
                ref_name: search_ref,
                language,
                role: options.role.as_deref(),
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.file_weight, RRF_K as f32);
        all_results.extend(results);
    }

    // Apply local lexical reranking boosts on top of RRF scores.
    let ranking_reasons = if debug_ranking {
        let reasons = rerank_with_reasons(&mut all_results, query);
        Some(reasons)
    } else {
        rerank(&mut all_results, query);
        None
    };
    // Short-circuit semantic only after lexical rerank has shaped score spread.
    semantic_state.apply_lexical_short_circuit(&all_results);

    if semantic_state.semantic_eligible() {
        let (semantic_limit, lexical_fanout, semantic_fanout) =
            semantic_fanout_limits(limit, &options.search_config);
        semantic_limit_used = semantic_limit;
        lexical_fanout_used = lexical_fanout;
        semantic_fanout_used = semantic_fanout;
        if let Some(conn) = conn {
            if let Some(project_id) =
                resolve_semantic_project_id(conn, effective_ref.as_str(), &all_results)
            {
                if let Ok(vector_count) = cruxe_state::vector_index::count_vectors_for_scope(
                    conn,
                    &project_id,
                    &effective_ref,
                ) {
                    if vector_count >= 200_000 {
                        response_warnings.push(format!(
                            "semantic_vector_scale_tier3: {} vectors indexed for ref {}; sqlite backend may degrade. consider search.semantic.embedding.vector_backend=\"lancedb\"",
                            vector_count, effective_ref
                        ));
                    } else if vector_count >= 50_000 {
                        response_warnings.push(format!(
                            "semantic_vector_scale_tier2: {} vectors indexed for ref {}; sqlite backend may degrade with larger corpora",
                            vector_count, effective_ref
                        ));
                    }
                }
                match semantic_query(
                    conn,
                    &options.search_config,
                    query,
                    effective_ref.as_str(),
                    project_id.as_str(),
                    semantic_limit,
                ) {
                    Ok(semantic_output) => {
                        semantic_state.external_provider_blocked |=
                            semantic_output.external_provider_blocked;
                        if semantic_output.results.is_empty() {
                            semantic_state.mark_skipped("semantic_no_matches");
                        } else {
                            let mut semantic_results = semantic_output.results;
                            enrich_semantic_hits_with_symbol_index(
                                &index_set.symbols,
                                &mut semantic_results,
                                effective_ref.as_str(),
                            )?;
                            let effective_semantic_cap = semantic_limit.min(semantic_fanout);
                            semantic_budget_exhausted =
                                semantic_results.len() >= effective_semantic_cap;
                            all_results = blend_hybrid_results(
                                all_results,
                                semantic_results,
                                semantic_state.semantic_ratio_used,
                                lexical_fanout,
                                semantic_fanout,
                            );
                            for result in &mut all_results {
                                if result.provenance == "semantic" {
                                    let kind_match = result
                                        .kind
                                        .as_deref()
                                        .map(|kind| {
                                            kind_weight(kind) + query_intent_boost(query, kind)
                                        })
                                        .unwrap_or(0.0);
                                    let penalty = test_file_penalty(&result.path);
                                    result.score += (kind_match + penalty) as f32;
                                }
                            }
                            semantic_state.semantic_triggered = true;
                            semantic_state.semantic_skipped_reason = None;
                        }
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            query,
                            ref_name = %effective_ref,
                            project_id = %project_id,
                            reason = "semantic_backend_error",
                            mode = %options.search_config.semantic.mode,
                            "semantic query failed, falling back to lexical-only results"
                        );
                        semantic_state.mark_skipped("semantic_backend_error");
                        semantic_state.semantic_fallback = true;
                    }
                }
            } else {
                semantic_state.mark_skipped("project_scope_unresolved");
            }
        } else {
            semantic_state.mark_skipped("semantic_requires_state_connection");
        }
    }

    // Role-filtered searches are symbol-channel queries. Semantic/hybrid blending
    // can introduce additional symbol candidates, so enforce a final role guard
    // before response shaping.
    if let Some(role) = options.role.as_deref() {
        retain_role_filtered_results(&mut all_results, role);
    }

    let total = all_results.len();

    let rerank_enabled = options.search_config.semantic_mode_typed()
        != cruxe_core::types::SemanticMode::Off
        && options.search_config.semantic.rerank.provider != "none";
    if rerank_enabled {
        let rerank_docs: Vec<RerankDocument> = all_results
            .iter()
            .map(|result| RerankDocument {
                result_id: result.result_id.clone(),
                text: format!(
                    "{}\n{}\n{}",
                    result.path,
                    result.name.clone().unwrap_or_default(),
                    result.snippet.clone().unwrap_or_default()
                ),
                base_score: result.score as f64,
            })
            .collect();
        let rerank_execution = rerank_documents(
            query,
            &rerank_docs,
            &options.search_config.semantic,
            limit.max(1),
        );
        apply_rerank_scores(&mut all_results, &rerank_execution.reranked);
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.result_id.cmp(&b.result_id))
        });
        semantic_state.external_provider_blocked |= rerank_execution.external_provider_blocked;
        semantic_state.rerank_provider = rerank_execution.provider;
        semantic_state.rerank_fallback = rerank_execution.fallback;
        semantic_state.rerank_fallback_reason = rerank_execution.fallback_reason;
    } else if options.search_config.semantic_mode_typed() == cruxe_core::types::SemanticMode::Off {
        semantic_state.rerank_provider = "none".to_string();
        semantic_state.rerank_fallback = false;
        semantic_state.rerank_fallback_reason = None;
    } else {
        semantic_state.rerank_provider = "local".to_string();
    }

    all_results.truncate(limit);
    let ranking_reasons = ranking_reasons.map(|reasons| reasons.into_iter().take(limit).collect());

    let confidence_threshold = options
        .search_config
        .confidence_threshold(options.confidence_threshold_override);
    let confidence = evaluate_confidence(
        &all_results,
        query,
        intent.intent,
        confidence_threshold,
        &options.search_config.semantic,
    );

    let metadata = SearchMetadata {
        semantic_mode: options.search_config.semantic.mode.clone(),
        semantic_enabled: options.search_config.semantic_enabled(),
        semantic_ratio_used: semantic_state.semantic_ratio_used,
        semantic_triggered: semantic_state.semantic_triggered,
        semantic_skipped_reason: semantic_state.semantic_skipped_reason,
        semantic_fallback: semantic_state.semantic_fallback,
        semantic_degraded: semantic_state.semantic_fallback,
        semantic_limit_used,
        lexical_fanout_used,
        semantic_fanout_used,
        semantic_budget_exhausted,
        external_provider_blocked: semantic_state.external_provider_blocked,
        embedding_model_version: options
            .search_config
            .semantic
            .embedding
            .model_version
            .clone(),
        rerank_provider: semantic_state.rerank_provider,
        rerank_fallback: semantic_state.rerank_fallback,
        rerank_fallback_reason: semantic_state.rerank_fallback_reason,
        low_confidence: confidence.low_confidence,
        suggested_action: confidence.suggested_action.clone(),
        confidence_threshold: confidence.threshold,
        top_score: confidence.top_score,
        score_margin: confidence.score_margin,
        channel_agreement: confidence.channel_agreement,
        query_intent_confidence: intent.confidence,
        intent_escalation_hint: intent.escalation_hint.clone(),
        warnings: response_warnings,
    };

    // Build suggested next actions
    let suggested = build_suggested_actions(&all_results, query, search_ref);

    debug!(
        query,
        intent = ?intent.intent,
        results = all_results.len(),
        total,
        "search_code"
    );

    Ok(SearchResponse {
        results: all_results,
        query_intent: intent.intent,
        total_candidates: total,
        suggested_next_actions: suggested,
        metadata,
        debug,
        ranking_reasons,
    })
}

/// Execute VCS-mode merged search for a non-default ref.
///
/// This runs base and overlay queries separately, suppresses tombstoned base paths,
/// applies overlay-wins merge semantics, and returns a unified response.
pub fn search_code_vcs_merged(
    ctx: VcsSearchContext<'_>,
    conn: Option<&Connection>,
    query: &str,
    language: Option<&str>,
    limit: usize,
    debug_ranking: bool,
) -> Result<SearchResponse, StateError> {
    search_code_vcs_merged_with_options(
        ctx,
        conn,
        query,
        language,
        limit,
        debug_ranking,
        SearchExecutionOptions::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn search_code_vcs_merged_with_options(
    ctx: VcsSearchContext<'_>,
    conn: Option<&Connection>,
    query: &str,
    language: Option<&str>,
    limit: usize,
    debug_ranking: bool,
    options: SearchExecutionOptions,
) -> Result<SearchResponse, StateError> {
    let base_options = options.clone();
    let overlay_options = options.clone();
    let run_sequential = || -> Result<(SearchResponse, SearchResponse), StateError> {
        let base = search_code_with_options(
            ctx.base_index_set,
            conn,
            query,
            Some(ctx.base_ref),
            language,
            limit,
            false,
            base_options.clone(),
        )?;
        let overlay = search_code_with_options(
            ctx.overlay_index_set,
            conn,
            query,
            Some(ctx.target_ref),
            language,
            limit,
            false,
            overlay_options.clone(),
        )?;
        Ok((base, overlay))
    };

    let (base, overlay) = if let Some(conn) = conn {
        let base_conn = clone_connection_for_parallel(conn);
        let overlay_conn = clone_connection_for_parallel(conn);
        if let (Some(base_conn), Some(overlay_conn)) = (base_conn, overlay_conn) {
            std::thread::scope(|scope| {
                let base_task = scope.spawn(move || {
                    search_code_with_options(
                        ctx.base_index_set,
                        Some(&base_conn),
                        query,
                        Some(ctx.base_ref),
                        language,
                        limit,
                        false,
                        base_options.clone(),
                    )
                });
                let overlay_task = scope.spawn(move || {
                    search_code_with_options(
                        ctx.overlay_index_set,
                        Some(&overlay_conn),
                        query,
                        Some(ctx.target_ref),
                        language,
                        limit,
                        false,
                        overlay_options.clone(),
                    )
                });

                let base = base_task
                    .join()
                    .map_err(|_| StateError::Sqlite("base search worker panicked".to_string()))??;
                let overlay = overlay_task.join().map_err(|_| {
                    StateError::Sqlite("overlay search worker panicked".to_string())
                })??;
                Ok::<_, StateError>((base, overlay))
            })?
        } else {
            run_sequential()?
        }
    } else {
        std::thread::scope(|scope| {
            let base_task = scope.spawn(|| {
                search_code_with_options(
                    ctx.base_index_set,
                    None,
                    query,
                    Some(ctx.base_ref),
                    language,
                    limit,
                    false,
                    options.clone(),
                )
            });
            let overlay_task = scope.spawn(|| {
                search_code_with_options(
                    ctx.overlay_index_set,
                    None,
                    query,
                    Some(ctx.target_ref),
                    language,
                    limit,
                    false,
                    options.clone(),
                )
            });

            let base = base_task
                .join()
                .map_err(|_| StateError::Sqlite("base search worker panicked".to_string()))??;
            let overlay = overlay_task
                .join()
                .map_err(|_| StateError::Sqlite("overlay search worker panicked".to_string()))??;
            Ok::<_, StateError>((base, overlay))
        })?
    };

    let mut results = overlay_merge::merged_search(base.results, overlay.results, ctx.tombstones);
    let ranking_reasons = if debug_ranking {
        Some(rerank_with_reasons(&mut results, query))
    } else {
        rerank(&mut results, query);
        None
    };
    results.truncate(limit);

    // Recalculate confidence on the merged result set (not the stale overlay snapshot).
    let mut metadata = overlay.metadata;
    let confidence_threshold = options
        .search_config
        .confidence_threshold(options.confidence_threshold_override);
    let confidence = evaluate_confidence(
        &results,
        query,
        base.query_intent,
        confidence_threshold,
        &options.search_config.semantic,
    );
    metadata.low_confidence = confidence.low_confidence;
    metadata.suggested_action = confidence.suggested_action;
    metadata.confidence_threshold = confidence.threshold;
    metadata.top_score = confidence.top_score;
    metadata.score_margin = confidence.score_margin;
    metadata.channel_agreement = confidence.channel_agreement;

    Ok(SearchResponse {
        results,
        query_intent: base.query_intent,
        total_candidates: base.total_candidates + overlay.total_candidates,
        suggested_next_actions: if !overlay.suggested_next_actions.is_empty() {
            overlay.suggested_next_actions
        } else {
            base.suggested_next_actions
        },
        metadata,
        debug: None,
        ranking_reasons,
    })
}

fn clone_connection_for_parallel(conn: &Connection) -> Option<Connection> {
    let path = conn.path()?;
    if path == ":memory:" {
        return None;
    }
    cruxe_state::db::open_connection(Path::new(path)).ok()
}

fn semantic_fanout_limits(limit: usize, search_config: &CoreSearchConfig) -> (usize, usize, usize) {
    let semantic_limit = limit
        .saturating_mul(search_config.semantic.semantic_limit_multiplier)
        .clamp(20, 1000);
    let lexical_fanout = limit
        .saturating_mul(search_config.semantic.lexical_fanout_multiplier)
        .clamp(40, 2000);
    let semantic_fanout = limit
        .saturating_mul(search_config.semantic.semantic_fanout_multiplier)
        .clamp(30, 1000);
    (semantic_limit, lexical_fanout, semantic_fanout)
}

/// Infer the project_id for semantic search from lexical results or the database.
///
/// Returns `None` (skipping semantic) when the ref spans multiple repos, since
/// semantic vectors are partitioned by project_id and we cannot know which to query.
fn resolve_semantic_project_id(
    conn: &Connection,
    ref_name: &str,
    lexical_results: &[SearchResult],
) -> Option<String> {
    // First try: infer from lexical result repos (most common path).
    let mut lexical_projects = lexical_results
        .iter()
        .filter_map(|result| {
            (!result.repo.is_empty())
                .then_some(result.repo.as_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    lexical_projects.sort();
    lexical_projects.dedup();
    if lexical_projects.len() == 1 {
        return lexical_projects.into_iter().next();
    }
    if lexical_projects.len() > 1 {
        tracing::debug!(
            count = lexical_projects.len(),
            "skipping semantic: multiple repos in lexical results"
        );
        return None;
    }

    // Fallback: query symbol_relations for repos indexed under this ref.
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT repo
             FROM symbol_relations
             WHERE \"ref\" = ?1
             LIMIT 2",
        )
        .ok()?;
    let rows = stmt
        .query_map([ref_name], |row| row.get::<_, String>(0))
        .ok()?;
    let mut repos = Vec::new();
    for repo in rows.flatten() {
        repos.push(repo);
        if repos.len() > 1 {
            tracing::debug!("skipping semantic: multiple repos found in symbol_relations for ref");
            return None;
        }
    }
    if repos.is_empty() {
        tracing::debug!("skipping semantic: no project_id could be inferred for ref");
    }
    repos.into_iter().next()
}

/// Build suggested next actions based on top results.
fn build_suggested_actions(
    results: &[SearchResult],
    query: &str,
    r#ref: Option<&str>,
) -> Vec<SuggestedAction> {
    let mut actions = Vec::new();

    if let Some(top) = results.first()
        && let Some(ref name) = top.name
    {
        actions.push(SuggestedAction {
            tool: "locate_symbol".to_string(),
            name: Some(name.clone()),
            query: None,
            r#ref: r#ref.map(String::from),
            limit: None,
        });
    }

    if results.len() > 3 {
        actions.push(SuggestedAction {
            tool: "search_code".to_string(),
            name: None,
            query: Some(query.to_string()),
            r#ref: r#ref.map(String::from),
            limit: Some(5),
        });
    }

    actions
}

/// Apply Reciprocal Rank Fusion (RRF) scores to results from a single index.
/// Each result gets `score = weight / (k + rank)` where rank is 1-based position.
/// Results must already be sorted by Tantivy relevance (descending score).
fn apply_rrf_scores(results: &mut [SearchResult], weight: f32, k: f32) {
    for (rank_0, result) in results.iter_mut().enumerate() {
        let rank = (rank_0 + 1) as f32;
        result.score = weight / (k + rank);
    }
}

fn apply_rerank_scores(results: &mut [SearchResult], reranked: &[cruxe_core::types::RerankResult]) {
    let score_by_doc: HashMap<&str, f64> = reranked
        .iter()
        .map(|result| (result.doc.as_str(), result.score))
        .collect();

    for result in results {
        if let Some(score) = score_by_doc.get(result.result_id.as_str()) {
            result.score = *score as f32;
        }
    }
}

fn default_result_provenance() -> String {
    "lexical".to_string()
}

fn retain_role_filtered_results(results: &mut Vec<SearchResult>, role: &str) {
    let Ok(expected_role) = role.parse::<SymbolRole>() else {
        results.clear();
        return;
    };

    results.retain(|result| {
        result.result_type == "symbol"
            && result
                .kind
                .as_deref()
                .and_then(SymbolKind::parse_kind)
                .is_some_and(|kind| kind.role() == expected_role)
    });
}

#[derive(Debug, Clone)]
struct SemanticExecutionState {
    semantic_ratio_used: f64,
    semantic_triggered: bool,
    semantic_skipped_reason: Option<String>,
    semantic_fallback: bool,
    external_provider_blocked: bool,
    rerank_provider: String,
    rerank_fallback: bool,
    rerank_fallback_reason: Option<String>,
    lexical_short_circuit_threshold: f64,
}

fn semantic_execution_state(
    intent: &crate::intent::IntentClassification,
    options: &SearchExecutionOptions,
) -> SemanticExecutionState {
    let semantic_mode = options.search_config.semantic_mode_typed();
    let semantic_enabled = options.search_config.semantic_enabled();
    let semantic_ratio_cap = options
        .search_config
        .semantic_ratio_for_intent(intent.intent, options.semantic_ratio_override);
    let semantic_skipped_reason = if !semantic_enabled {
        Some("semantic_disabled".to_string())
    } else if intent.intent != QueryIntent::NaturalLanguage {
        Some("intent_not_nl".to_string())
    } else if semantic_ratio_cap <= 0.0 {
        Some("semantic_ratio_zero".to_string())
    } else {
        match semantic_mode {
            cruxe_core::types::SemanticMode::RerankOnly => Some("mode_rerank_only".to_string()),
            cruxe_core::types::SemanticMode::Hybrid => None,
            cruxe_core::types::SemanticMode::Off => Some("semantic_disabled".to_string()),
        }
    };

    let rerank_provider = if semantic_mode == cruxe_core::types::SemanticMode::Off {
        "none".to_string()
    } else if options.search_config.semantic.rerank.provider == "none" {
        "local".to_string()
    } else {
        options.search_config.semantic.rerank.provider.clone()
    };

    SemanticExecutionState {
        semantic_ratio_used: if semantic_skipped_reason.is_none() {
            semantic_ratio_cap
        } else {
            0.0
        },
        semantic_triggered: false,
        semantic_skipped_reason,
        semantic_fallback: false,
        external_provider_blocked: false,
        rerank_provider,
        rerank_fallback: false,
        rerank_fallback_reason: None,
        lexical_short_circuit_threshold: options
            .search_config
            .semantic
            .lexical_short_circuit_threshold,
    }
}

impl SemanticExecutionState {
    fn semantic_eligible(&self) -> bool {
        self.semantic_skipped_reason.is_none() && self.semantic_ratio_used > 0.0
    }

    fn mark_skipped(&mut self, reason: &str) {
        self.semantic_triggered = false;
        self.semantic_ratio_used = 0.0;
        self.semantic_skipped_reason = Some(reason.to_string());
    }

    fn apply_lexical_short_circuit(&mut self, lexical_results: &[SearchResult]) {
        if self.semantic_skipped_reason.is_some() {
            return;
        }
        let top_lexical = lexical_results
            .iter()
            .map(|result| normalize_relevance_score(result.score as f64))
            .fold(0.0_f64, f64::max);
        if top_lexical >= self.lexical_short_circuit_threshold {
            self.mark_skipped("lexical_high_confidence");
        }
    }
}

#[derive(Clone, Copy)]
struct SearchScope<'a> {
    ref_name: Option<&'a str>,
    language: Option<&'a str>,
    role: Option<&'a str>,
}

fn search_index(
    index: &tantivy::Index,
    debug: &mut Option<SearchDebugInfo>,
    conn: Option<&Connection>,
    query: &str,
    result_type: &str,
    scope: SearchScope<'_>,
    limit: usize,
) -> Result<Vec<SearchResult>, StateError> {
    if scope.role.is_some() && result_type != "symbol" {
        return Ok(Vec::new());
    }

    let reader = index.reader().map_err(StateError::tantivy)?;
    let searcher = reader.searcher();
    let schema = index.schema();

    let search_fields: Vec<tantivy::schema::Field> = match result_type {
        "symbol" => [
            "symbol_exact",
            "qualified_name",
            "signature",
            "content",
            "path",
        ]
        .iter()
        .filter_map(|name| schema.get_field(name).ok())
        .collect(),
        "snippet" => ["content", "path", "imports"]
            .iter()
            .filter_map(|name| schema.get_field(name).ok())
            .collect(),
        "file" => ["path", "filename", "content_head"]
            .iter()
            .filter_map(|name| schema.get_field(name).ok())
            .collect(),
        _ => return Ok(Vec::new()),
    };

    if search_fields.is_empty() {
        return Ok(Vec::new());
    }

    let mut query_parser = QueryParser::for_index(index, search_fields);
    if result_type == "symbol" {
        if let Ok(field) = schema.get_field("symbol_exact") {
            query_parser.set_field_boost(field, 10.0);
        }
        if let Ok(field) = schema.get_field("qualified_name") {
            query_parser.set_field_boost(field, 3.0);
        }
        if let Ok(field) = schema.get_field("signature") {
            query_parser.set_field_boost(field, 1.5);
        }
        if let Ok(field) = schema.get_field("path") {
            query_parser.set_field_boost(field, 1.0);
        }
        if let Ok(field) = schema.get_field("content") {
            query_parser.set_field_boost(field, 0.5);
        }
    }
    let parsed_query = query_parser
        .parse_query(query)
        .map_err(StateError::tantivy)?;

    // Build final query with optional ref and language filters
    let final_query: Box<dyn tantivy::query::Query> =
        if scope.ref_name.is_some() || scope.language.is_some() || scope.role.is_some() {
            let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
            clauses.push((Occur::Must, parsed_query));

            if let Some(r) = scope.ref_name
                && let Ok(ref_field) = schema.get_field("ref")
            {
                clauses.push((
                    Occur::Must,
                    Box::new(TermQuery::new(
                        Term::from_field_text(ref_field, r),
                        IndexRecordOption::Basic,
                    )),
                ));
            }
            if let Some(lang) = scope.language
                && let Ok(lang_field) = schema.get_field("language")
            {
                clauses.push((
                    Occur::Must,
                    Box::new(TermQuery::new(
                        Term::from_field_text(lang_field, lang),
                        IndexRecordOption::Basic,
                    )),
                ));
            }
            if let Some(role) = scope.role
                && let Ok(role_field) = schema.get_field("role")
            {
                clauses.push((
                    Occur::Must,
                    Box::new(TermQuery::new(
                        Term::from_field_text(role_field, role),
                        IndexRecordOption::Basic,
                    )),
                ));
            }
            Box::new(BooleanQuery::new(clauses))
        } else {
            parsed_query
        };

    let top_docs = searcher
        .search(&final_query, &TopDocs::with_limit(limit))
        .map_err(StateError::tantivy)?;

    let mut results = Vec::new();
    for (score, doc_address) in top_docs {
        let doc = searcher
            .doc::<tantivy::TantivyDocument>(doc_address)
            .map_err(StateError::tantivy)?;

        let get_text = |field_name: &str| -> Option<String> {
            schema
                .get_field(field_name)
                .ok()
                .and_then(|f| doc.get_first(f))
                .and_then(|v| v.as_str())
                .map(|s: &str| s.to_string())
                .filter(|s| !s.is_empty())
        };
        let get_u64 = |field_name: &str| -> u64 {
            schema
                .get_field(field_name)
                .ok()
                .and_then(|f| doc.get_first(f))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        };

        let path = get_text("path").unwrap_or_default();
        let line_start = get_u64("line_start") as u32;
        let line_end = get_u64("line_end") as u32;
        let mut kind = get_text("kind");
        let mut symbol_name = get_text("symbol_exact").or_else(|| get_text("filename"));
        let mut qualified_name = get_text("qualified_name");
        let mut symbol_id = get_text("symbol_id");
        let mut symbol_stable_id = get_text("symbol_stable_id");
        let language = get_text("language").unwrap_or_default();
        let doc_repo = get_text("repo").unwrap_or_default();
        let doc_ref = get_text("ref").unwrap_or_default();

        if result_type == "snippet" {
            let join_hit = enrich_snippet_with_symbol_metadata(
                conn,
                &doc_repo,
                &doc_ref,
                &path,
                line_start,
                line_end,
                SnippetSymbolMetadata {
                    symbol_id: &mut symbol_id,
                    symbol_stable_id: &mut symbol_stable_id,
                    kind: &mut kind,
                    name: &mut symbol_name,
                    qualified_name: &mut qualified_name,
                },
            );
            if let Some(debug) = debug.as_mut() {
                if join_hit {
                    debug.join_status.hits += 1;
                } else {
                    debug.join_status.misses += 1;
                }
            }
        }

        let result_id = compute_stable_result_id(StableResultIdInput {
            result_type,
            repo: &doc_repo,
            ref_name: &doc_ref,
            path: &path,
            line_start,
            line_end,
            kind: kind.as_deref().unwrap_or(""),
            name: symbol_name.as_deref().unwrap_or(""),
            qualified_name: qualified_name.as_deref().unwrap_or(""),
            language: &language,
            symbol_stable_id: symbol_stable_id.as_deref().unwrap_or(""),
        });

        results.push(SearchResult {
            repo: doc_repo,
            result_id,
            symbol_id,
            symbol_stable_id,
            result_type: result_type.to_string(),
            path,
            line_start,
            line_end,
            kind,
            name: symbol_name,
            qualified_name,
            language,
            signature: get_text("signature"),
            visibility: get_text("visibility"),
            score,
            snippet: get_text("content").map(|c| {
                if c.len() > 200 {
                    // Truncate at a char boundary to avoid panic on multi-byte UTF-8.
                    let end = c
                        .char_indices()
                        .map(|(i, _)| i)
                        .take_while(|&i| i <= 200)
                        .last()
                        .unwrap_or(0);
                    format!("{}...", &c[..end])
                } else {
                    c
                }
            }),
            chunk_type: get_text("chunk_type"),
            source_layer: None,
            provenance: default_result_provenance(),
        });
    }

    Ok(results)
}

struct SnippetSymbolMetadata<'a> {
    symbol_id: &'a mut Option<String>,
    symbol_stable_id: &'a mut Option<String>,
    kind: &'a mut Option<String>,
    name: &'a mut Option<String>,
    qualified_name: &'a mut Option<String>,
}

fn enrich_snippet_with_symbol_metadata(
    conn: Option<&Connection>,
    repo: &str,
    r#ref: &str,
    path: &str,
    line_start: u32,
    line_end: u32,
    metadata: SnippetSymbolMetadata<'_>,
) -> bool {
    let SnippetSymbolMetadata {
        symbol_id,
        symbol_stable_id,
        kind,
        name,
        qualified_name,
    } = metadata;

    let Some(conn) = conn else {
        return false;
    };
    if repo.is_empty() || r#ref.is_empty() || path.is_empty() {
        return false;
    }

    let Ok(symbols) = cruxe_state::symbols::find_symbols_by_location(
        conn, repo, r#ref, path, line_start, line_end,
    ) else {
        return false;
    };
    let Some(best_symbol) = choose_best_symbol_match(symbols, line_start, line_end) else {
        return false;
    };

    *symbol_id = Some(best_symbol.symbol_id);
    *symbol_stable_id = Some(best_symbol.symbol_stable_id);
    *kind = Some(best_symbol.kind.as_str().to_string());
    *name = Some(best_symbol.name);
    *qualified_name = Some(best_symbol.qualified_name);
    true
}

fn choose_best_symbol_match(
    symbols: Vec<SymbolRecord>,
    line_start: u32,
    line_end: u32,
) -> Option<SymbolRecord> {
    symbols.into_iter().min_by_key(|sym| {
        let exact_range_mismatch =
            u8::from(sym.line_start != line_start || sym.line_end != line_end);
        let boundary_distance =
            sym.line_start.abs_diff(line_start) + sym.line_end.abs_diff(line_end);
        let span = sym.line_end.saturating_sub(sym.line_start);
        (exact_range_mismatch, boundary_distance, span)
    })
}

struct StableResultIdInput<'a> {
    result_type: &'a str,
    repo: &'a str,
    ref_name: &'a str,
    path: &'a str,
    line_start: u32,
    line_end: u32,
    kind: &'a str,
    name: &'a str,
    qualified_name: &'a str,
    language: &'a str,
    symbol_stable_id: &'a str,
}

fn compute_stable_result_id(input: StableResultIdInput<'_>) -> String {
    let StableResultIdInput {
        result_type,
        repo,
        ref_name,
        path,
        line_start,
        line_end,
        kind,
        name,
        qualified_name,
        language,
        symbol_stable_id,
    } = input;
    let payload = format!(
        "result:v2|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        result_type,
        repo,
        ref_name,
        path,
        line_start,
        line_end,
        kind,
        name,
        qualified_name,
        language,
        symbol_stable_id
    );
    format!("res_{}", blake3::hash(payload.as_bytes()).to_hex())
}

/// Enrich semantic-only hits with symbol metadata using `symbol_stable_id` lookup.
///
/// This is the parity bridge introduced by `refactor-multilang-symbol-contract` task 6.8:
/// semantic candidates should carry the same `kind`/`name`/`qualified_name` fields
/// as lexical candidates before downstream ranking and protocol serialization.
/// `semantic-retrieval-quality-contract` then layers degraded-state/budget metadata
/// on top of this enrichment path (it does not introduce a second enrichment pipeline).
fn enrich_semantic_hits_with_symbol_index(
    symbol_index: &tantivy::Index,
    results: &mut [SearchResult],
    ref_name: &str,
) -> Result<(), StateError> {
    if results.is_empty() {
        return Ok(());
    }

    let reader = symbol_index.reader().map_err(StateError::tantivy)?;
    let searcher = reader.searcher();
    let schema = symbol_index.schema();

    let Some(symbol_stable_id_field) = schema.get_field("symbol_stable_id").ok() else {
        return Ok(());
    };
    let ref_field = schema.get_field("ref").ok();
    let kind_field = schema.get_field("kind").ok();
    let name_field = schema.get_field("symbol_exact").ok();
    let qualified_name_field = schema.get_field("qualified_name").ok();

    for result in results.iter_mut() {
        if result.kind.is_some() && result.name.is_some() && result.qualified_name.is_some() {
            continue;
        }
        let Some(symbol_stable_id) = result
            .symbol_stable_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };

        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = vec![(
            Occur::Must,
            Box::new(TermQuery::new(
                Term::from_field_text(symbol_stable_id_field, symbol_stable_id),
                IndexRecordOption::Basic,
            )),
        )];
        if let Some(field) = ref_field {
            clauses.push((
                Occur::Must,
                Box::new(TermQuery::new(
                    Term::from_field_text(field, ref_name),
                    IndexRecordOption::Basic,
                )),
            ));
        }

        let query = BooleanQuery::new(clauses);
        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(1))
            .map_err(StateError::tantivy)?;
        let Some((_, doc_address)) = top_docs.into_iter().next() else {
            continue;
        };
        let doc = searcher
            .doc::<tantivy::TantivyDocument>(doc_address)
            .map_err(StateError::tantivy)?;

        if result.kind.is_none() {
            result.kind = kind_field
                .and_then(|field| doc.get_first(field))
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
        }
        if result.name.is_none() {
            result.name = name_field
                .and_then(|field| doc.get_first(field))
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
        }
        if result.qualified_name.is_none() {
            result.qualified_name = qualified_name_field
                .and_then(|field| doc.get_first(field))
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxe_core::config::SearchConfig as CoreSearchConfig;
    use cruxe_core::types::{SymbolKind, SymbolRecord};
    use cruxe_state::{db, schema, vector_index, vector_index::VectorRecord};
    use std::collections::HashSet;
    use tempfile::tempdir;

    fn make_result(score: f32) -> SearchResult {
        SearchResult {
            repo: "repo".to_string(),
            result_id: "res-1".to_string(),
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
            provenance: "lexical".to_string(),
        }
    }

    #[test]
    fn lexical_short_circuit_uses_reranked_score_band() {
        let mut state = SemanticExecutionState {
            semantic_ratio_used: 0.5,
            semantic_triggered: false,
            semantic_skipped_reason: None,
            semantic_fallback: false,
            external_provider_blocked: false,
            rerank_provider: "none".to_string(),
            rerank_fallback: false,
            rerank_fallback_reason: None,
            lexical_short_circuit_threshold: 0.85,
        };

        state.apply_lexical_short_circuit(&[make_result(0.05)]);
        assert!(state.semantic_skipped_reason.is_none());

        state.apply_lexical_short_circuit(&[make_result(7.0)]);
        assert_eq!(
            state.semantic_skipped_reason.as_deref(),
            Some("lexical_high_confidence")
        );
    }

    #[test]
    fn hybrid_mode_returns_semantic_match_for_conceptual_query() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "hybrid".to_string();
        search_config.semantic.embedding.provider = "local".to_string();
        search_config.semantic.embedding.profile = "fast_local".to_string();
        search_config.semantic.embedding.model = "NomicEmbedTextV15Q".to_string();
        search_config.semantic.embedding.model_version = "fastembed-1".to_string();
        search_config.semantic.embedding.dimensions = 768;
        search_config.semantic.ratio = 0.6;
        search_config.semantic.rerank.provider = "none".to_string();

        let mut provider =
            cruxe_state::embedding::build_embedding_provider(&search_config.semantic)
                .unwrap()
                .provider;
        let query_text = "where is user login flow handled";
        let query_vector = provider.embed_batch(&[query_text.to_string()]).unwrap();
        let vector = query_vector.into_iter().next().unwrap();

        vector_index::upsert_vectors(
            &conn,
            &[VectorRecord {
                project_id: "proj-semantic".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: "stable-auth-1".to_string(),
                snippet_hash: "hash-auth-1".to_string(),
                embedding_model_id: provider.model_id().to_string(),
                embedding_model_version: provider.model_version().to_string(),
                embedding_dimensions: provider.dimensions(),
                path: "src/auth.rs".to_string(),
                line_start: 10,
                line_end: 24,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: "fn authenticate_user(request: LoginRequest) -> Result<User>"
                    .to_string(),
                vector,
            }],
        )
        .unwrap();
        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-semantic".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: "src/auth.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-auth-1".to_string(),
                symbol_stable_id: "stable-auth-1".to_string(),
                name: "authenticate_user".to_string(),
                qualified_name: "authenticate_user".to_string(),
                kind: SymbolKind::Function,
                signature: Some(
                    "fn authenticate_user(request: LoginRequest) -> Result<User>".to_string(),
                ),
                line_start: 10,
                line_end: 24,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some(
                    "fn authenticate_user(request: LoginRequest) -> Result<User> { todo!() }"
                        .to_string(),
                ),
            },
        )
        .unwrap();

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            query_text,
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
            },
        )
        .unwrap();

        assert!(!response.results.is_empty());
        assert_eq!(response.results[0].path, "src/auth.rs");
        assert!(matches!(
            response.results[0].provenance.as_str(),
            "semantic" | "hybrid"
        ));
        assert!(response.metadata.semantic_triggered);
        assert!(response.metadata.semantic_ratio_used > 0.0);
        assert!(response.metadata.semantic_skipped_reason.is_none());
    }

    #[test]
    fn hybrid_mode_role_filter_is_enforced_after_semantic_blend() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "hybrid".to_string();
        search_config.semantic.embedding.provider = "local".to_string();
        search_config.semantic.embedding.profile = "fast_local".to_string();
        search_config.semantic.embedding.model = "NomicEmbedTextV15Q".to_string();
        search_config.semantic.embedding.model_version = "fastembed-1".to_string();
        search_config.semantic.embedding.dimensions = 768;
        search_config.semantic.ratio = 0.6;
        search_config.semantic.rerank.provider = "none".to_string();

        let mut provider =
            cruxe_state::embedding::build_embedding_provider(&search_config.semantic)
                .unwrap()
                .provider;
        let query_text = "where is user login flow handled";
        let query_vector = provider.embed_batch(&[query_text.to_string()]).unwrap();
        let vector = query_vector.into_iter().next().unwrap();

        vector_index::upsert_vectors(
            &conn,
            &[VectorRecord {
                project_id: "proj-semantic".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: "stable-auth-1".to_string(),
                snippet_hash: "hash-auth-1".to_string(),
                embedding_model_id: provider.model_id().to_string(),
                embedding_model_version: provider.model_version().to_string(),
                embedding_dimensions: provider.dimensions(),
                path: "src/auth.rs".to_string(),
                line_start: 10,
                line_end: 24,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: "fn authenticate_user(request: LoginRequest) -> Result<User>"
                    .to_string(),
                vector,
            }],
        )
        .unwrap();
        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-semantic".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: "src/auth.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-auth-1".to_string(),
                symbol_stable_id: "stable-auth-1".to_string(),
                name: "authenticate_user".to_string(),
                qualified_name: "authenticate_user".to_string(),
                kind: SymbolKind::Function,
                signature: Some(
                    "fn authenticate_user(request: LoginRequest) -> Result<User>".to_string(),
                ),
                line_start: 10,
                line_end: 24,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some(
                    "fn authenticate_user(request: LoginRequest) -> Result<User> { todo!() }"
                        .to_string(),
                ),
            },
        )
        .unwrap();

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            query_text,
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: Some("type".to_string()),
            },
        )
        .unwrap();

        assert!(
            response.results.is_empty(),
            "semantic function hits must not leak into role=type queries"
        );
    }

    #[test]
    fn hybrid_mode_reports_external_provider_blocked_for_embedding_path() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "hybrid".to_string();
        search_config.semantic.embedding.provider = "openai".to_string();
        search_config.semantic.embedding.profile = "fast_local".to_string();
        search_config.semantic.embedding.model = "NomicEmbedTextV15Q".to_string();
        search_config.semantic.embedding.model_version = "fastembed-1".to_string();
        search_config.semantic.embedding.dimensions = 768;
        search_config.semantic.ratio = 0.6;
        search_config.semantic.rerank.provider = "none".to_string();
        search_config.semantic.external_provider_enabled = false;
        search_config.semantic.allow_code_payload_to_external = false;

        let built_provider =
            cruxe_state::embedding::build_embedding_provider(&search_config.semantic).unwrap();
        assert!(built_provider.external_provider_blocked);
        let mut provider = built_provider.provider;
        let query_text = "where is token refresh handled";
        let query_vector = provider.embed_batch(&[query_text.to_string()]).unwrap();
        let vector = query_vector.into_iter().next().unwrap();

        vector_index::upsert_vectors(
            &conn,
            &[VectorRecord {
                project_id: "proj-embed-blocked".to_string(),
                ref_name: "main".to_string(),
                symbol_stable_id: "stable-refresh-1".to_string(),
                snippet_hash: "hash-refresh-1".to_string(),
                embedding_model_id: provider.model_id().to_string(),
                embedding_model_version: provider.model_version().to_string(),
                embedding_dimensions: provider.dimensions(),
                path: "src/token.rs".to_string(),
                line_start: 6,
                line_end: 18,
                language: "rust".to_string(),
                chunk_type: Some("function_body".to_string()),
                snippet_text: "fn refresh_access_token(claims: Claims) -> Result<Token>"
                    .to_string(),
                vector,
            }],
        )
        .unwrap();
        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-embed-blocked".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: "src/token.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-refresh-1".to_string(),
                symbol_stable_id: "stable-refresh-1".to_string(),
                name: "refresh_access_token".to_string(),
                qualified_name: "refresh_access_token".to_string(),
                kind: SymbolKind::Function,
                signature: Some(
                    "fn refresh_access_token(claims: Claims) -> Result<Token>".to_string(),
                ),
                line_start: 6,
                line_end: 18,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some(
                    "fn refresh_access_token(claims: Claims) -> Result<Token> { todo!() }"
                        .to_string(),
                ),
            },
        )
        .unwrap();

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            query_text,
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
            },
        )
        .unwrap();

        assert!(response.metadata.semantic_triggered);
        assert!(response.metadata.external_provider_blocked);
        assert!(!response.results.is_empty());
    }

    #[test]
    fn resolve_semantic_project_id_uses_lexical_repo_when_available() {
        let lexical = vec![SearchResult {
            repo: "proj-lexical".to_string(),
            result_id: "res-1".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "symbol".to_string(),
            path: "src/lib.rs".to_string(),
            line_start: 1,
            line_end: 1,
            kind: None,
            name: None,
            qualified_name: None,
            language: "rust".to_string(),
            signature: None,
            visibility: None,
            score: 1.0,
            snippet: None,
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        }];

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let project = resolve_semantic_project_id(&conn, "main", &lexical);
        assert_eq!(project.as_deref(), Some("proj-lexical"));
    }

    #[test]
    fn resolve_semantic_project_id_falls_back_to_symbol_relations_when_lexical_empty() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-fallback".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: "src/lib.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-1".to_string(),
                symbol_stable_id: "stable-1".to_string(),
                name: "handler".to_string(),
                qualified_name: "handler".to_string(),
                kind: SymbolKind::Function,
                signature: Some("fn handler()".to_string()),
                line_start: 1,
                line_end: 3,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some("fn handler() {}".to_string()),
            },
        )
        .unwrap();

        let project = resolve_semantic_project_id(&conn, "main", &[]);
        assert_eq!(project.as_deref(), Some("proj-fallback"));
    }

    #[test]
    fn hybrid_mode_skips_semantic_when_project_scope_is_ambiguous() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "hybrid".to_string();
        search_config.semantic.embedding.provider = "local".to_string();
        search_config.semantic.embedding.profile = "fast_local".to_string();
        search_config.semantic.embedding.model = "NomicEmbedTextV15Q".to_string();
        search_config.semantic.embedding.model_version = "fastembed-1".to_string();
        search_config.semantic.embedding.dimensions = 768;
        search_config.semantic.ratio = 0.6;
        search_config.semantic.rerank.provider = "none".to_string();

        for repo in ["proj-a", "proj-b"] {
            cruxe_state::symbols::insert_symbol(
                &conn,
                &SymbolRecord {
                    repo: repo.to_string(),
                    r#ref: "main".to_string(),
                    commit: None,
                    path: format!("src/{repo}/lib.rs"),
                    language: "rust".to_string(),
                    symbol_id: format!("sym-{repo}"),
                    symbol_stable_id: format!("stable-{repo}"),
                    name: "handler".to_string(),
                    qualified_name: "handler".to_string(),
                    kind: SymbolKind::Function,
                    signature: Some("fn handler()".to_string()),
                    line_start: 1,
                    line_end: 3,
                    parent_symbol_id: None,
                    visibility: Some("pub".to_string()),
                    content: Some("fn handler() {}".to_string()),
                },
            )
            .unwrap();
        }

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            "where is auth handled",
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
            },
        )
        .unwrap();

        assert!(!response.metadata.semantic_triggered);
        assert_eq!(
            response.metadata.semantic_skipped_reason.as_deref(),
            Some("project_scope_unresolved")
        );
    }

    #[test]
    fn semantic_mode_off_disables_rerank_even_when_provider_is_configured() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "off".to_string();
        search_config.semantic.rerank.provider = "cohere".to_string();

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            "where is authentication handled",
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
            },
        )
        .unwrap();

        assert_eq!(response.metadata.rerank_provider, "none");
        assert!(!response.metadata.rerank_fallback);
    }

    #[test]
    fn semantic_backend_error_sets_semantic_fallback_metadata() {
        let dir = tempdir().unwrap();
        let index_set = IndexSet::open(dir.path()).unwrap();

        let db_path = dir.path().join("state.db");
        let conn = db::open_connection(&db_path).unwrap();
        schema::create_tables(&conn).unwrap();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.mode = "hybrid".to_string();
        search_config.semantic.embedding.provider = "local".to_string();
        search_config.semantic.embedding.model = "BGESmallENV15Q".to_string();
        search_config.semantic.embedding.dimensions = 768; // Intentional mismatch (model expects 384)
        search_config.semantic.ratio = 0.6;
        search_config.semantic.rerank.provider = "none".to_string();

        cruxe_state::symbols::insert_symbol(
            &conn,
            &SymbolRecord {
                repo: "proj-semantic-error".to_string(),
                r#ref: "main".to_string(),
                commit: None,
                path: "src/auth.rs".to_string(),
                language: "rust".to_string(),
                symbol_id: "sym-auth".to_string(),
                symbol_stable_id: "stable-auth".to_string(),
                name: "authenticate_user".to_string(),
                qualified_name: "authenticate_user".to_string(),
                kind: SymbolKind::Function,
                signature: Some("fn authenticate_user()".to_string()),
                line_start: 1,
                line_end: 10,
                parent_symbol_id: None,
                visibility: Some("pub".to_string()),
                content: Some("fn authenticate_user() {}".to_string()),
            },
        )
        .unwrap();

        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            "where is user auth handled",
            Some("main"),
            Some("rust"),
            10,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
            },
        )
        .unwrap();

        assert!(!response.metadata.semantic_triggered);
        assert!(response.metadata.semantic_fallback);
        assert!(response.metadata.semantic_degraded);
        assert_eq!(
            response.metadata.semantic_skipped_reason.as_deref(),
            Some("semantic_backend_error")
        );
    }

    #[test]
    fn vcs_merged_confidence_threshold_uses_request_override() {
        let base_dir = tempdir().unwrap();
        let overlay_dir = tempdir().unwrap();
        let base_index_set = IndexSet::open(base_dir.path()).unwrap();
        let overlay_index_set = IndexSet::open(overlay_dir.path()).unwrap();
        let tombstones = HashSet::new();

        let mut search_config = CoreSearchConfig::default();
        search_config.semantic.confidence_threshold = 0.15;
        let response = search_code_vcs_merged_with_options(
            VcsSearchContext {
                base_index_set: &base_index_set,
                overlay_index_set: &overlay_index_set,
                base_ref: "main",
                target_ref: "feat/test",
                tombstones: &tombstones,
            },
            None,
            "where is auth handled",
            Some("rust"),
            5,
            false,
            SearchExecutionOptions {
                search_config,
                semantic_ratio_override: None,
                confidence_threshold_override: Some(0.91),
                role: None,
            },
        )
        .unwrap();

        assert!((response.metadata.confidence_threshold - 0.91).abs() < f64::EPSILON);
    }

    #[test]
    fn semantic_fanout_limits_defaults_match_legacy_behavior() {
        let config = CoreSearchConfig::default();
        assert_eq!(semantic_fanout_limits(10, &config), (20, 40, 30));
        assert_eq!(semantic_fanout_limits(15, &config), (30, 60, 45));
    }

    #[test]
    fn semantic_fanout_limits_use_configured_multipliers() {
        let mut config = CoreSearchConfig::default();
        config.semantic.semantic_limit_multiplier = 5;
        config.semantic.lexical_fanout_multiplier = 6;
        config.semantic.semantic_fanout_multiplier = 7;
        assert_eq!(semantic_fanout_limits(3, &config), (20, 40, 30));
        assert_eq!(semantic_fanout_limits(10, &config), (50, 60, 70));
    }

    #[test]
    fn semantic_fanout_limits_apply_caps() {
        let mut config = CoreSearchConfig::default();
        config.semantic.semantic_limit_multiplier = 200;
        config.semantic.lexical_fanout_multiplier = 300;
        config.semantic.semantic_fanout_multiplier = 400;
        assert_eq!(semantic_fanout_limits(20, &config), (1000, 2000, 1000));
    }
}
