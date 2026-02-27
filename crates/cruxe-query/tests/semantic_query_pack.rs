use std::collections::BTreeMap;

#[test]
fn query_pack_is_stratified_and_versioned() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("benchmarks")
        .join("semantic")
        .join("query-pack.v1.json");

    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));

    assert_eq!(
        value["version"].as_str(),
        Some("semantic-query-pack-v1"),
        "query pack version mismatch"
    );

    let queries = value["queries"]
        .as_array()
        .expect("queries must be an array");
    assert!(queries.len() >= 100, "expected at least 100 queries");

    let mut counts = BTreeMap::<String, usize>::new();
    for query in queries {
        let language = query["language"]
            .as_str()
            .expect("query.language must be a string");
        *counts.entry(language.to_string()).or_default() += 1;
    }

    for language in ["rust", "typescript", "python", "go"] {
        let count = counts.get(language).copied().unwrap_or_default();
        assert!(
            count >= 20,
            "language {language} should have >= 20 queries, got {count}"
        );
    }
}
