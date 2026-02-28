use cruxe_core::config::SearchConfig;
use cruxe_core::types::QueryIntent;
use cruxe_query::adaptive_plan::{PlanController, PlanSelectionInput, QueryPlan, plan_budget};
use cruxe_query::search::{SearchExecutionOptions, search_code_with_options};
use cruxe_state::tantivy_index::IndexSet;
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::tempdir;

#[derive(Debug, Deserialize)]
struct RouterFixturePack {
    version: String,
    cases: Vec<RouterFixtureCase>,
}

#[derive(Debug, Deserialize)]
struct RouterFixtureCase {
    id: String,
    intent: String,
    lexical_confidence: f64,
    semantic_runtime_available: bool,
    override_plan: Option<String>,
    expected_selected: String,
    expected_selection_reason: String,
}

#[derive(Debug, Deserialize)]
struct AmbiguousFixturePack {
    version: String,
    downgrade_rate_floor: f64,
    cases: Vec<AmbiguousFixtureCase>,
}

#[derive(Debug, Deserialize)]
struct AmbiguousFixtureCase {
    id: String,
    intent: String,
    lexical_confidence: f64,
    semantic_runtime_available: bool,
    override_plan: Option<String>,
    simulate_timeout: bool,
    expected_selected: String,
    expected_executed: String,
    expected_downgraded: bool,
    expected_downgrade_reason: Option<String>,
}

#[derive(Default)]
struct EvalRun {
    latencies_ms: Vec<f64>,
    selected_counts: BTreeMap<String, usize>,
    downgraded: usize,
}

fn fixture_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("semantic")
        .join(file)
}

fn load_json_fixture<T: for<'de> Deserialize<'de>>(file: &str) -> T {
    let path = fixture_path(file);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()))
}

fn parse_intent(raw: &str) -> QueryIntent {
    match raw {
        "symbol" => QueryIntent::Symbol,
        "path" => QueryIntent::Path,
        "error" => QueryIntent::Error,
        "natural_language" => QueryIntent::NaturalLanguage,
        other => panic!("unsupported fixture intent: {other}"),
    }
}

fn p95(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let rank = (0.95 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

fn run_eval(
    index_set: &IndexSet,
    search_config: &SearchConfig,
    iterations: usize,
    plan_override: Option<&str>,
) -> EvalRun {
    let mut eval = EvalRun::default();
    for _ in 0..iterations {
        let started = Instant::now();
        let response = search_code_with_options(
            index_set,
            None,
            "where is auth handled",
            Some("main"),
            None,
            10,
            false,
            SearchExecutionOptions {
                search_config: search_config.clone(),
                semantic_ratio_override: None,
                confidence_threshold_override: None,
                role: None,
                plan_override: plan_override.map(ToString::to_string),
                policy_mode_override: None,
                policy_runtime: None,
            },
        )
        .expect("eval search invocation should succeed");
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        eval.latencies_ms.push(elapsed_ms);
        *eval
            .selected_counts
            .entry(response.metadata.query_plan_selected)
            .or_default() += 1;
        if response.metadata.query_plan_downgraded {
            eval.downgraded += 1;
        }
    }
    eval
}

#[test]
fn adaptive_router_haystack_style_fixtures_select_expected_plan() {
    let fixtures: RouterFixturePack = load_json_fixture("adaptive-router-fixtures.v1.json");
    assert_eq!(fixtures.version, "adaptive-router-fixtures-v1");
    assert!(
        !fixtures.cases.is_empty(),
        "router fixtures must not be empty"
    );

    let config = SearchConfig::default().adaptive_plan;
    for case in fixtures.cases {
        let selection = PlanController::select(PlanSelectionInput {
            intent: parse_intent(&case.intent),
            lexical_confidence: case.lexical_confidence,
            semantic_runtime_available: case.semantic_runtime_available,
            override_plan: case.override_plan.as_deref(),
            config: &config,
        });
        assert_eq!(
            selection.selected.as_str(),
            case.expected_selected,
            "fixture {} selected plan mismatch",
            case.id
        );
        assert_eq!(
            selection.selection_reason.as_str(),
            case.expected_selection_reason,
            "fixture {} selection reason mismatch",
            case.id
        );
    }
}

#[test]
fn adaptive_router_llamaindex_style_ambiguous_fixtures_validate_downgrade_behavior() {
    let fixtures: AmbiguousFixturePack = load_json_fixture("adaptive-ambiguous-fixtures.v1.json");
    assert_eq!(fixtures.version, "adaptive-ambiguous-fixtures-v1");
    assert!(
        !fixtures.cases.is_empty(),
        "ambiguous fixtures must not be empty"
    );

    let mut downgraded = 0usize;
    let config = SearchConfig::default();
    for case in &fixtures.cases {
        let mut selection = PlanController::select(PlanSelectionInput {
            intent: parse_intent(&case.intent),
            lexical_confidence: case.lexical_confidence,
            semantic_runtime_available: case.semantic_runtime_available,
            override_plan: case.override_plan.as_deref(),
            config: &config.adaptive_plan,
        });
        if case.simulate_timeout {
            let budget = plan_budget(selection.executed, 10, &config);
            selection.ensure_latency_budget(budget.latency_budget_ms + 1, budget);
        }

        if selection.downgraded {
            downgraded += 1;
        }

        assert_eq!(
            selection.selected.as_str(),
            case.expected_selected,
            "fixture {} selected plan mismatch",
            case.id
        );
        assert_eq!(
            selection.executed.as_str(),
            case.expected_executed,
            "fixture {} executed plan mismatch",
            case.id
        );
        assert_eq!(
            selection.downgraded, case.expected_downgraded,
            "fixture {} downgraded flag mismatch",
            case.id
        );
        assert_eq!(
            selection
                .downgrade_reason
                .map(|reason| reason.as_str().to_string()),
            case.expected_downgrade_reason,
            "fixture {} downgrade reason mismatch",
            case.id
        );
    }

    let downgrade_rate = downgraded as f64 / fixtures.cases.len() as f64;
    assert!(
        downgrade_rate >= fixtures.downgrade_rate_floor,
        "downgrade_rate={} must be >= configured floor={}",
        downgrade_rate,
        fixtures.downgrade_rate_floor
    );
}

#[test]
fn adaptive_plan_benchmark_asserts_plan_p95_budgets_and_downgrade_rates() {
    let tmp = tempdir().unwrap();
    let index_set = IndexSet::open(tmp.path()).unwrap();
    let search_config = SearchConfig::default();

    let mut latencies_by_plan: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut downgrades_by_plan: BTreeMap<String, usize> = BTreeMap::new();
    let mut invocations_by_plan: BTreeMap<String, usize> = BTreeMap::new();

    let invocations = [
        ("lexical_fast", Some("lexical_fast"), "AuthHandler"),
        (
            "hybrid_standard",
            Some("hybrid_standard"),
            "where is auth handled",
        ),
        (
            "semantic_deep",
            Some("semantic_deep"),
            "which module handles auth failures?",
        ),
    ];

    for _ in 0..30 {
        for (expected_selected, override_plan, query) in invocations {
            let started = Instant::now();
            let response = search_code_with_options(
                &index_set,
                None,
                query,
                Some("main"),
                None,
                10,
                false,
                SearchExecutionOptions {
                    search_config: search_config.clone(),
                    semantic_ratio_override: None,
                    confidence_threshold_override: None,
                    role: None,
                    plan_override: override_plan.map(ToString::to_string),
                    policy_mode_override: None,
                    policy_runtime: None,
                },
            )
            .expect("search invocation should succeed");
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(
                response.metadata.query_plan_selected, expected_selected,
                "override should deterministically select expected plan"
            );

            latencies_by_plan
                .entry(expected_selected.to_string())
                .or_default()
                .push(elapsed_ms);
            *invocations_by_plan
                .entry(expected_selected.to_string())
                .or_default() += 1;
            if response.metadata.query_plan_downgraded {
                *downgrades_by_plan
                    .entry(expected_selected.to_string())
                    .or_default() += 1;
            }
        }
    }

    let expected_budget_ms = [
        (
            QueryPlan::LexicalFast.as_str(),
            search_config.adaptive_plan.lexical_fast_latency_budget_ms as f64,
        ),
        (
            QueryPlan::HybridStandard.as_str(),
            search_config
                .adaptive_plan
                .hybrid_standard_latency_budget_ms as f64,
        ),
        (
            QueryPlan::SemanticDeep.as_str(),
            search_config.adaptive_plan.semantic_deep_latency_budget_ms as f64,
        ),
    ];

    for (plan, budget_ms) in expected_budget_ms {
        let latencies = latencies_by_plan
            .get(plan)
            .unwrap_or_else(|| panic!("missing benchmark samples for plan={plan}"));
        let p95_latency = p95(latencies);
        println!(
            "adaptive-plan-benchmark plan={} p95_latency_ms={:.3} budget_ms={:.3}",
            plan, p95_latency, budget_ms
        );
        assert!(
            p95_latency <= budget_ms,
            "plan={} p95_latency_ms={} must be <= configured_budget_ms={}",
            plan,
            p95_latency,
            budget_ms
        );
    }

    for plan in [
        QueryPlan::HybridStandard.as_str(),
        QueryPlan::SemanticDeep.as_str(),
    ] {
        let total = invocations_by_plan
            .get(plan)
            .copied()
            .unwrap_or_else(|| panic!("missing invocation count for plan={plan}"));
        let downgraded = downgrades_by_plan.get(plan).copied().unwrap_or(0);
        let downgrade_rate = downgraded as f64 / total as f64;
        println!(
            "adaptive-plan-benchmark plan={} downgrade_rate={:.3} downgraded={} total={}",
            plan, downgrade_rate, downgraded, total
        );
        assert!(
            downgrade_rate >= 0.95,
            "plan={} downgrade_rate={} expected >= 0.95 when semantic runtime is unavailable",
            plan,
            downgrade_rate
        );
    }
}

#[test]
fn adaptive_plan_retrieval_eval_gate_compares_enabled_vs_disabled_baseline() {
    let tmp = tempdir().unwrap();
    let index_set = IndexSet::open(tmp.path()).unwrap();

    let mut baseline = SearchConfig::default();
    baseline.semantic.mode = "hybrid".to_string();
    baseline.adaptive_plan.enabled = false;
    let mut adaptive = SearchConfig::default();
    adaptive.semantic.mode = "hybrid".to_string();

    let baseline_eval = run_eval(&index_set, &baseline, 30, Some("lexical_fast"));
    let adaptive_eval = run_eval(&index_set, &adaptive, 30, Some("lexical_fast"));

    let baseline_p95 = p95(&baseline_eval.latencies_ms);
    let adaptive_p95 = p95(&adaptive_eval.latencies_ms);
    let baseline_downgrade_rate = baseline_eval.downgraded as f64 / 30.0;
    let adaptive_downgrade_rate = adaptive_eval.downgraded as f64 / 30.0;

    println!(
        "adaptive-plan-eval-gate baseline_p95_ms={:.3} adaptive_p95_ms={:.3}",
        baseline_p95, adaptive_p95
    );
    println!(
        "adaptive-plan-eval-gate baseline_downgrade_rate={:.3} adaptive_downgrade_rate={:.3}",
        baseline_downgrade_rate, adaptive_downgrade_rate
    );
    println!(
        "adaptive-plan-eval-gate baseline_selected={:?} adaptive_selected={:?}",
        baseline_eval.selected_counts, adaptive_eval.selected_counts
    );

    assert_eq!(
        baseline_eval
            .selected_counts
            .get(QueryPlan::HybridStandard.as_str())
            .copied()
            .unwrap_or(0),
        30,
        "baseline run should force hybrid_standard when adaptive planning is disabled"
    );
    assert_eq!(
        adaptive_eval
            .selected_counts
            .get(QueryPlan::LexicalFast.as_str())
            .copied()
            .unwrap_or(0),
        30,
        "adaptive run should honor lexical_fast override when enabled"
    );
    assert!(
        adaptive_downgrade_rate < baseline_downgrade_rate,
        "adaptive run should reduce downgrade rate vs disabled baseline"
    );
}
