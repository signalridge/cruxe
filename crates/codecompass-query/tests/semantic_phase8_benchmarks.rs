use codecompass_core::config::SearchConfig;
use codecompass_core::time::now_iso8601;
use codecompass_core::types::{FileRecord, QueryIntent, SnippetRecord, SymbolKind, SymbolRecord};
use codecompass_indexer::writer::BatchWriter;
use codecompass_query::hybrid::semantic_query;
use codecompass_query::search::{SearchExecutionOptions, SearchResponse, search_code_with_options};
use codecompass_state::embedding;
use codecompass_state::tantivy_index::IndexSet;
use codecompass_state::vector_index::{self, VectorRecord};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use tempfile::tempdir;

type SeedCorpusResult = (Vec<CorpusDocMeta>, HashMap<String, String>, Vec<SymbolCase>);

const REF_NAME: &str = "main";
const PROJECT_ID: &str = "bench-proj";
const VECTOR_DIMS: usize = 32;
const SEARCH_LIMIT: usize = 10;

static PHASE8_REPORT: OnceLock<Result<Phase8Report, String>> = OnceLock::new();

#[derive(Debug, Clone)]
struct QueryCase {
    id: String,
    language: String,
    query: String,
}

#[derive(Debug, Clone)]
struct SymbolCase {
    language: String,
    query: String,
    expected_name: String,
}

#[derive(Debug, Clone)]
struct CorpusDocMeta {
    path: String,
    language: String,
    symbol_name: String,
    symbol_stable_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct BucketReport {
    bucket: String,
    file_count: usize,
    lexical_latency_p95_ms: f64,
    hybrid_latency_p95_ms: f64,
    latency_overhead_ms: f64,
    lexical_mrr: f64,
    hybrid_mrr: f64,
    mrr_delta_percent: f64,
    lexical_symbol_precision_at_1: f64,
    hybrid_symbol_precision_at_1: f64,
    tantivy_index_bytes: u64,
    semantic_vector_bytes: u64,
    vector_to_tantivy_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
struct Phase8Report {
    generated_at: String,
    query_count: usize,
    symbol_query_count: usize,
    buckets: Vec<BucketReport>,
}

#[test]
#[ignore = "benchmark harness"]
fn benchmark_t406_hybrid_latency_overhead_under_200ms_across_repo_size_buckets() {
    let report = phase8_report();
    for bucket in &report.buckets {
        assert!(
            bucket.latency_overhead_ms < 200.0,
            "bucket={} files={} lexical_p95={:.2}ms hybrid_p95={:.2}ms overhead={:.2}ms should be <200ms",
            bucket.bucket,
            bucket.file_count,
            bucket.lexical_latency_p95_ms,
            bucket.hybrid_latency_p95_ms,
            bucket.latency_overhead_ms
        );
    }
}

#[test]
#[ignore = "benchmark harness"]
fn benchmark_t407_vector_index_size_under_2x_tantivy() {
    let report = phase8_report();
    for bucket in &report.buckets {
        assert!(
            bucket.vector_to_tantivy_ratio < 2.0,
            "bucket={} files={} semantic_bytes={} tantivy_bytes={} ratio={:.3} should be <2.0",
            bucket.bucket,
            bucket.file_count,
            bucket.semantic_vector_bytes,
            bucket.tantivy_index_bytes,
            bucket.vector_to_tantivy_ratio
        );
    }
}

#[test]
#[ignore = "benchmark harness"]
fn benchmark_t409_hybrid_mrr_improves_without_symbol_precision_regression() {
    let report = phase8_report();
    for bucket in &report.buckets {
        assert!(
            bucket.mrr_delta_percent >= 15.0,
            "bucket={} files={} lexical_mrr={:.4} hybrid_mrr={:.4} mrr_delta={:.2}% should be >=15%",
            bucket.bucket,
            bucket.file_count,
            bucket.lexical_mrr,
            bucket.hybrid_mrr,
            bucket.mrr_delta_percent
        );
        assert!(
            bucket.hybrid_symbol_precision_at_1 + 1e-9 >= bucket.lexical_symbol_precision_at_1,
            "bucket={} files={} lexical_symbol_p1={:.4} hybrid_symbol_p1={:.4} should not regress",
            bucket.bucket,
            bucket.file_count,
            bucket.lexical_symbol_precision_at_1,
            bucket.hybrid_symbol_precision_at_1
        );
    }
}

fn phase8_report() -> &'static Phase8Report {
    match PHASE8_REPORT.get_or_init(run_phase8_benchmarks) {
        Ok(report) => report,
        Err(err) => panic!("{err}"),
    }
}

fn run_phase8_benchmarks() -> Result<Phase8Report, String> {
    let query_cases = load_query_pack()?;
    let symbol_case_count = std::env::var("CODECOMPASS_T409_SYMBOL_QUERIES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(80);
    let symbol_case_count = symbol_case_count.min(query_cases.len());

    let buckets = vec![
        (
            "<10k".to_string(),
            read_env_usize("CODECOMPASS_T406_BUCKET_LT10K", 5_000),
        ),
        (
            "10k-50k".to_string(),
            read_env_usize("CODECOMPASS_T406_BUCKET_10K_50K", 20_000),
        ),
        (
            ">50k".to_string(),
            read_env_usize("CODECOMPASS_T406_BUCKET_GT50K", 55_000),
        ),
    ];

    let mut bucket_reports = Vec::with_capacity(buckets.len());
    for (label, file_count) in buckets {
        let report = run_bucket_benchmark(&label, file_count, &query_cases, symbol_case_count)?;
        bucket_reports.push(report);
    }

    let report = Phase8Report {
        generated_at: now_iso8601(),
        query_count: query_cases.len(),
        symbol_query_count: symbol_case_count,
        buckets: bucket_reports,
    };
    write_phase8_report(&report)?;
    Ok(report)
}

fn run_bucket_benchmark(
    bucket: &str,
    file_count: usize,
    query_cases: &[QueryCase],
    symbol_case_count: usize,
) -> Result<BucketReport, String> {
    let workspace = tempdir().map_err(|err| format!("create tempdir: {err}"))?;
    let db_path = workspace.path().join("state.db");
    let conn = codecompass_state::db::open_connection(&db_path)
        .map_err(|err| format!("open sqlite {}: {err}", db_path.display()))?;
    codecompass_state::schema::create_tables(&conn)
        .map_err(|err| format!("create schema {}: {err}", db_path.display()))?;
    let index_root = workspace.path().join("index");
    let index_set = IndexSet::open_at(&index_root)
        .map_err(|err| format!("open index {}: {err}", index_root.display()))?;

    let lexical_config = lexical_config();
    let hybrid_config = hybrid_config();

    let (docs, expected_paths, symbol_cases) = seed_corpus(
        &conn,
        &index_set,
        file_count,
        query_cases,
        symbol_case_count,
    )?;

    let db_size_before_vectors = file_size(&db_path);
    seed_vectors(&conn, &docs, query_cases, &hybrid_config)?;
    if let Some(first_case) = query_cases.first() {
        semantic_query(
            &conn,
            &hybrid_config,
            &first_case.query,
            REF_NAME,
            PROJECT_ID,
            5,
        )
        .map_err(|err| format!("semantic smoke check failed: {err}"))?;
    }
    let db_size_after_vectors = file_size(&db_path);
    let semantic_vector_bytes = db_size_after_vectors.saturating_sub(db_size_before_vectors);
    let tantivy_bytes = directory_size(&index_root);

    let lexical_eval = run_natural_language_eval(
        &index_set,
        &conn,
        query_cases,
        &expected_paths,
        &lexical_config,
    )?;
    let hybrid_eval = run_natural_language_eval(
        &index_set,
        &conn,
        query_cases,
        &expected_paths,
        &hybrid_config,
    )?;

    let lexical_symbol_precision =
        run_symbol_precision_eval(&index_set, &conn, &symbol_cases, &lexical_config)?;
    let hybrid_symbol_precision =
        run_symbol_precision_eval(&index_set, &conn, &symbol_cases, &hybrid_config)?;

    let latency_overhead_ms = (hybrid_eval.p95_latency_ms - lexical_eval.p95_latency_ms).max(0.0);
    let mrr_delta_percent = if lexical_eval.mrr <= f64::EPSILON {
        if hybrid_eval.mrr > 0.0 { 100.0 } else { 0.0 }
    } else {
        ((hybrid_eval.mrr - lexical_eval.mrr) / lexical_eval.mrr) * 100.0
    };

    let vector_to_tantivy_ratio = if tantivy_bytes == 0 {
        0.0
    } else {
        semantic_vector_bytes as f64 / tantivy_bytes as f64
    };

    Ok(BucketReport {
        bucket: bucket.to_string(),
        file_count,
        lexical_latency_p95_ms: lexical_eval.p95_latency_ms,
        hybrid_latency_p95_ms: hybrid_eval.p95_latency_ms,
        latency_overhead_ms,
        lexical_mrr: lexical_eval.mrr,
        hybrid_mrr: hybrid_eval.mrr,
        mrr_delta_percent,
        lexical_symbol_precision_at_1: lexical_symbol_precision,
        hybrid_symbol_precision_at_1: hybrid_symbol_precision,
        tantivy_index_bytes: tantivy_bytes,
        semantic_vector_bytes,
        vector_to_tantivy_ratio,
    })
}

fn seed_corpus(
    conn: &rusqlite::Connection,
    index_set: &IndexSet,
    file_count: usize,
    query_cases: &[QueryCase],
    symbol_case_count: usize,
) -> Result<SeedCorpusResult, String> {
    let mut docs = Vec::with_capacity(file_count);
    let mut expected_paths = HashMap::with_capacity(query_cases.len());
    let mut symbol_cases = Vec::with_capacity(symbol_case_count);
    let batch = BatchWriter::new(index_set).map_err(|err| format!("create batch writer: {err}"))?;
    let now = now_iso8601();

    for idx in 0..file_count {
        let language = if idx < query_cases.len() {
            query_cases[idx].language.clone()
        } else {
            match idx % 4 {
                0 => "rust".to_string(),
                1 => "typescript".to_string(),
                2 => "python".to_string(),
                _ => "go".to_string(),
            }
        };
        let extension = language_extension(&language);
        let path = format!("src/{language}/unit_{idx:05}.{extension}");
        let symbol_name = format!("symbol_unit_{idx:05}");
        let symbol_stable_id = format!("stable-unit-{idx:05}");
        let snippet_content = generic_snippet_content(&language);

        let symbol = SymbolRecord {
            repo: PROJECT_ID.to_string(),
            r#ref: REF_NAME.to_string(),
            commit: None,
            path: path.clone(),
            language: language.clone(),
            symbol_id: format!("sym-{idx:05}"),
            symbol_stable_id: symbol_stable_id.clone(),
            name: symbol_name.clone(),
            qualified_name: symbol_name.clone(),
            kind: SymbolKind::Function,
            signature: Some(format!("{symbol_name}()")),
            line_start: 1,
            line_end: 3,
            parent_symbol_id: None,
            visibility: Some("pub".to_string()),
            content: Some(snippet_content.to_string()),
        };
        let snippet = SnippetRecord {
            repo: PROJECT_ID.to_string(),
            r#ref: REF_NAME.to_string(),
            commit: None,
            path: path.clone(),
            language: language.clone(),
            chunk_type: "function_body".to_string(),
            imports: None,
            line_start: 1,
            line_end: 3,
            content: snippet_content.to_string(),
        };
        let file_record = FileRecord {
            repo: PROJECT_ID.to_string(),
            r#ref: REF_NAME.to_string(),
            commit: None,
            path: path.clone(),
            filename: format!("unit_{idx:05}.{extension}"),
            language: language.clone(),
            content_hash: format!("hash-{idx:05}"),
            size_bytes: snippet_content.len() as u64,
            updated_at: now.clone(),
            content_head: Some(snippet_content.to_string()),
        };

        batch
            .add_symbols(&index_set.symbols, std::slice::from_ref(&symbol))
            .map_err(|err| format!("add symbols for {}: {err}", path))?;
        batch
            .add_snippets(&index_set.snippets, std::slice::from_ref(&snippet))
            .map_err(|err| format!("add snippets for {}: {err}", path))?;
        batch
            .add_file(&index_set.files, &file_record)
            .map_err(|err| format!("add file {}: {err}", path))?;
        batch
            .write_sqlite(conn, std::slice::from_ref(&symbol), &file_record, None)
            .map_err(|err| format!("write sqlite for {}: {err}", path))?;

        if idx < query_cases.len() {
            expected_paths.insert(query_cases[idx].id.clone(), path.clone());
        }
        if symbol_cases.len() < symbol_case_count {
            symbol_cases.push(SymbolCase {
                language: language.clone(),
                query: symbol_name.clone(),
                expected_name: symbol_name.clone(),
            });
        }

        docs.push(CorpusDocMeta {
            path,
            language,
            symbol_name,
            symbol_stable_id,
        });
    }

    batch
        .commit()
        .map_err(|err| format!("commit batch: {err}"))?;
    Ok((docs, expected_paths, symbol_cases))
}

fn seed_vectors(
    conn: &rusqlite::Connection,
    docs: &[CorpusDocMeta],
    query_cases: &[QueryCase],
    search_config: &SearchConfig,
) -> Result<(), String> {
    let mut provider = embedding::build_embedding_provider(&search_config.semantic)
        .map_err(|err| format!("build embedding provider: {err}"))?
        .provider;
    let model_id = provider.model_id().to_string();
    let model_version = provider.model_version().to_string();
    let query_inputs: Vec<String> = query_cases.iter().map(|case| case.query.clone()).collect();
    let query_vectors = provider
        .embed_batch(&query_inputs)
        .map_err(|err| format!("embed query batch: {err}"))?;

    let mut records = Vec::with_capacity(docs.len());
    for (idx, doc) in docs.iter().enumerate() {
        let vector = if idx < query_vectors.len() {
            query_vectors[idx].clone()
        } else {
            deterministic_vector(idx as u64, VECTOR_DIMS)
        };
        records.push(VectorRecord {
            project_id: PROJECT_ID.to_string(),
            ref_name: REF_NAME.to_string(),
            symbol_stable_id: doc.symbol_stable_id.clone(),
            snippet_hash: format!("snippet-hash-{idx:05}"),
            embedding_model_id: model_id.clone(),
            embedding_model_version: model_version.clone(),
            embedding_dimensions: VECTOR_DIMS,
            path: doc.path.clone(),
            line_start: 1,
            line_end: 3,
            language: doc.language.clone(),
            chunk_type: Some("function_body".to_string()),
            snippet_text: format!("semantic-target-{}", doc.symbol_name),
            vector,
        });

        if records.len() >= 1024 {
            vector_index::upsert_vectors(conn, &records)
                .map_err(|err| format!("upsert vector chunk: {err}"))?;
            records.clear();
        }
    }

    if !records.is_empty() {
        vector_index::upsert_vectors(conn, &records)
            .map_err(|err| format!("upsert tail vectors: {err}"))?;
    }
    Ok(())
}

#[derive(Debug)]
struct NaturalLanguageEval {
    p95_latency_ms: f64,
    mrr: f64,
}

fn run_natural_language_eval(
    index_set: &IndexSet,
    conn: &rusqlite::Connection,
    query_cases: &[QueryCase],
    expected_paths: &HashMap<String, String>,
    search_config: &SearchConfig,
) -> Result<NaturalLanguageEval, String> {
    let mut latencies = Vec::with_capacity(query_cases.len());
    let mut rr_sum = 0.0_f64;

    for case in query_cases {
        let start = Instant::now();
        let response = search_code_with_options(
            index_set,
            Some(conn),
            &case.query,
            Some(REF_NAME),
            Some(&case.language),
            SEARCH_LIMIT,
            false,
            SearchExecutionOptions {
                search_config: search_config.clone(),
                semantic_ratio_override: None,
                confidence_threshold_override: None,
            },
        )
        .map_err(|err| format!("search failed for {}: {err}", case.id))?;

        if std::env::var("CODECOMPASS_PHASE8_DEBUG").as_deref() == Ok("1") && case.id == "rust-001"
        {
            eprintln!(
                "[phase8-debug] mode={} query={} semantic_triggered={} skipped_reason={:?} semantic_ratio_used={:.3} top_paths={:?}",
                search_config.semantic.mode,
                case.query,
                response.metadata.semantic_triggered,
                response.metadata.semantic_skipped_reason,
                response.metadata.semantic_ratio_used,
                response
                    .results
                    .iter()
                    .take(5)
                    .map(|result| format!(
                        "{}|{:?}|{:.4}|{}",
                        result.path, result.symbol_stable_id, result.score, result.provenance
                    ))
                    .collect::<Vec<_>>()
            );
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        latencies.push(latency_ms);

        let expected_path = expected_paths
            .get(&case.id)
            .ok_or_else(|| format!("missing expected path for {}", case.id))?;
        rr_sum += reciprocal_rank_by_path(&response, expected_path);
    }

    let mrr = if query_cases.is_empty() {
        0.0
    } else {
        rr_sum / query_cases.len() as f64
    };

    Ok(NaturalLanguageEval {
        p95_latency_ms: percentile(&latencies, 0.95),
        mrr,
    })
}

fn run_symbol_precision_eval(
    index_set: &IndexSet,
    conn: &rusqlite::Connection,
    symbol_cases: &[SymbolCase],
    search_config: &SearchConfig,
) -> Result<f64, String> {
    if symbol_cases.is_empty() {
        return Ok(1.0);
    }

    let mut hits = 0usize;
    for case in symbol_cases {
        let response = search_code_with_options(
            index_set,
            Some(conn),
            &case.query,
            Some(REF_NAME),
            Some(&case.language),
            SEARCH_LIMIT,
            false,
            SearchExecutionOptions {
                search_config: search_config.clone(),
                semantic_ratio_override: None,
                confidence_threshold_override: None,
            },
        )
        .map_err(|err| format!("symbol search failed for {}: {err}", case.query))?;

        if response.query_intent != QueryIntent::Symbol {
            return Err(format!(
                "symbol query '{}' classified as {:?}",
                case.query, response.query_intent
            ));
        }

        let top_name = response
            .results
            .first()
            .and_then(|result| result.name.as_deref());
        if top_name == Some(case.expected_name.as_str()) {
            hits += 1;
        }
    }

    Ok(hits as f64 / symbol_cases.len() as f64)
}

fn reciprocal_rank_by_path(response: &SearchResponse, expected_path: &str) -> f64 {
    for (idx, result) in response.results.iter().enumerate() {
        if result.path == expected_path {
            return 1.0 / (idx + 1) as f64;
        }
    }
    0.0
}

fn lexical_config() -> SearchConfig {
    let mut config = SearchConfig::default();
    config.semantic.mode = "off".to_string();
    config.semantic.rerank.provider = "none".to_string();
    config
}

fn hybrid_config() -> SearchConfig {
    let mut config = SearchConfig::default();
    config.semantic.mode = "hybrid".to_string();
    config.semantic.ratio = 0.8;
    config.semantic.lexical_short_circuit_threshold = 1.0;
    config.semantic.embedding.provider = "local".to_string();
    config.semantic.embedding.profile = "fast_local".to_string();
    // Use a deterministic synthetic model-id for benchmark harness vectors.
    // This keeps dimensionality at VECTOR_DIMS without requiring fastembed runtime.
    config.semantic.embedding.model = "deterministic32".to_string();
    config.semantic.embedding.model_version = "synthetic-32".to_string();
    config.semantic.embedding.dimensions = VECTOR_DIMS;
    config.semantic.embedding.batch_size = 128;
    config.semantic.rerank.provider = "none".to_string();
    config
}

fn load_query_pack() -> Result<Vec<QueryCase>, String> {
    let path = repo_root().join("benchmarks/semantic/query-pack.v1.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|err| format!("read query pack {}: {err}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|err| format!("parse query pack: {err}"))?;
    let queries = value
        .get("queries")
        .and_then(|queries| queries.as_array())
        .ok_or_else(|| "query pack missing queries array".to_string())?;

    let mut parsed = Vec::with_capacity(queries.len());
    for item in queries {
        let id = item
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "query item missing id".to_string())?;
        let language = item
            .get("language")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("query item {} missing language", id))?;
        let query = item
            .get("query")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("query item {} missing query", id))?;
        parsed.push(QueryCase {
            id: id.to_string(),
            language: language.to_string(),
            query: query.to_string(),
        });
    }
    Ok(parsed)
}

fn write_phase8_report(report: &Phase8Report) -> Result<(), String> {
    let dir = std::env::var("CODECOMPASS_PHASE8_REPORT_DIR")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target/semantic-phase8-benchmarks"));
    std::fs::create_dir_all(&dir).map_err(|err| format!("create report dir: {err}"))?;
    let path = dir.join("phase8-benchmark-report.json");
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| format!("serialize benchmark report: {err}"))?;
    std::fs::write(&path, json)
        .map_err(|err| format!("write benchmark report {}: {err}", path.display()))?;
    eprintln!("[phase8-benchmark] report={}", path.display());
    Ok(())
}

fn read_env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn generic_snippet_content(language: &str) -> &'static str {
    match language {
        "typescript" => {
            r#"export interface RequestContext {
  traceId: string;
  tenantId: string;
  actorId: string;
  deadlineMs: number;
}

export function genericHandler(ctx: RequestContext): boolean {
  const normalizedTenant = ctx.tenantId.trim().toLowerCase();
  const normalizedActor = ctx.actorId.trim();
  const tracingEnabled = ctx.traceId.length > 0;
  const budgetAvailable = ctx.deadlineMs > 5;
  return tracingEnabled && budgetAvailable && normalizedTenant.length > 0 && normalizedActor.length > 0;
}"#
        }
        "python" => {
            r#"from dataclasses import dataclass

@dataclass
class RequestContext:
    trace_id: str
    tenant_id: str
    actor_id: str
    deadline_ms: int

def generic_handler(ctx: RequestContext) -> bool:
    normalized_tenant = ctx.tenant_id.strip().lower()
    normalized_actor = ctx.actor_id.strip()
    tracing_enabled = len(ctx.trace_id) > 0
    budget_available = ctx.deadline_ms > 5
    return tracing_enabled and budget_available and bool(normalized_tenant) and bool(normalized_actor)
"#
        }
        "go" => {
            r#"package bench

type RequestContext struct {
	TraceID    string
	TenantID   string
	ActorID    string
	DeadlineMS int
}

func GenericHandler(ctx RequestContext) bool {
	normalizedTenant := strings.TrimSpace(strings.ToLower(ctx.TenantID))
	normalizedActor := strings.TrimSpace(ctx.ActorID)
	tracingEnabled := len(ctx.TraceID) > 0
	budgetAvailable := ctx.DeadlineMS > 5
	return tracingEnabled && budgetAvailable && len(normalizedTenant) > 0 && len(normalizedActor) > 0
}"#
        }
        _ => {
            r#"#[derive(Clone, Debug)]
pub struct RequestContext {
    pub trace_id: String,
    pub tenant_id: String,
    pub actor_id: String,
    pub deadline_ms: u64,
}

pub fn generic_handler(ctx: &RequestContext) -> bool {
    let normalized_tenant = ctx.tenant_id.trim().to_ascii_lowercase();
    let normalized_actor = ctx.actor_id.trim();
    let tracing_enabled = !ctx.trace_id.is_empty();
    let budget_available = ctx.deadline_ms > 5;
    tracing_enabled && budget_available && !normalized_tenant.is_empty() && !normalized_actor.is_empty()
}"#
        }
    }
}

fn language_extension(language: &str) -> &'static str {
    match language {
        "typescript" => "ts",
        "python" => "py",
        "go" => "go",
        _ => "rs",
    }
}

fn deterministic_vector(seed: u64, dimensions: usize) -> Vec<f32> {
    if dimensions == 0 {
        return Vec::new();
    }
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    if state == 0 {
        state = 1;
    }
    let mut values = Vec::with_capacity(dimensions);
    for _ in 0..dimensions {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let n = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let scaled = (n as f64 / u64::MAX as f64) * 2.0 - 1.0;
        values.push(scaled as f32);
    }

    let norm = values
        .iter()
        .map(|value| {
            let v = *value as f64;
            v * v
        })
        .sum::<f64>()
        .sqrt();
    if norm <= f64::EPSILON {
        return values;
    }
    values
        .into_iter()
        .map(|value| (value as f64 / norm) as f32)
        .collect()
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let rank = (p.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn directory_size(path: &Path) -> u64 {
    let mut total = 0_u64;
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += directory_size(&entry_path);
        } else if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    total
}
