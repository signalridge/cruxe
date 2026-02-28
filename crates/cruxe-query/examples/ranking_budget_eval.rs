use cruxe_core::config::RankingSignalBudgetConfig;
use cruxe_query::ranking::rerank_with_reasons_with_budget;
use cruxe_query::search::SearchResult;
use serde::Serialize;
use std::error::Error;
use std::str::FromStr;

#[derive(Debug, Clone)]
struct FixtureCase {
    id: &'static str,
    query: &'static str,
    expected_top_result_id: &'static str,
    candidates: Vec<SearchResult>,
}

#[derive(Debug, Clone, Copy)]
enum EvalProfile {
    PreContract,
    PostContract,
}

impl EvalProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::PreContract => "pre_contract_simulated",
            Self::PostContract => "post_contract",
        }
    }
}

impl FromStr for EvalProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pre" | "pre_contract" | "pre_contract_simulated" => Ok(Self::PreContract),
            "post" | "post_contract" => Ok(Self::PostContract),
            other => Err(format!("unsupported profile: {other}")),
        }
    }
}

#[derive(Debug, Serialize)]
struct CaseReport {
    id: String,
    query: String,
    expected_top_result_id: String,
    observed_top_result_id: Option<String>,
    reciprocal_rank: f64,
    top_reason_summary: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    profile: String,
    total_cases: usize,
    top1_hit_rate: f64,
    mrr: f64,
    cases: Vec<CaseReport>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let profile = parse_profile(std::env::args().collect::<Vec<_>>().as_slice())?;
    let budgets = budgets_for_profile(profile);
    let fixtures = fixtures();

    let mut hits = 0usize;
    let mut rr_sum = 0.0_f64;
    let mut reports = Vec::with_capacity(fixtures.len());
    for case in fixtures {
        let mut candidates = case.candidates.clone();
        let reasons = rerank_with_reasons_with_budget(&mut candidates, case.query, &budgets);
        let observed_top = candidates.first().map(|result| result.result_id.clone());
        if observed_top.as_deref() == Some(case.expected_top_result_id) {
            hits += 1;
        }

        let rr = reciprocal_rank(&candidates, case.expected_top_result_id);
        rr_sum += rr;
        reports.push(CaseReport {
            id: case.id.to_string(),
            query: case.query.to_string(),
            expected_top_result_id: case.expected_top_result_id.to_string(),
            observed_top_result_id: observed_top,
            reciprocal_rank: rr,
            top_reason_summary: reasons
                .first()
                .and_then(|reason| serde_json::to_value(reason).ok())
                .unwrap_or_else(|| serde_json::json!({})),
        });
    }

    let total = reports.len();
    let report = EvalReport {
        profile: profile.as_str().to_string(),
        total_cases: total,
        top1_hit_rate: if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        },
        mrr: if total == 0 {
            0.0
        } else {
            rr_sum / total as f64
        },
        cases: reports,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn parse_profile(args: &[String]) -> Result<EvalProfile, String> {
    let mut profile = EvalProfile::PostContract;
    let mut idx = 1usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--profile" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    return Err("--profile requires a value".to_string());
                };
                profile = EvalProfile::from_str(value)?;
            }
            "--help" | "-h" => {
                return Err("usage: cargo run -p cruxe-query --example ranking_budget_eval -- --profile <pre|post>".to_string());
            }
            _ => {}
        }
        idx += 1;
    }
    Ok(profile)
}

fn budgets_for_profile(profile: EvalProfile) -> RankingSignalBudgetConfig {
    let mut budgets = RankingSignalBudgetConfig::default();
    match profile {
        EvalProfile::PreContract => {
            // Simulate the pre-contract regime: weak lexical floor + aggressive secondary boosts.
            budgets.exact_match.default = 0.0;
            budgets.qualified_name.default = 0.5;
            budgets.path_affinity.default = 2.0;
            budgets.definition_boost.default = 2.0;
            budgets.kind_match.default = 3.0;
            budgets.secondary_cap_when_exact.default = 6.0;
        }
        EvalProfile::PostContract => {
            // Canonical defaults (exact match dominates, conservative secondary budget).
        }
    }
    budgets
}

fn fixtures() -> Vec<FixtureCase> {
    vec![
        FixtureCase {
            id: "exact_vs_structural_a",
            query: "validate_token",
            expected_top_result_id: "exact_validate_token",
            candidates: vec![
                candidate(
                    "structural_hotspot",
                    "helper",
                    "auth::helper",
                    "src/auth/validate_token_hub.rs",
                    "class",
                    0.05,
                ),
                candidate(
                    "exact_validate_token",
                    "validate_token",
                    "auth::validate_token",
                    "src/auth/token.rs",
                    "function",
                    0.02,
                ),
            ],
        },
        FixtureCase {
            id: "exact_vs_structural_b",
            query: "AuthService",
            expected_top_result_id: "exact_auth_service",
            candidates: vec![
                candidate(
                    "structural_auth_module",
                    "module_index",
                    "auth::module_index",
                    "src/auth/AuthService_registry.rs",
                    "class",
                    0.10,
                ),
                candidate(
                    "exact_auth_service",
                    "AuthService",
                    "auth::AuthService",
                    "src/auth/service.rs",
                    "class",
                    0.01,
                ),
            ],
        },
        FixtureCase {
            id: "exact_with_test_penalty",
            query: "refresh_session",
            expected_top_result_id: "exact_refresh_session",
            candidates: vec![
                candidate(
                    "structural_refresh",
                    "refresh_helper",
                    "session::refresh_helper",
                    "src/session/refresh_session_pipeline.rs",
                    "class",
                    0.03,
                ),
                candidate(
                    "exact_refresh_session",
                    "refresh_session",
                    "session::refresh_session",
                    "src/session/session_test.rs",
                    "function",
                    0.02,
                ),
            ],
        },
    ]
}

fn candidate(
    result_id: &str,
    name: &str,
    qualified_name: &str,
    path: &str,
    kind: &str,
    bm25_score: f32,
) -> SearchResult {
    SearchResult {
        repo: "fixture-repo".to_string(),
        result_id: result_id.to_string(),
        symbol_id: Some(format!("sym-{result_id}")),
        symbol_stable_id: Some(format!("stable-{result_id}")),
        result_type: "symbol".to_string(),
        path: path.to_string(),
        line_start: 1,
        line_end: 2,
        kind: Some(kind.to_string()),
        name: Some(name.to_string()),
        qualified_name: Some(qualified_name.to_string()),
        language: "rust".to_string(),
        signature: None,
        visibility: Some("pub".to_string()),
        score: bm25_score,
        snippet: None,
        chunk_type: None,
        source_layer: None,
        provenance: "lexical".to_string(),
    }
}

fn reciprocal_rank(results: &[SearchResult], expected: &str) -> f64 {
    for (idx, result) in results.iter().enumerate() {
        if result.result_id == expected {
            return 1.0 / (idx as f64 + 1.0);
        }
    }
    0.0
}
