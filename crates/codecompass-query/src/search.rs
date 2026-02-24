use codecompass_core::error::StateError;
use codecompass_core::types::{QueryIntent, RankingReasons, RefScope, SymbolRecord};
use codecompass_state::tantivy_index::IndexSet;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::{IndexRecordOption, Value};
use tracing::debug;

use crate::intent::classify_intent;
use crate::planner::build_plan_with_ref;
use crate::ranking::{rerank, rerank_with_reasons};

/// A search result from search_code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<SearchDebugInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_reasons: Option<Vec<RankingReasons>>,
}

/// Optional debug payload for search_code.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchDebugInfo {
    pub join_status: JoinStatus,
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
    let mut debug = tracing::enabled!(tracing::Level::DEBUG).then_some(SearchDebugInfo::default());

    let intent = classify_intent(query);
    let ref_scope = match r#ref {
        Some(explicit) => RefScope::explicit(explicit),
        None => RefScope::live(),
    };
    let plan = build_plan_with_ref(intent, ref_scope);
    let effective_ref = plan.ref_scope.r#ref.clone();
    let search_ref = Some(effective_ref.as_str());

    let mut all_results = Vec::new();

    // Search each index and apply RRF (Reciprocal Rank Fusion) scoring.
    // RRF score per source = weight / (k + rank), where k=60 is the standard constant.
    // Results are already sorted by Tantivy relevance within each source.
    const RRF_K: f32 = 60.0;

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
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.symbol_weight, RRF_K);
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
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.snippet_weight, RRF_K);
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
            },
            limit,
        )?;
        apply_rrf_scores(&mut results, plan.file_weight, RRF_K);
        all_results.extend(results);
    }

    let total = all_results.len();

    // Apply rule-based reranking boosts on top of RRF scores
    let ranking_reasons = if debug_ranking {
        let reasons = rerank_with_reasons(&mut all_results, query);
        // Truncate reasons to match final result limit
        all_results.truncate(limit);
        Some(reasons.into_iter().take(limit).collect::<Vec<_>>())
    } else {
        rerank(&mut all_results, query);
        all_results.truncate(limit);
        None
    };

    // Build suggested next actions
    let suggested = build_suggested_actions(&all_results, query, search_ref);

    debug!(
        query,
        ?intent,
        results = all_results.len(),
        total,
        "search_code"
    );

    Ok(SearchResponse {
        results: all_results,
        query_intent: intent,
        total_candidates: total,
        suggested_next_actions: suggested,
        debug,
        ranking_reasons,
    })
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

#[derive(Clone, Copy)]
struct SearchScope<'a> {
    ref_name: Option<&'a str>,
    language: Option<&'a str>,
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

    let query_parser = QueryParser::for_index(index, search_fields);
    let parsed_query = query_parser
        .parse_query(query)
        .map_err(StateError::tantivy)?;

    // Build final query with optional ref and language filters
    let final_query: Box<dyn tantivy::query::Query> =
        if scope.ref_name.is_some() || scope.language.is_some() {
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

    let Ok(symbols) = codecompass_state::symbols::find_symbols_by_location(
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
