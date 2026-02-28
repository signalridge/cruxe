use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::types::generate_project_id;
use cruxe_query::search::{SearchExecutionOptions, SearchResult, search_code_with_options};
use cruxe_state::{db, project, schema, tantivy_index::IndexSet};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug)]
struct CliArgs {
    workspace: PathBuf,
    config: PathBuf,
    query_pack: PathBuf,
    ref_name: String,
    limit: usize,
    output: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct QueryPack {
    version: String,
    queries: Vec<QueryCase>,
}

#[derive(Debug, Deserialize)]
struct QueryCase {
    id: String,
    language: String,
    query: String,
    expected_hint: String,
    #[serde(default = "default_intent")]
    intent: String,
}

#[derive(Debug, Serialize)]
struct QueryResultMetric {
    id: String,
    intent: String,
    latency_ms: f64,
    reciprocal_rank: f64,
    hit_rank: Option<usize>,
    zero_results: bool,
    semantic_degraded: bool,
    semantic_budget_exhausted: bool,
}

#[derive(Debug, Serialize)]
struct EvaluationReport {
    mode: String,
    query_pack_version: String,
    total_queries: usize,
    natural_language_queries: usize,
    symbol_queries: usize,
    latency_p95_ms: f64,
    latency_mean_ms: f64,
    mrr: f64,
    symbol_precision_at_1: f64,
    zero_result_rate: f64,
    degraded_query_rate: f64,
    semantic_budget_exhaustion_rate: f64,
    external_provider_blocked_count: usize,
    tier1_acceptance_profile: String,
    per_query: Vec<QueryResultMetric>,
}

fn default_intent() -> String {
    "natural_language".to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let workspace = std::fs::canonicalize(&args.workspace)?;
    let config_path = std::fs::canonicalize(&args.config)?;

    let config = Config::load_with_file(Some(&workspace), Some(&config_path))?;
    let workspace_str = workspace.to_string_lossy().to_string();
    let project_id = generate_project_id(&workspace_str);
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);

    let index_set = IndexSet::open_existing(&data_dir).map_err(|err| {
        format!(
            "failed to open index set under {}: {err}",
            data_dir.display()
        )
    })?;
    let conn = db::open_connection_with_config(
        &db_path,
        config.storage.busy_timeout_ms,
        config.storage.cache_size,
    )?;
    schema::create_tables(&conn)?;
    let registered = project::get_by_root(&conn, &workspace_str)?;
    if registered.is_none() {
        return Err(format!(
            "workspace {} is not registered; run `cruxe init` first",
            workspace.display()
        )
        .into());
    }

    let raw = std::fs::read_to_string(&args.query_pack)?;
    let query_pack: QueryPack = serde_json::from_str(&raw)?;

    let mut latencies = Vec::with_capacity(query_pack.queries.len());
    let mut metrics = Vec::with_capacity(query_pack.queries.len());
    let mut mrr_sum = 0.0_f64;
    let mut mrr_count: usize = 0;
    let mut symbol_hits: usize = 0;
    let mut symbol_count: usize = 0;
    let mut zero_results: usize = 0;
    let mut degraded_queries: usize = 0;
    let mut budget_exhausted_queries: usize = 0;
    let mut external_provider_blocked_count: usize = 0;

    for query_case in &query_pack.queries {
        let start = Instant::now();
        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            &query_case.query,
            Some(&args.ref_name),
            Some(&query_case.language),
            args.limit,
            false,
            SearchExecutionOptions {
                search_config: config.search.clone(),
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
                policy_mode_override: None,
            },
        )?;
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        latencies.push(latency_ms);

        if response.metadata.external_provider_blocked {
            external_provider_blocked_count += 1;
        }
        if response.metadata.semantic_degraded {
            degraded_queries += 1;
        }
        if response.metadata.semantic_budget_exhausted {
            budget_exhausted_queries += 1;
        }

        let (rr, rank) = reciprocal_rank(&response.results, &query_case.expected_hint);
        let is_symbol = query_case.intent.eq_ignore_ascii_case("symbol");
        if is_symbol {
            symbol_count += 1;
            if rank == Some(1) {
                symbol_hits += 1;
            }
        } else {
            mrr_count += 1;
            mrr_sum += rr;
        }

        let zero = response.results.is_empty();
        if zero {
            zero_results += 1;
        }

        metrics.push(QueryResultMetric {
            id: query_case.id.clone(),
            intent: query_case.intent.clone(),
            latency_ms,
            reciprocal_rank: rr,
            hit_rank: rank,
            zero_results: zero,
            semantic_degraded: response.metadata.semantic_degraded,
            semantic_budget_exhausted: response.metadata.semantic_budget_exhausted,
        });
    }

    let latency_p95_ms = percentile(&latencies, 0.95);
    let latency_mean_ms = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    let mrr = if mrr_count == 0 {
        0.0
    } else {
        mrr_sum / mrr_count as f64
    };
    let symbol_precision_at_1 = if symbol_count == 0 {
        1.0
    } else {
        symbol_hits as f64 / symbol_count as f64
    };
    let zero_result_rate = if metrics.is_empty() {
        0.0
    } else {
        zero_results as f64 / metrics.len() as f64
    };
    let degraded_query_rate = if metrics.is_empty() {
        0.0
    } else {
        degraded_queries as f64 / metrics.len() as f64
    };
    let semantic_budget_exhaustion_rate = if metrics.is_empty() {
        0.0
    } else {
        budget_exhausted_queries as f64 / metrics.len() as f64
    };

    let report = EvaluationReport {
        mode: config.search.semantic.mode.clone(),
        query_pack_version: query_pack.version,
        total_queries: metrics.len(),
        natural_language_queries: mrr_count,
        symbol_queries: symbol_count,
        latency_p95_ms,
        latency_mean_ms,
        mrr,
        symbol_precision_at_1,
        zero_result_rate,
        degraded_query_rate,
        semantic_budget_exhaustion_rate,
        external_provider_blocked_count,
        tier1_acceptance_profile:
            "Tier-1 acceptance target: p95 latency <= 500ms, report zero_result_rate + MRR evidence."
                .to_string(),
        per_query: metrics,
    };

    let json = serde_json::to_string_pretty(&report)?;
    if let Some(output) = &args.output {
        std::fs::write(output, json)?;
    } else {
        println!("{json}");
    }
    Ok(())
}

fn reciprocal_rank(results: &[SearchResult], expected_hint: &str) -> (f64, Option<usize>) {
    let expected = expected_hint.to_ascii_lowercase();
    for (idx, result) in results.iter().enumerate() {
        if result_matches_hint(result, &expected) {
            let rank = idx + 1;
            return (1.0 / rank as f64, Some(rank));
        }
    }
    (0.0, None)
}

fn result_matches_hint(result: &SearchResult, expected: &str) -> bool {
    let candidates = [
        Some(result.path.as_str()),
        result.name.as_deref(),
        result.qualified_name.as_deref(),
        result.signature.as_deref(),
        result.snippet.as_deref(),
        result.symbol_stable_id.as_deref(),
    ];
    candidates
        .iter()
        .flatten()
        .any(|candidate| candidate.to_ascii_lowercase().contains(expected))
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let rank = (p.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

fn parse_args() -> Result<CliArgs, Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut workspace: Option<PathBuf> = None;
    let mut config: Option<PathBuf> = None;
    let mut query_pack: Option<PathBuf> = None;
    let mut ref_name = "main".to_string();
    let mut limit: usize = 20;
    let mut output: Option<PathBuf> = None;

    let mut idx = 0usize;
    while idx < args.len() {
        let flag = &args[idx];
        let value = args.get(idx + 1).cloned();
        match flag.as_str() {
            "--workspace" => {
                workspace = Some(PathBuf::from(require_value(flag, value)?));
                idx += 2;
            }
            "--config" => {
                config = Some(PathBuf::from(require_value(flag, value)?));
                idx += 2;
            }
            "--query-pack" => {
                query_pack = Some(PathBuf::from(require_value(flag, value)?));
                idx += 2;
            }
            "--ref" => {
                ref_name = require_value(flag, value)?;
                idx += 2;
            }
            "--limit" => {
                let parsed = require_value(flag, value)?;
                limit = parsed.parse::<usize>()?;
                idx += 2;
            }
            "--output" => {
                output = Some(PathBuf::from(require_value(flag, value)?));
                idx += 2;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {flag}").into());
            }
        }
    }

    let workspace = workspace.ok_or_else(|| "--workspace is required".to_string())?;
    let config = config.ok_or_else(|| "--config is required".to_string())?;
    let query_pack = query_pack.ok_or_else(|| "--query-pack is required".to_string())?;

    Ok(CliArgs {
        workspace,
        config,
        query_pack,
        ref_name,
        limit,
        output,
    })
}

fn require_value(flag: &str, value: Option<String>) -> Result<String, Box<dyn Error>> {
    match value {
        Some(v) => Ok(v),
        None => Err(format!("missing value for {flag}").into()),
    }
}

fn print_usage() {
    eprintln!(
        "Usage:
  cargo run -p cruxe-query --example semantic_benchmark_eval -- \\
    --workspace <path> --config <path> --query-pack <path> [--ref <ref>] [--limit <n>] [--output <path>]"
    );
}
