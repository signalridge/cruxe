use cruxe_query::retrieval_eval::{
    ExpectedTarget, GatePolicy, GateVerdict, QueryExecutionOutcome, RetrievalIntent,
    RetrievalQueryCase, RetrievalResult, RetrievalSuite, SuiteBaseline, compare_against_baseline,
    evaluate_with_runner, load_beir_queries_and_qrels, percentile,
};

fn sample_suite() -> RetrievalSuite {
    RetrievalSuite {
        version: "retrieval-eval-suite-v1".to_string(),
        queries: vec![
            RetrievalQueryCase {
                id: "q-symbol-1".to_string(),
                query: "AuthHandler".to_string(),
                intent: RetrievalIntent::Symbol,
                expected_targets: vec![ExpectedTarget {
                    hint: "handler.rs".to_string(),
                }],
                negative_targets: vec![],
            },
            RetrievalQueryCase {
                id: "q-nl-1".to_string(),
                query: "where is request auth validated".to_string(),
                intent: RetrievalIntent::NaturalLanguage,
                expected_targets: vec![ExpectedTarget {
                    hint: "auth.rs".to_string(),
                }],
                negative_targets: vec![],
            },
        ],
    }
}

#[test]
fn percentile_is_deterministic() {
    let values = vec![10.0, 5.0, 8.0, 1.0, 6.0];
    assert!((percentile(&values, 0.95) - 10.0).abs() < f64::EPSILON);
    assert!((percentile(&values, 0.50) - 6.0).abs() < f64::EPSILON);
}

#[test]
fn evaluate_with_runner_produces_metrics_and_bucket_latencies() {
    let suite = sample_suite();
    let report = evaluate_with_runner(&suite, 5, |query| {
        let first = if query.id == "q-symbol-1" {
            RetrievalResult::new("src/auth/handler.rs", Some("AuthHandler"), 0.98)
        } else {
            RetrievalResult::new("src/auth/auth.rs", Some("validate_token"), 0.88)
        };
        vec![first]
    });

    assert_eq!(report.total_queries, 2);
    assert!(report.metrics.recall_at_k >= 1.0);
    assert!(report.metrics.mrr >= 1.0);
    assert!(report.metrics.ndcg_at_k <= 1.0);
    assert!(report.metrics.ndcg_at_k > 0.5);
    assert!(
        report
            .latency_by_intent
            .contains_key(&RetrievalIntent::Symbol)
    );
}

#[test]
fn compare_against_baseline_flags_regressions_with_taxonomy() {
    let suite = sample_suite();
    let report = evaluate_with_runner(&suite, 5, |_query| {
        vec![RetrievalResult::new(
            "src/irrelevant.rs",
            Some("noop"),
            0.01,
        )]
    });

    let baseline = SuiteBaseline::from_metrics("retrieval-eval-suite-v1", 0.95, 0.90, 0.92, 20.0);
    let policy = GatePolicy::strict_defaults();

    let gate = compare_against_baseline(&report, &baseline, &policy);
    assert_eq!(gate.verdict, GateVerdict::Fail);
    assert!(gate.taxonomy.iter().any(|t| t == "recall_drop"));
    assert!(gate.taxonomy.iter().any(|t| t == "ranking_shift"));
}

#[test]
fn beir_loader_reads_queries_and_qrels() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("retrieval")
        .join("beir-sample");
    let loaded = load_beir_queries_and_qrels(
        &root.join("queries.jsonl"),
        &root.join("qrels.tsv"),
        RetrievalIntent::NaturalLanguage,
    )
    .expect("load beir sample");

    assert_eq!(loaded.queries.len(), 2);
    assert!(
        loaded
            .queries
            .iter()
            .all(|q| !q.expected_targets.is_empty())
    );
}

#[test]
fn suite_loader_rejects_missing_intent_field() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let suite_path = tmp.path().join("suite-missing-intent.json");
    std::fs::write(
        &suite_path,
        r#"{
            "version":"retrieval-eval-suite-v1",
            "queries":[
                {"id":"q1","query":"AuthHandler","expected_targets":[{"hint":"handler.rs"}]}
            ]
        }"#,
    )
    .expect("write suite fixture");

    let err = RetrievalSuite::load_from_path(&suite_path).expect_err("missing intent must fail");
    assert!(err.to_string().contains("intent"));
}

#[test]
fn baseline_compatibility_rejects_suite_version_mismatch() {
    let suite = sample_suite();
    let baseline = SuiteBaseline::from_metrics("other-suite-v1", 0.9, 0.9, 0.9, 10.0);
    let err = baseline
        .validate_compatibility(&suite)
        .expect_err("suite version mismatch should fail");
    assert!(err.to_string().contains("does not match"));
}

#[test]
fn from_metrics_populates_all_latency_fields_with_p95_fallback() {
    let baseline = SuiteBaseline::from_metrics("retrieval-eval-suite-v1", 0.9, 0.8, 0.85, 42.0);
    assert!((baseline.metrics.latency_p50_ms - 42.0).abs() < 1e-9);
    assert!((baseline.metrics.latency_p95_ms - 42.0).abs() < 1e-9);
    assert!((baseline.metrics.latency_mean_ms - 42.0).abs() < 1e-9);
}

#[test]
fn from_metrics_with_latency_preserves_explicit_distribution_values() {
    let baseline = SuiteBaseline::from_metrics_with_latency(
        "retrieval-eval-suite-v1",
        0.9,
        0.8,
        0.85,
        12.0,
        42.0,
        20.0,
    );
    assert!((baseline.metrics.latency_p50_ms - 12.0).abs() < 1e-9);
    assert!((baseline.metrics.latency_p95_ms - 42.0).abs() < 1e-9);
    assert!((baseline.metrics.latency_mean_ms - 20.0).abs() < 1e-9);
}

#[test]
fn compare_against_baseline_flags_intent_latency_regressions() {
    let suite = sample_suite();
    let baseline_report = evaluate_with_runner(&suite, 5, |query| {
        let first = if query.id == "q-symbol-1" {
            RetrievalResult::new("src/auth/handler.rs", Some("AuthHandler"), 0.98)
        } else {
            RetrievalResult::new("src/auth/auth.rs", Some("validate_token"), 0.88)
        };
        QueryExecutionOutcome::from(vec![first]).with_latency(12.0)
    });
    let baseline = SuiteBaseline::from_report(&baseline_report);
    let policy = GatePolicy::strict_defaults();

    let regressed_report = evaluate_with_runner(&suite, 5, |query| {
        let first = if query.id == "q-symbol-1" {
            RetrievalResult::new("src/auth/handler.rs", Some("AuthHandler"), 0.98)
        } else {
            RetrievalResult::new("src/auth/auth.rs", Some("validate_token"), 0.88)
        };
        let latency_ms = match query.intent {
            RetrievalIntent::Symbol => 900.0,
            RetrievalIntent::NaturalLanguage => 880.0,
            RetrievalIntent::Path | RetrievalIntent::Error => 850.0,
        };
        QueryExecutionOutcome::from(vec![first]).with_latency(latency_ms)
    });

    let gate = compare_against_baseline(&regressed_report, &baseline, &policy);
    assert_eq!(gate.verdict, GateVerdict::Fail);
    assert!(gate.taxonomy.iter().any(|t| t == "latency_regression"));
    assert!(
        gate.checks
            .iter()
            .any(|check| check.metric == "latency_p95_ms.symbol" && !check.passed)
    );
}
