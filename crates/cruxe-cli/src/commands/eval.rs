use anyhow::{Context, Result};
use cruxe_core::config::Config;
use cruxe_core::constants;
use cruxe_core::types::generate_project_id;
use cruxe_query::retrieval_eval::{
    GatePolicy, GateVerdict, QueryExecutionOutcome, RetrievalGateReport, RetrievalIntent,
    RetrievalResult, RetrievalSuite, SuiteBaseline, compare_against_baseline, evaluate_with_runner,
    load_beir_suite, render_summary_table, render_trec_qrels, render_trec_run,
};
use cruxe_query::search::{SearchExecutionOptions, search_code_with_options};
use cruxe_state::{db, project, schema, tantivy_index::IndexSet};
use std::path::Path;
use std::time::Instant;

#[allow(clippy::too_many_arguments)]
pub fn run_retrieval(
    workspace: &Path,
    suite_path: Option<&Path>,
    baseline_path: &Path,
    policy_path: &Path,
    ref_name: &str,
    limit: usize,
    output_path: Option<&Path>,
    dry_run: bool,
    update_baseline: bool,
    beir_corpus_path: Option<&Path>,
    beir_queries_path: Option<&Path>,
    beir_qrels_path: Option<&Path>,
    trec_run_out: Option<&Path>,
    trec_qrels_out: Option<&Path>,
    config_file: Option<&Path>,
) -> Result<()> {
    let workspace = std::fs::canonicalize(workspace).context("Failed to resolve workspace")?;
    let workspace_str = workspace.to_string_lossy().to_string();

    let config = Config::load_with_file(Some(&workspace), config_file)?;
    let project_id = generate_project_id(&workspace_str);
    let data_dir = config.project_data_dir(&project_id);
    let db_path = data_dir.join(constants::STATE_DB_FILE);

    let index_set = IndexSet::open_existing(&data_dir).map_err(|err| {
        anyhow::anyhow!(
            "failed to open index set under {}: {}",
            data_dir.display(),
            err
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
        anyhow::bail!(
            "workspace {} is not registered; run `cruxe init --path <workspace>` first",
            workspace.display()
        );
    }

    let suite = if let (Some(corpus), Some(queries), Some(qrels)) =
        (beir_corpus_path, beir_queries_path, beir_qrels_path)
    {
        load_beir_suite(corpus, queries, qrels, RetrievalIntent::NaturalLanguage)
            .map_err(|err| anyhow::anyhow!("failed to load BEIR suite: {err}"))?
    } else if let Some(suite_path) = suite_path {
        RetrievalSuite::load_from_path(suite_path)
            .map_err(|err| anyhow::anyhow!("failed to load suite: {err}"))?
    } else {
        anyhow::bail!(
            "either --suite or BEIR inputs (--beir-corpus/--beir-queries/--beir-qrels) are required"
        );
    };

    let report = evaluate_with_runner(&suite, limit, |query| {
        let started = Instant::now();
        let response = search_code_with_options(
            &index_set,
            Some(&conn),
            &query.query,
            Some(ref_name),
            None,
            limit,
            false,
            SearchExecutionOptions {
                search_config: config.search.clone(),
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
                plan_override: None,
                policy_mode_override: None,
                policy_runtime: None,
            },
        );

        match response {
            Ok(response) => {
                let results = response
                    .results
                    .iter()
                    .map(|result| RetrievalResult {
                        path: result.path.clone(),
                        name: result.name.clone(),
                        qualified_name: result.qualified_name.clone(),
                        signature: result.signature.clone(),
                        symbol_stable_id: result.symbol_stable_id.clone(),
                        score: result.score as f64,
                    })
                    .collect::<Vec<_>>();
                QueryExecutionOutcome {
                    results,
                    latency_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                    semantic_degraded: response.metadata.semantic_degraded,
                    semantic_budget_exhausted: response.metadata.semantic_budget_exhausted,
                }
            }
            Err(err) => {
                eprintln!(
                    "[retrieval-eval] query_id={} failed with search error: {}",
                    query.id, err
                );
                QueryExecutionOutcome {
                    results: Vec::new(),
                    latency_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
                    semantic_degraded: true,
                    semantic_budget_exhausted: false,
                }
            }
        }
    });

    if update_baseline {
        let baseline = SuiteBaseline::from_report(&report);
        if let Some(parent) = baseline_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(baseline_path, serde_json::to_string_pretty(&baseline)?)?;
        println!("updated baseline: {}", baseline_path.display());
    }

    let baseline = SuiteBaseline::load_from_path(baseline_path).map_err(|err| {
        anyhow::anyhow!(
            "failed to load baseline {}: {}",
            baseline_path.display(),
            err
        )
    })?;
    baseline
        .validate_compatibility(&suite)
        .map_err(|err| anyhow::anyhow!("baseline compatibility check failed: {err}"))?;
    let policy = GatePolicy::load_from_path(policy_path).map_err(|err| {
        anyhow::anyhow!("failed to load policy {}: {}", policy_path.display(), err)
    })?;

    let gate = compare_against_baseline(&report, &baseline, &policy);
    let gate_report = RetrievalGateReport {
        report: report.clone(),
        gate: gate.clone(),
    };

    if let Some(output_path) = output_path {
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output_path, serde_json::to_string_pretty(&gate_report)?)?;
        println!("report: {}", output_path.display());
    } else {
        println!("{}", serde_json::to_string_pretty(&gate_report)?);
    }

    println!("{}", render_summary_table(&report, &gate));

    if let Some(path) = trec_run_out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, render_trec_run(&report))?;
        println!("trec run: {}", path.display());
    }
    if let Some(path) = trec_qrels_out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, render_trec_qrels(&suite))?;
        println!("trec qrels: {}", path.display());
    }

    if gate.verdict == GateVerdict::Fail {
        eprintln!("retrieval gate verdict: FAIL taxonomy={:?}", gate.taxonomy);
        if !dry_run {
            anyhow::bail!("retrieval gate failed (disable failure by using --dry-run)");
        }
    } else {
        println!("retrieval gate verdict: PASS");
    }

    Ok(())
}
