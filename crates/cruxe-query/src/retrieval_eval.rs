use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum RetrievalEvalError {
    #[error("failed to read suite {path}: {source}")]
    ReadSuite {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse suite {path}: {source}")]
    ParseSuite {
        path: String,
        source: serde_json::Error,
    },
    #[error("suite validation error: {0}")]
    Validation(String),
    #[error("failed to read BEIR file {path}: {source}")]
    ReadBeir {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse BEIR line in {path}: {reason}")]
    ParseBeirLine { path: String, reason: String },
    #[error("failed to parse policy {path}: {source}")]
    ParsePolicy {
        path: String,
        source: serde_json::Error,
    },
    #[error("failed to parse baseline {path}: {source}")]
    ParseBaseline {
        path: String,
        source: serde_json::Error,
    },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalIntent {
    Symbol,
    Path,
    Error,
    #[default]
    NaturalLanguage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectedTarget {
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQueryCase {
    pub id: String,
    pub query: String,
    pub intent: RetrievalIntent,
    #[serde(default)]
    pub expected_targets: Vec<ExpectedTarget>,
    #[serde(default)]
    pub negative_targets: Vec<ExpectedTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSuite {
    pub version: String,
    pub queries: Vec<RetrievalQueryCase>,
}

impl RetrievalSuite {
    pub fn load_from_path(path: &Path) -> Result<Self, RetrievalEvalError> {
        let raw =
            std::fs::read_to_string(path).map_err(|source| RetrievalEvalError::ReadSuite {
                path: path.display().to_string(),
                source,
            })?;
        let suite = serde_json::from_str::<Self>(&raw).map_err(|source| {
            RetrievalEvalError::ParseSuite {
                path: path.display().to_string(),
                source,
            }
        })?;
        suite.validate()?;
        Ok(suite)
    }

    pub fn validate(&self) -> Result<(), RetrievalEvalError> {
        if self.version.trim().is_empty() {
            return Err(RetrievalEvalError::Validation(
                "suite version must be non-empty".to_string(),
            ));
        }
        if self.queries.is_empty() {
            return Err(RetrievalEvalError::Validation(
                "suite must include at least one query".to_string(),
            ));
        }

        let mut ids = BTreeSet::new();
        for query in &self.queries {
            if query.id.trim().is_empty() {
                return Err(RetrievalEvalError::Validation(
                    "query id must be non-empty".to_string(),
                ));
            }
            if query.query.trim().is_empty() {
                return Err(RetrievalEvalError::Validation(format!(
                    "query '{}' has empty query text",
                    query.id
                )));
            }
            if query.expected_targets.is_empty() {
                return Err(RetrievalEvalError::Validation(format!(
                    "query '{}' must include at least one expected target",
                    query.id
                )));
            }
            if !ids.insert(query.id.clone()) {
                return Err(RetrievalEvalError::Validation(format!(
                    "duplicate query id '{}'",
                    query.id
                )));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_stable_id: Option<String>,
    pub score: f64,
}

impl RetrievalResult {
    pub fn new(path: impl Into<String>, name: Option<&str>, score: f64) -> Self {
        Self {
            path: path.into(),
            name: name.map(ToString::to_string),
            qualified_name: None,
            signature: None,
            symbol_stable_id: None,
            score,
        }
    }

    pub fn doc_id(&self) -> String {
        if let Some(symbol_id) = &self.symbol_stable_id {
            return symbol_id.clone();
        }
        if let Some(qualified_name) = &self.qualified_name {
            return format!("{}::{}", self.path, qualified_name);
        }
        if let Some(name) = &self.name {
            return format!("{}::{}", self.path, name);
        }
        self.path.clone()
    }
}

#[derive(Debug, Clone)]
pub struct QueryExecutionOutcome {
    pub results: Vec<RetrievalResult>,
    pub latency_ms: Option<f64>,
    pub semantic_degraded: bool,
    pub semantic_budget_exhausted: bool,
}

impl QueryExecutionOutcome {
    pub fn with_latency(mut self, latency_ms: f64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }
}

impl From<Vec<RetrievalResult>> for QueryExecutionOutcome {
    fn from(results: Vec<RetrievalResult>) -> Self {
        Self {
            results,
            latency_ms: None,
            semantic_degraded: false,
            semantic_budget_exhausted: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopResultEntry {
    pub doc_id: String,
    pub rank: usize,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEvaluation {
    pub id: String,
    pub intent: RetrievalIntent,
    pub latency_ms: f64,
    pub reciprocal_rank: f64,
    pub recall_at_k: f64,
    pub ndcg_at_k: f64,
    pub hit_rank: Option<usize>,
    pub zero_results: bool,
    pub cluster_ratio: f64,
    pub semantic_degraded: bool,
    pub semantic_budget_exhausted: bool,
    #[serde(default)]
    pub negative_hits: usize,
    pub top_results: Vec<TopResultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateMetrics {
    pub recall_at_k: f64,
    pub mrr: f64,
    pub ndcg_at_k: f64,
    pub zero_result_rate: f64,
    pub clustering_ratio: f64,
    pub degraded_query_rate: f64,
    pub semantic_budget_exhaustion_rate: f64,
    #[serde(default)]
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_mean_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySummary {
    pub sample_count: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub mean_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub version: String,
    pub suite_version: String,
    pub total_queries: usize,
    pub metrics: AggregateMetrics,
    pub latency_by_intent: BTreeMap<RetrievalIntent, LatencySummary>,
    pub per_query: Vec<QueryEvaluation>,
}

pub fn evaluate_with_runner<F, O>(
    suite: &RetrievalSuite,
    limit: usize,
    mut runner: F,
) -> EvaluationReport
where
    F: FnMut(&RetrievalQueryCase) -> O,
    O: Into<QueryExecutionOutcome>,
{
    let mut queries = suite.queries.clone();
    queries.sort_by(|a, b| a.id.cmp(&b.id));

    let mut per_query = Vec::with_capacity(queries.len());
    let mut recall_sum = 0.0;
    let mut mrr_sum = 0.0;
    let mut ndcg_sum = 0.0;
    let mut zero_count = 0usize;
    let mut degraded_count = 0usize;
    let mut budget_exhausted_count = 0usize;
    let mut clustering_sum = 0.0;
    let mut latencies = Vec::with_capacity(queries.len());
    let mut latency_by_intent_samples: BTreeMap<RetrievalIntent, Vec<f64>> = BTreeMap::new();

    for query in &queries {
        let started = Instant::now();
        let outcome = runner(query).into();
        let elapsed_ms = outcome
            .latency_ms
            .unwrap_or_else(|| started.elapsed().as_secs_f64() * 1000.0);

        let mut ranked_results = outcome.results;
        ranked_results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc_id().cmp(&b.doc_id()))
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.name.as_deref().cmp(&b.name.as_deref()))
                .then_with(|| {
                    a.qualified_name
                        .as_deref()
                        .cmp(&b.qualified_name.as_deref())
                })
                .then_with(|| a.signature.as_deref().cmp(&b.signature.as_deref()))
        });
        let top_results: Vec<RetrievalResult> = ranked_results.into_iter().take(limit).collect();
        let top_result_refs = top_results
            .iter()
            .enumerate()
            .map(|(idx, result)| TopResultEntry {
                doc_id: result.doc_id(),
                rank: idx + 1,
                score: result.score,
            })
            .collect::<Vec<_>>();

        let negative_hits = count_hits_against_targets(&top_results, &query.negative_targets);
        let (hit_rank, recall, ndcg, rr) = if negative_hits > 0 {
            (None, 0.0, 0.0, 0.0)
        } else {
            let hit_rank = first_hit_rank(&top_results, &query.expected_targets);
            let recall = recall_at_k(&top_results, &query.expected_targets);
            let ndcg = ndcg_at_k(&top_results, &query.expected_targets);
            let rr = hit_rank.map(|rank| 1.0 / rank as f64).unwrap_or(0.0);
            (hit_rank, recall, ndcg, rr)
        };
        let cluster_ratio = clustering_ratio(&top_results);

        if top_results.is_empty() {
            zero_count += 1;
        }
        if outcome.semantic_degraded {
            degraded_count += 1;
        }
        if outcome.semantic_budget_exhausted {
            budget_exhausted_count += 1;
        }

        recall_sum += recall;
        mrr_sum += rr;
        ndcg_sum += ndcg;
        clustering_sum += cluster_ratio;
        latencies.push(elapsed_ms);
        latency_by_intent_samples
            .entry(query.intent)
            .or_default()
            .push(elapsed_ms);

        per_query.push(QueryEvaluation {
            id: query.id.clone(),
            intent: query.intent,
            latency_ms: elapsed_ms,
            reciprocal_rank: rr,
            recall_at_k: recall,
            ndcg_at_k: ndcg,
            hit_rank,
            zero_results: top_results.is_empty(),
            cluster_ratio,
            semantic_degraded: outcome.semantic_degraded,
            semantic_budget_exhausted: outcome.semantic_budget_exhausted,
            negative_hits,
            top_results: top_result_refs,
        });
    }

    let total = per_query.len().max(1) as f64;
    let metrics = AggregateMetrics {
        recall_at_k: recall_sum / total,
        mrr: mrr_sum / total,
        ndcg_at_k: ndcg_sum / total,
        zero_result_rate: zero_count as f64 / total,
        clustering_ratio: clustering_sum / total,
        degraded_query_rate: degraded_count as f64 / total,
        semantic_budget_exhaustion_rate: budget_exhausted_count as f64 / total,
        latency_p50_ms: percentile(&latencies, 0.50),
        latency_p95_ms: percentile(&latencies, 0.95),
        latency_mean_ms: if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        },
    };

    let mut latency_by_intent = BTreeMap::new();
    for (intent, values) in latency_by_intent_samples {
        let mean = if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        };
        latency_by_intent.insert(
            intent,
            LatencySummary {
                sample_count: values.len(),
                p50_ms: percentile(&values, 0.5),
                p95_ms: percentile(&values, 0.95),
                mean_ms: mean,
            },
        );
    }

    EvaluationReport {
        version: "retrieval-eval-report-v1".to_string(),
        suite_version: suite.version.clone(),
        total_queries: per_query.len(),
        metrics,
        latency_by_intent,
        per_query,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteBaseline {
    pub version: String,
    pub suite_version: String,
    pub metrics: AggregateMetrics,
    #[serde(default)]
    pub latency_by_intent: BTreeMap<RetrievalIntent, LatencySummary>,
}

impl SuiteBaseline {
    pub fn from_metrics(
        suite_version: &str,
        recall_at_k: f64,
        mrr: f64,
        ndcg_at_k: f64,
        latency_p95_ms: f64,
    ) -> Self {
        Self {
            version: "retrieval-eval-baseline-v1".to_string(),
            suite_version: suite_version.to_string(),
            metrics: AggregateMetrics {
                recall_at_k,
                mrr,
                ndcg_at_k,
                zero_result_rate: 0.0,
                clustering_ratio: 0.0,
                degraded_query_rate: 0.0,
                semantic_budget_exhaustion_rate: 0.0,
                latency_p50_ms: 0.0,
                latency_p95_ms,
                latency_mean_ms: 0.0,
            },
            latency_by_intent: BTreeMap::new(),
        }
    }

    pub fn from_report(report: &EvaluationReport) -> Self {
        Self {
            version: "retrieval-eval-baseline-v1".to_string(),
            suite_version: report.suite_version.clone(),
            metrics: report.metrics.clone(),
            latency_by_intent: report.latency_by_intent.clone(),
        }
    }

    pub fn load_from_path(path: &Path) -> Result<Self, RetrievalEvalError> {
        let raw =
            std::fs::read_to_string(path).map_err(|source| RetrievalEvalError::ReadSuite {
                path: path.display().to_string(),
                source,
            })?;
        serde_json::from_str(&raw).map_err(|source| RetrievalEvalError::ParseBaseline {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn validate_compatibility(&self, suite: &RetrievalSuite) -> Result<(), RetrievalEvalError> {
        if self.suite_version != suite.version {
            return Err(RetrievalEvalError::Validation(format!(
                "baseline suite version '{}' does not match suite version '{}'",
                self.suite_version, suite.version
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatePolicy {
    pub version: String,
    pub quality: QualityPolicy,
    pub latency: LatencyPolicy,
    pub tolerance: TolerancePolicy,
}

impl GatePolicy {
    pub fn strict_defaults() -> Self {
        Self {
            version: "retrieval-gate-policy-v1".to_string(),
            quality: QualityPolicy {
                min_recall_at_k: 0.85,
                min_mrr: 0.80,
                min_ndcg_at_k: 0.80,
                max_zero_result_rate: 0.20,
                max_clustering_ratio: 0.90,
                max_degraded_query_rate: 0.40,
                enforce_degraded_query_rate: default_enforce_degraded_query_rate(),
            },
            latency: LatencyPolicy {
                max_p95_latency_ms: 500.0,
                max_p50_latency_ms: 250.0,
                by_intent: BTreeMap::from([
                    (
                        RetrievalIntent::Symbol,
                        IntentLatencyPolicy {
                            max_p50_latency_ms: 220.0,
                            max_p95_latency_ms: 450.0,
                        },
                    ),
                    (
                        RetrievalIntent::Path,
                        IntentLatencyPolicy {
                            max_p50_latency_ms: 220.0,
                            max_p95_latency_ms: 450.0,
                        },
                    ),
                    (
                        RetrievalIntent::Error,
                        IntentLatencyPolicy {
                            max_p50_latency_ms: 250.0,
                            max_p95_latency_ms: 500.0,
                        },
                    ),
                    (
                        RetrievalIntent::NaturalLanguage,
                        IntentLatencyPolicy {
                            max_p50_latency_ms: 300.0,
                            max_p95_latency_ms: 550.0,
                        },
                    ),
                ]),
            },
            tolerance: TolerancePolicy {
                recall_at_k: 0.01,
                mrr: 0.01,
                ndcg_at_k: 0.01,
                zero_result_rate: 0.02,
                clustering_ratio: 0.05,
                degraded_query_rate: 0.05,
                p50_latency_ms: 20.0,
                p95_latency_ms: 25.0,
            },
        }
    }

    pub fn load_from_path(path: &Path) -> Result<Self, RetrievalEvalError> {
        let raw =
            std::fs::read_to_string(path).map_err(|source| RetrievalEvalError::ReadSuite {
                path: path.display().to_string(),
                source,
            })?;
        serde_json::from_str(&raw).map_err(|source| RetrievalEvalError::ParsePolicy {
            path: path.display().to_string(),
            source,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityPolicy {
    pub min_recall_at_k: f64,
    pub min_mrr: f64,
    pub min_ndcg_at_k: f64,
    pub max_zero_result_rate: f64,
    pub max_clustering_ratio: f64,
    pub max_degraded_query_rate: f64,
    #[serde(default = "default_enforce_degraded_query_rate")]
    pub enforce_degraded_query_rate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentLatencyPolicy {
    pub max_p50_latency_ms: f64,
    pub max_p95_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPolicy {
    pub max_p95_latency_ms: f64,
    #[serde(default = "default_max_p50_latency_ms")]
    pub max_p50_latency_ms: f64,
    #[serde(default)]
    pub by_intent: BTreeMap<RetrievalIntent, IntentLatencyPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TolerancePolicy {
    pub recall_at_k: f64,
    pub mrr: f64,
    pub ndcg_at_k: f64,
    pub zero_result_rate: f64,
    pub clustering_ratio: f64,
    pub degraded_query_rate: f64,
    #[serde(default = "default_p50_latency_tolerance_ms")]
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
}

fn default_max_p50_latency_ms() -> f64 {
    500.0
}

fn default_p50_latency_tolerance_ms() -> f64 {
    25.0
}

fn default_enforce_degraded_query_rate() -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateCheck {
    pub metric: String,
    pub current: f64,
    pub expected: f64,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub verdict: GateVerdict,
    pub checks: Vec<GateCheck>,
    pub taxonomy: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalGateReport {
    pub report: EvaluationReport,
    pub gate: GateResult,
}

pub fn compare_against_baseline(
    report: &EvaluationReport,
    baseline: &SuiteBaseline,
    policy: &GatePolicy,
) -> GateResult {
    let mut checks = Vec::new();
    let mut taxonomy = BTreeSet::new();

    let recall_expected = baseline.metrics.recall_at_k - policy.tolerance.recall_at_k;
    let recall_floor = policy.quality.min_recall_at_k.max(recall_expected);
    let recall_passed = report.metrics.recall_at_k >= recall_floor;
    checks.push(GateCheck {
        metric: "recall_at_k".to_string(),
        current: report.metrics.recall_at_k,
        expected: recall_floor,
        passed: recall_passed,
    });
    if !recall_passed {
        taxonomy.insert("recall_drop".to_string());
    }

    let mrr_expected = baseline.metrics.mrr - policy.tolerance.mrr;
    let mrr_floor = policy.quality.min_mrr.max(mrr_expected);
    let mrr_passed = report.metrics.mrr >= mrr_floor;
    checks.push(GateCheck {
        metric: "mrr".to_string(),
        current: report.metrics.mrr,
        expected: mrr_floor,
        passed: mrr_passed,
    });
    if !mrr_passed {
        taxonomy.insert("ranking_shift".to_string());
    }

    let ndcg_expected = baseline.metrics.ndcg_at_k - policy.tolerance.ndcg_at_k;
    let ndcg_floor = policy.quality.min_ndcg_at_k.max(ndcg_expected);
    let ndcg_passed = report.metrics.ndcg_at_k >= ndcg_floor;
    checks.push(GateCheck {
        metric: "ndcg_at_k".to_string(),
        current: report.metrics.ndcg_at_k,
        expected: ndcg_floor,
        passed: ndcg_passed,
    });
    if !ndcg_passed {
        taxonomy.insert("ranking_shift".to_string());
    }

    let baseline_p50_cap = if baseline.metrics.latency_p50_ms > 0.0 {
        baseline.metrics.latency_p50_ms + policy.tolerance.p50_latency_ms
    } else {
        policy.latency.max_p50_latency_ms
    };
    let p50_cap = policy.latency.max_p50_latency_ms.min(baseline_p50_cap);
    let p50_passed = report.metrics.latency_p50_ms <= p50_cap;
    checks.push(GateCheck {
        metric: "latency_p50_ms".to_string(),
        current: report.metrics.latency_p50_ms,
        expected: p50_cap,
        passed: p50_passed,
    });
    if !p50_passed {
        taxonomy.insert("latency_regression".to_string());
    }

    let baseline_p95_cap = if baseline.metrics.latency_p95_ms > 0.0 {
        baseline.metrics.latency_p95_ms + policy.tolerance.p95_latency_ms
    } else {
        policy.latency.max_p95_latency_ms
    };
    let p95_cap = policy.latency.max_p95_latency_ms.min(baseline_p95_cap);
    let p95_passed = report.metrics.latency_p95_ms <= p95_cap;
    checks.push(GateCheck {
        metric: "latency_p95_ms".to_string(),
        current: report.metrics.latency_p95_ms,
        expected: p95_cap,
        passed: p95_passed,
    });
    if !p95_passed {
        taxonomy.insert("latency_regression".to_string());
    }

    for (intent, summary) in &report.latency_by_intent {
        let label = intent_label(*intent);
        let intent_policy = policy.latency.by_intent.get(intent);
        let policy_intent_p50_cap = intent_policy
            .map(|cfg| cfg.max_p50_latency_ms)
            .unwrap_or(policy.latency.max_p50_latency_ms);
        let policy_intent_p95_cap = intent_policy
            .map(|cfg| cfg.max_p95_latency_ms)
            .unwrap_or(policy.latency.max_p95_latency_ms);
        let baseline_intent = baseline.latency_by_intent.get(intent);
        let baseline_intent_p50_cap = baseline_intent
            .map(|value| value.p50_ms + policy.tolerance.p50_latency_ms)
            .unwrap_or(policy_intent_p50_cap);
        let baseline_intent_p95_cap = baseline_intent
            .map(|value| value.p95_ms + policy.tolerance.p95_latency_ms)
            .unwrap_or(policy_intent_p95_cap);

        let intent_p50_cap = policy_intent_p50_cap.min(baseline_intent_p50_cap);
        let intent_p95_cap = policy_intent_p95_cap.min(baseline_intent_p95_cap);
        let intent_p50_passed = summary.p50_ms <= intent_p50_cap;
        let intent_p95_passed = summary.p95_ms <= intent_p95_cap;

        checks.push(GateCheck {
            metric: format!("latency_p50_ms.{label}"),
            current: summary.p50_ms,
            expected: intent_p50_cap,
            passed: intent_p50_passed,
        });
        checks.push(GateCheck {
            metric: format!("latency_p95_ms.{label}"),
            current: summary.p95_ms,
            expected: intent_p95_cap,
            passed: intent_p95_passed,
        });

        if !intent_p50_passed || !intent_p95_passed {
            taxonomy.insert("latency_regression".to_string());
        }
    }

    let cluster_cap = policy
        .quality
        .max_clustering_ratio
        .min(baseline.metrics.clustering_ratio + policy.tolerance.clustering_ratio);
    let cluster_passed = report.metrics.clustering_ratio <= cluster_cap;
    checks.push(GateCheck {
        metric: "clustering_ratio".to_string(),
        current: report.metrics.clustering_ratio,
        expected: cluster_cap,
        passed: cluster_passed,
    });
    if !cluster_passed {
        taxonomy.insert("diversity_collapse".to_string());
    }

    let zero_cap = policy
        .quality
        .max_zero_result_rate
        .min(baseline.metrics.zero_result_rate + policy.tolerance.zero_result_rate);
    let zero_passed = report.metrics.zero_result_rate <= zero_cap;
    checks.push(GateCheck {
        metric: "zero_result_rate".to_string(),
        current: report.metrics.zero_result_rate,
        expected: zero_cap,
        passed: zero_passed,
    });
    if !zero_passed {
        taxonomy.insert("recall_drop".to_string());
    }

    let degraded_cap = policy
        .quality
        .max_degraded_query_rate
        .min(baseline.metrics.degraded_query_rate + policy.tolerance.degraded_query_rate);
    // NOTE: `semantic_degraded` in search metadata currently aliases fallback state
    // in the baseline pipeline. Keep this signal observe-only by default and allow
    // explicit opt-in policy enforcement when metadata semantics are trusted.
    let degraded_signal_is_trustworthy = policy.quality.enforce_degraded_query_rate;
    let degraded_passed = if degraded_signal_is_trustworthy {
        report.metrics.degraded_query_rate <= degraded_cap
    } else {
        true
    };
    checks.push(GateCheck {
        metric: "degraded_query_rate".to_string(),
        current: report.metrics.degraded_query_rate,
        expected: degraded_cap,
        passed: degraded_passed,
    });
    if degraded_signal_is_trustworthy && !degraded_passed {
        taxonomy.insert("semantic_degraded_spike".to_string());
    } else if !degraded_signal_is_trustworthy && report.metrics.degraded_query_rate > degraded_cap {
        taxonomy.insert("semantic_degraded_observe_only".to_string());
    }

    let verdict = if checks.iter().all(|check| check.passed) {
        GateVerdict::Pass
    } else {
        GateVerdict::Fail
    };

    GateResult {
        verdict,
        checks,
        taxonomy: taxonomy.into_iter().collect(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BeirQueryRecord {
    #[serde(rename = "_id")]
    id: String,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BeirCorpusRecord {
    #[serde(rename = "_id")]
    id: String,
    title: Option<String>,
    text: Option<String>,
}

pub fn load_beir_suite(
    corpus_path: &Path,
    queries_path: &Path,
    qrels_path: &Path,
    intent: RetrievalIntent,
) -> Result<RetrievalSuite, RetrievalEvalError> {
    let corpus_raw =
        std::fs::read_to_string(corpus_path).map_err(|source| RetrievalEvalError::ReadBeir {
            path: corpus_path.display().to_string(),
            source,
        })?;
    let mut corpus_ids = BTreeSet::new();
    for (idx, line) in corpus_raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<BeirCorpusRecord>(line).map_err(|err| {
            RetrievalEvalError::ParseBeirLine {
                path: corpus_path.display().to_string(),
                reason: format!("line {}: {err}", idx + 1),
            }
        })?;
        corpus_ids.insert(record.id);
    }

    let mut suite = load_beir_queries_and_qrels(queries_path, qrels_path, intent)?;
    for query in &mut suite.queries {
        query
            .expected_targets
            .retain(|target| corpus_ids.contains(&target.hint));
    }
    suite
        .queries
        .retain(|query| !query.expected_targets.is_empty());
    suite.validate()?;
    Ok(suite)
}

pub fn load_beir_queries_and_qrels(
    queries_path: &Path,
    qrels_path: &Path,
    intent: RetrievalIntent,
) -> Result<RetrievalSuite, RetrievalEvalError> {
    let queries_raw =
        std::fs::read_to_string(queries_path).map_err(|source| RetrievalEvalError::ReadBeir {
            path: queries_path.display().to_string(),
            source,
        })?;
    let qrels_raw =
        std::fs::read_to_string(qrels_path).map_err(|source| RetrievalEvalError::ReadBeir {
            path: qrels_path.display().to_string(),
            source,
        })?;

    let mut queries = Vec::<BeirQueryRecord>::new();
    for (idx, line) in queries_raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<BeirQueryRecord>(line).map_err(|err| {
            RetrievalEvalError::ParseBeirLine {
                path: queries_path.display().to_string(),
                reason: format!("line {}: {err}", idx + 1),
            }
        })?;
        queries.push(record);
    }

    let mut qrels: HashMap<String, Vec<ExpectedTarget>> = HashMap::new();
    for (idx, line) in qrels_raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let columns: Vec<&str> = trimmed.split_whitespace().collect();
        if idx == 0 && looks_like_qrels_header(&columns) {
            continue;
        }
        if columns.len() < 3 {
            return Err(RetrievalEvalError::ParseBeirLine {
                path: qrels_path.display().to_string(),
                reason: format!("line {} has <3 columns", idx + 1),
            });
        }
        let query_id = columns[0].to_string();
        let corpus_id = columns[1].to_string();
        let score = columns[2].parse::<f64>().unwrap_or(0.0);
        if score <= 0.0 {
            continue;
        }
        qrels
            .entry(query_id)
            .or_default()
            .push(ExpectedTarget { hint: corpus_id });
    }

    let mut cases = Vec::new();
    for query in queries {
        if let Some(expected_targets) = qrels.get(&query.id) {
            cases.push(RetrievalQueryCase {
                id: query.id,
                query: query.text,
                intent,
                expected_targets: expected_targets.clone(),
                negative_targets: Vec::new(),
            });
        }
    }

    let suite = RetrievalSuite {
        version: "beir-suite-v1".to_string(),
        queries: cases,
    };
    suite.validate()?;
    Ok(suite)
}

pub fn render_trec_run(report: &EvaluationReport) -> String {
    let mut lines = Vec::new();
    for query in &report.per_query {
        let qid = &query.id;
        for entry in &query.top_results {
            lines.push(format!(
                "{qid}\tQ0\t{}\t{}\t{:.6}\tcruxe",
                entry.doc_id, entry.rank, entry.score
            ));
        }
    }
    lines.join("\n") + "\n"
}

pub fn render_trec_qrels(suite: &RetrievalSuite) -> String {
    let mut lines = vec!["query-id\tcorpus-id\tscore".to_string()];
    for query in &suite.queries {
        for target in &query.expected_targets {
            lines.push(format!("{}\t{}\t1", query.id, target.hint));
        }
    }
    lines.join("\n") + "\n"
}

pub fn render_summary_table(report: &EvaluationReport, gate: &GateResult) -> String {
    let mut output = String::new();
    output.push_str("Retrieval Eval Summary\n");
    output.push_str("=====================\n");
    output.push_str(&format!(
        "suite={} total_queries={} verdict={:?}\n",
        report.suite_version, report.total_queries, gate.verdict
    ));
    output.push_str(&format!(
        "recall@k={:.4} mrr={:.4} ndcg@k={:.4} zero_rate={:.4} cluster_ratio={:.4} p50_ms={:.2} p95_ms={:.2}\n",
        report.metrics.recall_at_k,
        report.metrics.mrr,
        report.metrics.ndcg_at_k,
        report.metrics.zero_result_rate,
        report.metrics.clustering_ratio,
        report.metrics.latency_p50_ms,
        report.metrics.latency_p95_ms
    ));
    if !gate.taxonomy.is_empty() {
        output.push_str("taxonomy=");
        output.push_str(&gate.taxonomy.join(","));
        output.push('\n');
    }
    output.push_str("\nChecks:\n");
    for check in &gate.checks {
        output.push_str(&format!(
            "- {:<22} current={:<8.4} expected={:<8.4} {}\n",
            check.metric,
            check.current,
            check.expected,
            if check.passed { "PASS" } else { "FAIL" }
        ));
    }
    output
}

fn intent_label(intent: RetrievalIntent) -> &'static str {
    match intent {
        RetrievalIntent::Symbol => "symbol",
        RetrievalIntent::Path => "path",
        RetrievalIntent::Error => "error",
        RetrievalIntent::NaturalLanguage => "natural_language",
    }
}

fn first_hit_rank(
    results: &[RetrievalResult],
    expected_targets: &[ExpectedTarget],
) -> Option<usize> {
    let expected: Vec<String> = expected_targets
        .iter()
        .map(|target| target.hint.to_ascii_lowercase())
        .collect();
    for (idx, result) in results.iter().enumerate() {
        if matches_any_expected(result, &expected) {
            return Some(idx + 1);
        }
    }
    None
}

fn recall_at_k(results: &[RetrievalResult], expected_targets: &[ExpectedTarget]) -> f64 {
    if expected_targets.is_empty() {
        return 0.0;
    }

    let expected: Vec<String> = expected_targets
        .iter()
        .map(|target| target.hint.to_ascii_lowercase())
        .collect();
    let mut matched = BTreeSet::new();

    for result in results {
        for target in &expected {
            if result_matches_target(result, target) {
                matched.insert(target.clone());
            }
        }
    }

    matched.len() as f64 / expected.len() as f64
}

fn ndcg_at_k(results: &[RetrievalResult], expected_targets: &[ExpectedTarget]) -> f64 {
    if results.is_empty() || expected_targets.is_empty() {
        return 0.0;
    }

    let expected: Vec<String> = expected_targets
        .iter()
        .map(|target| target.hint.to_ascii_lowercase())
        .collect();

    let mut dcg = 0.0;
    let mut matched = BTreeSet::new();
    for (idx, result) in results.iter().enumerate() {
        let mut gained = false;
        for target in &expected {
            if matched.contains(target) {
                continue;
            }
            if result_matches_target(result, target) {
                matched.insert(target.clone());
                gained = true;
                break;
            }
        }
        if gained {
            dcg += 1.0 / ((idx as f64 + 2.0).log2());
        }
    }

    let ideal_hits = expected.len().min(results.len());
    if ideal_hits == 0 {
        return 0.0;
    }

    let mut idcg = 0.0;
    for idx in 0..ideal_hits {
        idcg += 1.0 / ((idx as f64 + 2.0).log2());
    }

    if idcg <= f64::EPSILON {
        0.0
    } else {
        dcg / idcg
    }
}

fn clustering_ratio(results: &[RetrievalResult]) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    let mut counts = HashMap::<&str, usize>::new();
    for result in results {
        *counts.entry(result.path.as_str()).or_default() += 1;
    }
    let max_count = counts.values().copied().max().unwrap_or(0);
    max_count as f64 / results.len() as f64
}

fn matches_any_expected(result: &RetrievalResult, expected: &[String]) -> bool {
    expected
        .iter()
        .any(|target| result_matches_target(result, target))
}

fn count_hits_against_targets(results: &[RetrievalResult], targets: &[ExpectedTarget]) -> usize {
    if targets.is_empty() {
        return 0;
    }
    let lowered_targets: Vec<String> = targets
        .iter()
        .map(|target| target.hint.to_ascii_lowercase())
        .collect();
    results
        .iter()
        .filter(|result| matches_any_expected(result, &lowered_targets))
        .count()
}

fn looks_like_qrels_header(columns: &[&str]) -> bool {
    if columns.len() < 2 {
        return false;
    }
    let first = columns[0].trim().to_ascii_lowercase();
    let second = columns[1].trim().to_ascii_lowercase();
    let first_is_header = matches!(
        first.as_str(),
        "query-id" | "query_id" | "queryid" | "qid" | "query"
    );
    let second_is_header = matches!(
        second.as_str(),
        "corpus-id"
            | "corpus_id"
            | "corpusid"
            | "doc-id"
            | "docid"
            | "document-id"
            | "did"
            | "corpus"
    );
    first_is_header && second_is_header
}

fn result_matches_target(result: &RetrievalResult, target: &str) -> bool {
    let mut candidates = Vec::new();
    candidates.push(result.path.to_ascii_lowercase());
    if let Some(name) = &result.name {
        candidates.push(name.to_ascii_lowercase());
    }
    if let Some(name) = &result.qualified_name {
        candidates.push(name.to_ascii_lowercase());
    }
    if let Some(signature) = &result.signature {
        candidates.push(signature.to_ascii_lowercase());
    }
    if let Some(symbol_id) = &result.symbol_stable_id {
        candidates.push(symbol_id.to_ascii_lowercase());
    }

    candidates
        .iter()
        .any(|candidate| candidate.contains(target))
}

pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let rank = (p.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_validation_catches_duplicate_ids() {
        let suite = RetrievalSuite {
            version: "v1".to_string(),
            queries: vec![
                RetrievalQueryCase {
                    id: "q1".to_string(),
                    query: "foo".to_string(),
                    intent: RetrievalIntent::NaturalLanguage,
                    expected_targets: vec![ExpectedTarget {
                        hint: "x.rs".to_string(),
                    }],
                    negative_targets: vec![],
                },
                RetrievalQueryCase {
                    id: "q1".to_string(),
                    query: "bar".to_string(),
                    intent: RetrievalIntent::NaturalLanguage,
                    expected_targets: vec![ExpectedTarget {
                        hint: "y.rs".to_string(),
                    }],
                    negative_targets: vec![],
                },
            ],
        };

        let err = suite.validate().expect_err("duplicate id should fail");
        assert!(err.to_string().contains("duplicate query id"));
    }

    #[test]
    fn trec_renderers_produce_expected_shape() {
        let suite = RetrievalSuite {
            version: "v1".to_string(),
            queries: vec![RetrievalQueryCase {
                id: "q1".to_string(),
                query: "foo".to_string(),
                intent: RetrievalIntent::NaturalLanguage,
                expected_targets: vec![ExpectedTarget {
                    hint: "docA".to_string(),
                }],
                negative_targets: vec![],
            }],
        };

        let report = EvaluationReport {
            version: "retrieval-eval-report-v1".to_string(),
            suite_version: "v1".to_string(),
            total_queries: 1,
            metrics: AggregateMetrics {
                recall_at_k: 1.0,
                mrr: 1.0,
                ndcg_at_k: 1.0,
                zero_result_rate: 0.0,
                clustering_ratio: 1.0,
                degraded_query_rate: 0.0,
                semantic_budget_exhaustion_rate: 0.0,
                latency_p50_ms: 1.0,
                latency_p95_ms: 1.0,
                latency_mean_ms: 1.0,
            },
            latency_by_intent: BTreeMap::new(),
            per_query: vec![QueryEvaluation {
                id: "q1".to_string(),
                intent: RetrievalIntent::NaturalLanguage,
                latency_ms: 1.0,
                reciprocal_rank: 1.0,
                recall_at_k: 1.0,
                ndcg_at_k: 1.0,
                hit_rank: Some(1),
                zero_results: false,
                cluster_ratio: 1.0,
                semantic_degraded: false,
                semantic_budget_exhausted: false,
                negative_hits: 0,
                top_results: vec![TopResultEntry {
                    doc_id: "docA".to_string(),
                    rank: 1,
                    score: 0.9,
                }],
            }],
        };

        let run = render_trec_run(&report);
        assert!(run.contains("q1\tQ0\tdocA\t1\t0.900000\tcruxe"));

        let qrels = render_trec_qrels(&suite);
        assert!(qrels.contains("query-id\tcorpus-id\tscore"));
        assert!(qrels.contains("q1\tdocA\t1"));
    }

    #[test]
    fn gate_report_json_contains_gate_taxonomy() {
        let report = EvaluationReport {
            version: "retrieval-eval-report-v1".to_string(),
            suite_version: "retrieval-eval-suite-v1".to_string(),
            total_queries: 1,
            metrics: AggregateMetrics {
                recall_at_k: 1.0,
                mrr: 1.0,
                ndcg_at_k: 1.0,
                zero_result_rate: 0.0,
                clustering_ratio: 1.0,
                degraded_query_rate: 0.0,
                semantic_budget_exhaustion_rate: 0.0,
                latency_p50_ms: 1.0,
                latency_p95_ms: 1.0,
                latency_mean_ms: 1.0,
            },
            latency_by_intent: BTreeMap::new(),
            per_query: Vec::new(),
        };
        let gate = GateResult {
            verdict: GateVerdict::Fail,
            checks: vec![GateCheck {
                metric: "recall_at_k".to_string(),
                current: 0.1,
                expected: 0.9,
                passed: false,
            }],
            taxonomy: vec!["recall_drop".to_string()],
        };
        let gate_report = RetrievalGateReport { report, gate };
        let value = serde_json::to_value(gate_report).expect("serialize gate report");

        assert!(value.get("report").is_some());
        assert!(value.get("gate").is_some());
        assert_eq!(
            value["gate"]["taxonomy"]
                .as_array()
                .expect("taxonomy array")
                .len(),
            1
        );
    }

    #[test]
    fn negative_targets_zero_out_positive_scoring_when_hit() {
        let suite = RetrievalSuite {
            version: "v1".to_string(),
            queries: vec![RetrievalQueryCase {
                id: "q1".to_string(),
                query: "auth".to_string(),
                intent: RetrievalIntent::NaturalLanguage,
                expected_targets: vec![ExpectedTarget {
                    hint: "auth.rs".to_string(),
                }],
                negative_targets: vec![ExpectedTarget {
                    hint: "config.rs".to_string(),
                }],
            }],
        };

        let report = evaluate_with_runner(&suite, 5, |_| {
            vec![
                RetrievalResult::new("src/config.rs", Some("load_config"), 1.0),
                RetrievalResult::new("src/auth.rs", Some("authenticate"), 0.9),
            ]
        });

        assert_eq!(report.per_query.len(), 1);
        let evaluation = &report.per_query[0];
        assert_eq!(evaluation.negative_hits, 1);
        assert_eq!(evaluation.hit_rank, None);
        assert_eq!(evaluation.reciprocal_rank, 0.0);
        assert_eq!(evaluation.recall_at_k, 0.0);
        assert_eq!(evaluation.ndcg_at_k, 0.0);
    }

    #[test]
    fn beir_qrels_first_row_with_query_token_is_not_treated_as_header() {
        let tmp = tempfile::tempdir().unwrap();
        let queries_path = tmp.path().join("queries.jsonl");
        let qrels_path = tmp.path().join("qrels.tsv");

        std::fs::write(
            &queries_path,
            "{\"_id\":\"query123\",\"text\":\"auth flow\"}\n",
        )
        .unwrap();
        std::fs::write(&qrels_path, "query123\tauth_doc\t1\n").unwrap();

        let suite = load_beir_queries_and_qrels(
            &queries_path,
            &qrels_path,
            RetrievalIntent::NaturalLanguage,
        )
        .unwrap();
        assert_eq!(suite.queries.len(), 1);
        assert_eq!(suite.queries[0].id, "query123");
        assert_eq!(suite.queries[0].expected_targets[0].hint, "auth_doc");
    }
}
