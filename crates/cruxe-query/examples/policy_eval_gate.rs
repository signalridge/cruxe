use cruxe_core::config::SearchConfig as CoreSearchConfig;
use cruxe_core::types::PolicyMode;
use cruxe_query::policy::PolicyRuntime;
use cruxe_query::search::SearchResult;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ModeSummary {
    mode: String,
    emitted: usize,
    blocked: usize,
    redacted: usize,
    warnings: usize,
    sample_outputs: Vec<SampleOutput>,
}

#[derive(Debug, Serialize)]
struct SampleOutput {
    path: String,
    snippet: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let corpus = sample_results();
    let mut config = CoreSearchConfig::default();
    config.policy.allow_request_override = true;
    config.policy.path.deny = vec!["**/secrets/**".to_string()];
    config.policy.detect_secrets.enabled = true;
    config.policy.detect_secrets.plugins = vec!["github".to_string(), "aws".to_string()];

    let mut summaries = Vec::new();
    for mode in [
        PolicyMode::Off,
        PolicyMode::AuditOnly,
        PolicyMode::Balanced,
        PolicyMode::Strict,
    ] {
        config.policy.mode = mode.as_str().to_string();
        let runtime = PolicyRuntime::from_search_config(&config, None)?;
        let applied = runtime.apply(corpus.clone())?;
        summaries.push(ModeSummary {
            mode: mode.as_str().to_string(),
            emitted: applied.results.len(),
            blocked: applied.blocked_count,
            redacted: applied.redacted_count,
            warnings: applied.warnings.len(),
            sample_outputs: applied
                .results
                .iter()
                .take(2)
                .map(|result| SampleOutput {
                    path: result.path.clone(),
                    snippet: result.snippet.clone(),
                })
                .collect(),
        });
    }

    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

fn sample_results() -> Vec<SearchResult> {
    let aws_fixture_token = format!("{}{}", "AKIA", "ABCDEFGHIJKLMNOP");
    vec![
        SearchResult {
            repo: "repo".to_string(),
            result_id: "res-1".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "snippet".to_string(),
            path: "src/auth.rs".to_string(),
            line_start: 10,
            line_end: 22,
            kind: Some("function".to_string()),
            name: Some("authenticate".to_string()),
            qualified_name: Some("authenticate".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: Some("pub".to_string()),
            score: 1.0,
            snippet: Some("fn authenticate() -> Result<()> { Ok(()) }".to_string()),
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        },
        SearchResult {
            repo: "repo".to_string(),
            result_id: "res-2".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "snippet".to_string(),
            path: "src/secrets/keys.rs".to_string(),
            line_start: 3,
            line_end: 3,
            kind: Some("constant".to_string()),
            name: Some("API_KEY".to_string()),
            qualified_name: Some("API_KEY".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: Some("private".to_string()),
            score: 0.9,
            snippet: Some("const API_KEY: &str = \"ghp_abcdefghijklmnopqrstuvwxyz\";".to_string()),
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        },
        SearchResult {
            repo: "repo".to_string(),
            result_id: "res-3".to_string(),
            symbol_id: None,
            symbol_stable_id: None,
            result_type: "snippet".to_string(),
            path: "src/notify.rs".to_string(),
            line_start: 8,
            line_end: 12,
            kind: Some("function".to_string()),
            name: Some("notify".to_string()),
            qualified_name: Some("notify".to_string()),
            language: "rust".to_string(),
            signature: None,
            visibility: Some("pub".to_string()),
            score: 0.8,
            snippet: Some(format!(
                "send_email(\"security@example.com\", \"{}\")",
                aws_fixture_token
            )),
            chunk_type: None,
            source_layer: None,
            provenance: "lexical".to_string(),
        },
    ]
}
