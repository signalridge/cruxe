use cruxe_core::config::SemanticConfig;
use cruxe_core::error::StateError;
use cruxe_core::types::RerankResult;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const COHERE_RERANK_ENDPOINT: &str = "https://api.cohere.com/v2/rerank";
const VOYAGE_RERANK_ENDPOINT: &str = "https://api.voyageai.com/v1/rerank";
const MAX_RERANK_CLIENT_CACHE_ENTRIES: usize = 8;

static RERANK_HTTP_CLIENT_CACHE: OnceLock<Mutex<HashMap<u64, Client>>> = OnceLock::new();
#[cfg(test)]
static TEST_RERANK_API_KEY_OVERRIDE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct RerankDocument {
    pub result_id: String,
    pub text: String,
    pub base_score: f64,
}

#[derive(Debug, Clone)]
pub struct RerankExecution {
    pub reranked: Vec<RerankResult>,
    pub provider: String,
    pub fallback: bool,
    pub fallback_reason: Option<String>,
    pub external_provider_blocked: bool,
}

impl Default for RerankExecution {
    fn default() -> Self {
        Self {
            reranked: Vec::new(),
            provider: "local".to_string(),
            fallback: false,
            fallback_reason: None,
            external_provider_blocked: false,
        }
    }
}

pub(crate) trait Rerank: Send + Sync {
    fn rerank(
        &self,
        query: &str,
        docs: &[RerankDocument],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, StateError>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalRuleReranker {
    phrase_boost: f64,
    token_overlap_weight: f64,
}

impl Default for LocalRuleReranker {
    fn default() -> Self {
        let semantic = SemanticConfig::default();
        Self::from_semantic(&semantic)
    }
}

impl LocalRuleReranker {
    fn from_semantic(semantic: &SemanticConfig) -> Self {
        Self {
            phrase_boost: semantic.local_rerank_phrase_boost,
            token_overlap_weight: semantic.local_rerank_token_overlap_weight,
        }
    }
}

impl Rerank for LocalRuleReranker {
    fn rerank(
        &self,
        query: &str,
        docs: &[RerankDocument],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, StateError> {
        if docs.is_empty() || top_n == 0 {
            return Ok(Vec::new());
        }

        let normalized_query = query.trim().to_ascii_lowercase();
        let query_tokens = tokenize(&normalized_query);

        let mut scored: Vec<RerankResult> = docs
            .iter()
            .map(|doc| {
                let text_lower = doc.text.to_ascii_lowercase();
                let overlap = token_overlap_ratio(&query_tokens, &text_lower);
                let phrase_boost =
                    if !normalized_query.is_empty() && text_lower.contains(&normalized_query) {
                        self.phrase_boost
                    } else {
                        0.0
                    };
                let score = doc.base_score + overlap * self.token_overlap_weight + phrase_boost;
                RerankResult {
                    doc: doc.result_id.clone(),
                    score,
                    provider: "local".to_string(),
                }
            })
            .collect();

        scored.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.doc.cmp(&right.doc))
        });
        scored.truncate(top_n.min(docs.len()));

        Ok(scored)
    }
}

struct ExternalRerankProvider {
    provider: String,
    endpoint: String,
    timeout: Duration,
}

impl ExternalRerankProvider {
    fn from_config(provider: &str, semantic: &SemanticConfig) -> Self {
        let endpoint = semantic
            .rerank
            .endpoint
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| match provider {
                "voyage" => VOYAGE_RERANK_ENDPOINT.to_string(),
                _ => COHERE_RERANK_ENDPOINT.to_string(),
            });
        Self {
            provider: provider.to_string(),
            endpoint,
            timeout: Duration::from_millis(semantic.rerank.timeout_ms.max(1)),
        }
    }

    fn rerank(
        &self,
        query: &str,
        docs: &[RerankDocument],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, StateError> {
        if docs.is_empty() || top_n == 0 {
            return Ok(Vec::new());
        }

        let api_key = resolve_rerank_api_key()?;
        let client = shared_rerank_http_client(self.timeout)?;

        let payload = build_external_payload(&self.provider, query, docs, top_n);
        let response = client
            .post(&self.endpoint)
            .bearer_auth(api_key)
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .map_err(StateError::external)?;

        if !response.status().is_success() {
            return Err(StateError::external(format!(
                "{}_http_{}",
                self.provider,
                response.status().as_u16()
            )));
        }

        let body: serde_json::Value = response.json().map_err(StateError::external)?;
        let mut reranked = match self.provider.as_str() {
            "voyage" => parse_voyage_response(body, docs),
            _ => parse_cohere_response(body, docs),
        };

        if reranked.is_empty() {
            return Err(StateError::external(format!(
                "{}_empty_response",
                self.provider
            )));
        }

        reranked.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.doc.cmp(&right.doc))
        });
        reranked.truncate(top_n.min(docs.len()));

        Ok(reranked)
    }
}

fn resolve_rerank_api_key() -> Result<String, StateError> {
    #[cfg(test)]
    {
        if let Ok(guard) = test_rerank_api_key_override().lock()
            && let Some(value) = guard.as_ref()
        {
            return Ok(value.clone());
        }
    }

    std::env::var("CRUXE_RERANK_API_KEY")
        .map_err(|_| StateError::external("missing_rerank_api_key"))
}

#[cfg(test)]
fn test_rerank_api_key_override() -> &'static Mutex<Option<String>> {
    TEST_RERANK_API_KEY_OVERRIDE.get_or_init(|| Mutex::new(None))
}

fn shared_rerank_http_client(timeout: Duration) -> Result<Client, StateError> {
    let timeout_ms = timeout.as_millis() as u64;
    cruxe_core::cache::get_or_insert_cached(
        &RERANK_HTTP_CLIENT_CACHE,
        timeout_ms,
        MAX_RERANK_CLIENT_CACHE_ENTRIES,
        || {
            Client::builder()
                .timeout(timeout)
                .build()
                .map_err(StateError::external)
        },
    )
}

pub fn rerank_documents(
    query: &str,
    docs: &[RerankDocument],
    semantic: &SemanticConfig,
    top_n: usize,
) -> RerankExecution {
    let requested_provider = normalize_provider(&semantic.rerank.provider);
    let normalized_top_n = top_n.max(1);

    if docs.is_empty() {
        return RerankExecution {
            reranked: Vec::new(),
            provider: "local".to_string(),
            fallback: false,
            fallback_reason: None,
            external_provider_blocked: false,
        };
    }

    if matches!(requested_provider.as_str(), "none" | "local") {
        return local_execution(query, docs, semantic, normalized_top_n, false, None, false);
    }

    if !semantic.allow_external_provider_calls() {
        return local_execution(
            query,
            docs,
            semantic,
            normalized_top_n,
            true,
            Some("external_provider_blocked".to_string()),
            true,
        );
    }

    let external = ExternalRerankProvider::from_config(&requested_provider, semantic);
    match external.rerank(query, docs, normalized_top_n) {
        Ok(reranked) => RerankExecution {
            reranked,
            provider: requested_provider,
            fallback: false,
            fallback_reason: None,
            external_provider_blocked: false,
        },
        Err(err) => {
            let reason = classify_external_fallback_reason(&requested_provider, &err);
            local_execution(
                query,
                docs,
                semantic,
                normalized_top_n,
                true,
                Some(reason),
                false,
            )
        }
    }
}

fn local_execution(
    query: &str,
    docs: &[RerankDocument],
    semantic: &SemanticConfig,
    top_n: usize,
    fallback: bool,
    fallback_reason: Option<String>,
    external_provider_blocked: bool,
) -> RerankExecution {
    let local = LocalRuleReranker::from_semantic(semantic);
    let reranked = local
        .rerank(query, docs, top_n)
        .unwrap_or_else(|_| deterministic_base_scores(docs, top_n));

    RerankExecution {
        reranked,
        provider: "local".to_string(),
        fallback,
        fallback_reason,
        external_provider_blocked,
    }
}

fn deterministic_base_scores(docs: &[RerankDocument], top_n: usize) -> Vec<RerankResult> {
    let mut reranked: Vec<RerankResult> = docs
        .iter()
        .map(|doc| RerankResult {
            doc: doc.result_id.clone(),
            score: doc.base_score,
            provider: "local".to_string(),
        })
        .collect();

    reranked.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.doc.cmp(&right.doc))
    });
    reranked.truncate(top_n.min(docs.len()));
    reranked
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "cohere" => "cohere".to_string(),
        "voyage" => "voyage".to_string(),
        "none" | "local" => "none".to_string(),
        _ => "none".to_string(),
    }
}

fn classify_external_fallback_reason(provider: &str, err: &StateError) -> String {
    let normalized = err.to_string().to_ascii_lowercase();
    if normalized.contains("missing_rerank_api_key") {
        return format!("{}_missing_api_key", provider);
    }
    if normalized.contains("timed out") || normalized.contains("timeout") {
        return format!("{}_timeout", provider);
    }
    if normalized.contains("http_") {
        return format!("{}_http_error", provider);
    }
    if normalized.contains("empty_response") {
        return format!("{}_empty_response", provider);
    }
    format!("{}_error", provider)
}

fn tokenize(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|token| token.len() > 1)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn token_overlap_ratio(query_tokens: &HashSet<String>, doc: &str) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }

    let doc_tokens = tokenize(doc);
    let matched = query_tokens
        .iter()
        .filter(|token| doc_tokens.contains(*token))
        .count();
    matched as f64 / query_tokens.len() as f64
}

fn build_external_payload(
    provider: &str,
    query: &str,
    docs: &[RerankDocument],
    top_n: usize,
) -> serde_json::Value {
    let documents: Vec<&str> = docs.iter().map(|doc| doc.text.as_str()).collect();
    let top_n = top_n.min(docs.len()).max(1);

    if provider == "voyage" {
        serde_json::json!({
            "model": "rerank-2-lite",
            "query": query,
            "documents": documents,
            "top_k": top_n,
            "return_documents": false
        })
    } else {
        serde_json::json!({
            "model": "rerank-v3.5",
            "query": query,
            "documents": documents,
            "top_n": top_n
        })
    }
}

fn parse_cohere_response(body: serde_json::Value, docs: &[RerankDocument]) -> Vec<RerankResult> {
    let response: CohereResponse = serde_json::from_value(body).unwrap_or_default();
    response
        .results
        .into_iter()
        .filter_map(|item| {
            docs.get(item.index).map(|doc| RerankResult {
                doc: doc.result_id.clone(),
                score: item.relevance_score,
                provider: "cohere".to_string(),
            })
        })
        .collect()
}

fn parse_voyage_response(body: serde_json::Value, docs: &[RerankDocument]) -> Vec<RerankResult> {
    let response: VoyageResponse = serde_json::from_value(body).unwrap_or_default();
    let candidates = if !response.data.is_empty() {
        response.data
    } else {
        response.results
    };

    candidates
        .into_iter()
        .filter_map(|item| {
            docs.get(item.index).map(|doc| RerankResult {
                doc: doc.result_id.clone(),
                score: item
                    .relevance_score
                    .or(item.score)
                    .unwrap_or(doc.base_score),
                provider: "voyage".to_string(),
            })
        })
        .collect()
}

#[derive(Debug, Default, Deserialize)]
struct CohereResponse {
    #[serde(default)]
    results: Vec<CohereResultItem>,
}

#[derive(Debug, Deserialize)]
struct CohereResultItem {
    index: usize,
    relevance_score: f64,
}

#[derive(Debug, Default, Deserialize)]
struct VoyageResponse {
    #[serde(default)]
    data: Vec<VoyageResultItem>,
    #[serde(default)]
    results: Vec<VoyageResultItem>,
}

#[derive(Debug, Deserialize)]
struct VoyageResultItem {
    index: usize,
    #[serde(default)]
    relevance_score: Option<f64>,
    #[serde(default)]
    score: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn set_test_rerank_api_key(value: Option<&str>) {
        let mut guard = test_rerank_api_key_override().lock().unwrap();
        *guard = value.map(ToString::to_string);
    }

    fn bind_test_listener() -> Option<TcpListener> {
        match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => Some(listener),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(err) => panic!("mock server bind failed: {err}"),
        }
    }

    fn make_doc(id: &str, text: &str, base_score: f64) -> RerankDocument {
        RerankDocument {
            result_id: id.to_string(),
            text: text.to_string(),
            base_score,
        }
    }

    fn semantic_with_provider(
        provider: &str,
        external_provider_enabled: bool,
        allow_code_payload_to_external: bool,
        endpoint: Option<&str>,
    ) -> SemanticConfig {
        let mut semantic = SemanticConfig::default();
        semantic.rerank.provider = provider.to_string();
        semantic.external_provider_enabled = external_provider_enabled;
        semantic.allow_code_payload_to_external = allow_code_payload_to_external;
        semantic.rerank.timeout_ms = 25;
        semantic.rerank.endpoint = endpoint.map(ToString::to_string);
        semantic
    }

    #[test]
    fn local_provider_dispatches_when_external_is_disabled() {
        let docs = vec![
            make_doc("a", "auth login handler", 1.0),
            make_doc("b", "payment retry helper", 1.1),
        ];
        let semantic = semantic_with_provider("none", false, false, None);

        let execution = rerank_documents("auth login", &docs, &semantic, 10);

        assert_eq!(execution.provider, "local");
        assert!(!execution.fallback);
        assert!(!execution.external_provider_blocked);
        assert_eq!(execution.reranked[0].provider, "local");
    }

    #[test]
    fn policy_block_forces_local_fallback() {
        let docs = vec![make_doc("a", "auth login handler", 1.0)];
        let semantic = semantic_with_provider("cohere", false, false, None);

        let execution = rerank_documents("auth login", &docs, &semantic, 10);

        assert_eq!(execution.provider, "local");
        assert!(execution.fallback);
        assert!(execution.external_provider_blocked);
        assert_eq!(
            execution.fallback_reason.as_deref(),
            Some("external_provider_blocked")
        );
    }

    #[test]
    fn external_provider_errors_fail_soft_to_local() {
        let docs = vec![
            make_doc("a", "auth login handler", 1.0),
            make_doc("b", "billing handler", 1.0),
        ];
        let semantic = semantic_with_provider(
            "cohere",
            true,
            true,
            Some("http://127.0.0.1:9/never-reachable"),
        );

        let execution = rerank_documents("auth", &docs, &semantic, 10);

        assert_eq!(execution.provider, "local");
        assert!(execution.fallback);
        assert!(!execution.external_provider_blocked);
        assert!(
            execution
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .starts_with("cohere_")
        );
    }

    #[test]
    fn fallback_reason_never_leaks_secret_material() {
        let fake_secret = "rk-live-secret-value";
        let err = StateError::sqlite(format!(
            "cohere upstream failure while using key={fake_secret}"
        ));

        let reason = classify_external_fallback_reason("cohere", &err);

        assert!(!reason.contains(fake_secret));
        assert!(reason.starts_with("cohere_"));
    }

    #[test]
    fn local_reranker_prefers_query_overlap() {
        let docs = vec![
            make_doc("a", "contains token auth and login", 1.0),
            make_doc("b", "no overlap terms", 1.0),
        ];
        let semantic = semantic_with_provider("none", false, false, None);

        let execution = rerank_documents("auth login", &docs, &semantic, 10);

        assert_eq!(execution.provider, "local");
        assert_eq!(
            execution.reranked.first().map(|item| item.doc.as_str()),
            Some("a")
        );
    }

    #[test]
    fn local_reranker_default_tuning_matches_legacy_scoring_formula() {
        let docs = vec![make_doc("a", "auth login handler", 1.0)];
        let semantic = semantic_with_provider("none", false, false, None);

        let execution = rerank_documents("auth login", &docs, &semantic, 10);
        let query_tokens = tokenize("auth login");
        let overlap = token_overlap_ratio(&query_tokens, "auth login handler");
        let expected = 1.0 + overlap * 2.5 + 0.75;

        assert_eq!(execution.provider, "local");
        assert_eq!(execution.reranked.len(), 1);
        assert!((execution.reranked[0].score - expected).abs() < 1e-12);
    }

    #[test]
    fn parse_external_responses_by_index() {
        let docs = vec![
            make_doc("doc-1", "alpha", 0.1),
            make_doc("doc-2", "beta", 0.1),
        ];

        let cohere = serde_json::json!({
            "results": [
                {"index": 1, "relevance_score": 0.9},
                {"index": 0, "relevance_score": 0.5}
            ]
        });
        let cohere_reranked = parse_cohere_response(cohere, &docs);
        assert_eq!(cohere_reranked[0].doc, "doc-2");
        assert_eq!(cohere_reranked[0].provider, "cohere");

        let voyage = serde_json::json!({
            "data": [
                {"index": 0, "relevance_score": 0.8}
            ]
        });
        let voyage_reranked = parse_voyage_response(voyage, &docs);
        assert_eq!(voyage_reranked[0].doc, "doc-1");
        assert_eq!(voyage_reranked[0].provider, "voyage");
    }

    #[test]
    fn integration_mock_cohere_endpoint_reranks_results() {
        let _guard = env_lock().lock().unwrap();
        set_test_rerank_api_key(Some("test-rerank-key"));

        let Some(listener) = bind_test_listener() else {
            set_test_rerank_api_key(None);
            return;
        };
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("mock server accept failed");
            stream
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let mut request_buf = [0_u8; 4096];
            let _ = stream.read(&mut request_buf);

            let body = r#"{"results":[{"index":1,"relevance_score":0.91},{"index":0,"relevance_score":0.42}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("mock server write failed");
        });

        let docs = vec![
            make_doc("doc-1", "alpha auth", 0.1),
            make_doc("doc-2", "beta login", 0.1),
        ];
        let endpoint = format!("http://{addr}/rerank");
        let semantic = semantic_with_provider("cohere", true, true, Some(endpoint.as_str()));

        let execution = rerank_documents("auth login", &docs, &semantic, 10);

        server.join().unwrap();
        set_test_rerank_api_key(None);

        assert_eq!(execution.provider, "cohere");
        assert!(!execution.fallback);
        assert!(!execution.external_provider_blocked);
        assert_eq!(
            execution.reranked.first().map(|r| r.doc.as_str()),
            Some("doc-2")
        );
    }

    #[test]
    fn integration_mock_cohere_timeout_falls_back_to_local() {
        let _guard = env_lock().lock().unwrap();
        set_test_rerank_api_key(Some("test-rerank-key"));

        let Some(listener) = bind_test_listener() else {
            set_test_rerank_api_key(None);
            return;
        };
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("mock server accept failed");
            // Hold connection open longer than client timeout, then close.
            thread::sleep(Duration::from_millis(220));
            drop(stream);
        });

        let docs = vec![
            make_doc("doc-1", "auth login flow", 0.4),
            make_doc("doc-2", "other context", 0.2),
        ];
        let endpoint = format!("http://{addr}/rerank");
        let mut semantic = semantic_with_provider("cohere", true, true, Some(endpoint.as_str()));
        semantic.rerank.timeout_ms = 50;

        let execution = rerank_documents("auth login", &docs, &semantic, 10);

        server.join().unwrap();
        set_test_rerank_api_key(None);

        assert_eq!(execution.provider, "local");
        assert!(execution.fallback);
        assert!(!execution.external_provider_blocked);
        assert!(
            execution
                .fallback_reason
                .as_deref()
                .unwrap_or_default()
                .starts_with("cohere_")
        );
    }
}
