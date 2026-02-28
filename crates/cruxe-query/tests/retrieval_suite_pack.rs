use std::collections::BTreeMap;

#[test]
fn retrieval_query_pack_is_versioned_and_intent_covered() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("retrieval")
        .join("query-pack.v1.json");

    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));

    assert_eq!(
        value["version"].as_str(),
        Some("retrieval-eval-suite-v1"),
        "query pack version mismatch"
    );

    let queries = value["queries"]
        .as_array()
        .expect("queries must be an array");
    assert!(queries.len() >= 8, "expected at least 8 queries");

    let mut counts = BTreeMap::<String, usize>::new();
    for query in queries {
        let intent = query["intent"]
            .as_str()
            .expect("query.intent must be a string");
        *counts.entry(intent.to_string()).or_default() += 1;

        let expected = query["expected_targets"]
            .as_array()
            .expect("expected_targets must be an array");
        assert!(
            !expected.is_empty(),
            "each query must include at least one expected target"
        );
    }

    for intent in ["symbol", "path", "error", "natural_language"] {
        let count = counts.get(intent).copied().unwrap_or_default();
        assert!(
            count >= 2,
            "intent {intent} should have >=2 queries, got {count}"
        );
    }
}

#[test]
fn retrieval_baseline_and_policy_are_parseable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("retrieval");

    let baseline_raw = std::fs::read_to_string(root.join("baseline.v1.json"))
        .expect("baseline should be readable");
    let baseline: serde_json::Value =
        serde_json::from_str(&baseline_raw).expect("baseline should be valid JSON");
    assert_eq!(
        baseline["version"].as_str(),
        Some("retrieval-eval-baseline-v1")
    );
    assert!(
        baseline["metrics"]["latency_p50_ms"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "baseline latency_p50_ms should be present"
    );
    assert!(
        baseline["latency_by_intent"]["natural_language"]["p95_ms"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "baseline intent latency summary should be present"
    );

    let policy_raw = std::fs::read_to_string(root.join("gate-policy.v1.json"))
        .expect("policy should be readable");
    let policy: serde_json::Value =
        serde_json::from_str(&policy_raw).expect("policy should be valid JSON");
    assert_eq!(policy["version"].as_str(), Some("retrieval-gate-policy-v1"));
    assert!(
        policy["quality"]["min_recall_at_k"].as_f64().unwrap_or(0.0) > 0.0,
        "policy quality min_recall_at_k should be > 0"
    );
    assert!(
        policy["latency"]["max_p50_latency_ms"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "policy latency max_p50_latency_ms should be > 0"
    );
    assert!(
        policy["latency"]["by_intent"]["symbol"]["max_p95_latency_ms"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "policy by_intent latency thresholds should be present"
    );
}
