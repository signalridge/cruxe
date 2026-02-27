use codecompass_core::config::SemanticConfig;
use codecompass_core::error::StateError;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tracing::warn;

const VOYAGE_EMBED_ENDPOINT: &str = "https://api.voyageai.com/v1/embeddings";
const OPENAI_EMBED_ENDPOINT: &str = "https://api.openai.com/v1/embeddings";
const MAX_EMBEDDING_HTTP_CLIENT_CACHE_ENTRIES: usize = 4;
const DEFAULT_FASTEMBED_CACHE_CAPACITY: usize = 4096;
type SharedTextEmbeddingRuntime = Arc<Mutex<TextEmbedding>>;
type RuntimeCache = HashMap<String, Option<SharedTextEmbeddingRuntime>>;
static FASTEMBED_RUNTIME_CACHE: OnceLock<Mutex<RuntimeCache>> = OnceLock::new();
static EMBEDDING_HTTP_CLIENT_CACHE: OnceLock<Mutex<HashMap<u64, Client>>> = OnceLock::new();

fn runtime_cache() -> &'static Mutex<RuntimeCache> {
    FASTEMBED_RUNTIME_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub trait EmbeddingProvider {
    fn model_id(&self) -> &str;
    fn model_version(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn embed_batch(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, StateError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingModelProfile {
    pub profile: String,
    pub model_name: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone)]
struct EmbeddingModelSelection {
    profile: EmbeddingModelProfile,
    model_version: String,
    batch_size: usize,
    fastembed_model: Option<EmbeddingModel>,
}

pub struct BuiltEmbeddingProvider {
    pub provider: Box<dyn EmbeddingProvider + Send>,
    pub external_provider_blocked: bool,
}

pub fn build_embedding_provider(
    semantic: &SemanticConfig,
) -> Result<BuiltEmbeddingProvider, StateError> {
    let selection = resolve_embedding_model(semantic)?;
    let provider = semantic.embedding.provider.to_ascii_lowercase();
    let enable_runtime = fastembed_runtime_enabled();
    match provider.as_str() {
        "voyage" | "openai" => {
            if !semantic.allow_external_provider_calls() {
                // Privacy gates force local-only path.
                let local = FastEmbedProvider::new(selection, enable_runtime);
                return Ok(BuiltEmbeddingProvider {
                    provider: Box::new(local),
                    external_provider_blocked: true,
                });
            }

            let external = ExternalEmbeddingProvider::new(
                provider,
                selection.profile.model_name.clone(),
                selection.model_version.clone(),
                selection.profile.dimensions,
                selection.batch_size,
            )?;
            Ok(BuiltEmbeddingProvider {
                provider: Box::new(external),
                external_provider_blocked: false,
            })
        }
        _ => {
            let local = FastEmbedProvider::new(selection, enable_runtime);
            Ok(BuiltEmbeddingProvider {
                provider: Box::new(local),
                external_provider_blocked: false,
            })
        }
    }
}

fn resolve_embedding_model(
    semantic: &SemanticConfig,
) -> Result<EmbeddingModelSelection, StateError> {
    let profile = semantic.embedding.profile.trim().to_ascii_lowercase();
    let explicit_model = semantic.embedding.model.trim();

    let default_model = match profile.as_str() {
        "code_quality" => "JinaEmbeddingsV2BaseCode",
        "high_quality" => "BGELargeENV15",
        _ => "NomicEmbedTextV15Q",
    };
    let chosen_model = if explicit_model.is_empty() {
        default_model.to_string()
    } else {
        explicit_model.to_string()
    };

    let expected_dim = model_dimensions(&chosen_model).unwrap_or(semantic.embedding.dimensions);
    if semantic.embedding.provider == "local" && semantic.embedding.dimensions != expected_dim {
        return Err(StateError::external(format!(
            "embedding dimensions mismatch: model={} expected={} configured={}",
            chosen_model, expected_dim, semantic.embedding.dimensions
        )));
    }

    Ok(EmbeddingModelSelection {
        profile: EmbeddingModelProfile {
            profile: if profile.is_empty() {
                "fast_local".to_string()
            } else {
                profile
            },
            model_name: chosen_model.clone(),
            dimensions: expected_dim,
        },
        model_version: semantic.embedding.model_version.clone(),
        batch_size: semantic.embedding.batch_size.max(1),
        fastembed_model: parse_fastembed_model(&chosen_model),
    })
}

fn parse_fastembed_model(model: &str) -> Option<EmbeddingModel> {
    let key = model.trim().to_ascii_lowercase();
    match key.as_str() {
        "nomicembedtextv15q" => Some(EmbeddingModel::NomicEmbedTextV15Q),
        "bgesmallenv15q" => Some(EmbeddingModel::BGESmallENV15Q),
        "bgebaseenv15q" => Some(EmbeddingModel::BGEBaseENV15Q),
        "jinaembeddingsv2basecode" => Some(EmbeddingModel::JinaEmbeddingsV2BaseCode),
        "bgelargeenv15" => Some(EmbeddingModel::BGELargeENV15),
        "gtelargeenv15" => Some(EmbeddingModel::GTELargeENV15),
        "snowflakearcticembedl" => Some(EmbeddingModel::SnowflakeArcticEmbedL),
        _ => model.parse::<EmbeddingModel>().ok(),
    }
}

fn model_dimensions(model: &str) -> Option<usize> {
    let target = parse_fastembed_model(model)?;
    TextEmbedding::list_supported_models()
        .into_iter()
        .find(|entry| entry.model == target)
        .map(|entry| entry.dim)
}

pub struct FastEmbedProvider {
    model_id: String,
    model_version: String,
    dimensions: usize,
    batch_size: usize,
    fastembed_model: Option<EmbeddingModel>,
    runtime: Option<SharedTextEmbeddingRuntime>,
    cache: HashMap<String, Vec<f32>>,
    cache_order: VecDeque<String>,
    cache_capacity: usize,
    enable_runtime: bool,
    attempted_runtime_init: bool,
}

impl FastEmbedProvider {
    fn new(selection: EmbeddingModelSelection, enable_runtime: bool) -> Self {
        Self {
            model_id: selection.profile.model_name,
            model_version: selection.model_version,
            dimensions: selection.profile.dimensions,
            batch_size: selection.batch_size,
            fastembed_model: selection.fastembed_model,
            runtime: None,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            cache_capacity: fastembed_cache_capacity(),
            enable_runtime,
            attempted_runtime_init: false,
        }
    }

    #[cfg(test)]
    fn cache_entries(&self) -> usize {
        self.cache.len()
    }

    fn ensure_runtime(&mut self) {
        if self.attempted_runtime_init || !self.enable_runtime {
            return;
        }
        self.attempted_runtime_init = true;
        let Some(model) = self.fastembed_model.clone() else {
            return;
        };
        let cache_key = self.model_id.clone();

        if let Ok(cache) = runtime_cache().lock()
            && let Some(cached) = cache.get(&cache_key).cloned()
        {
            self.runtime = cached;
            return;
        }

        let options = TextInitOptions::new(model).with_show_download_progress(false);
        match TextEmbedding::try_new(options) {
            Ok(runtime) => {
                let shared_runtime: SharedTextEmbeddingRuntime = Arc::new(Mutex::new(runtime));
                self.runtime = Some(shared_runtime.clone());
                if let Ok(mut cache) = runtime_cache().lock() {
                    cache.insert(cache_key, Some(shared_runtime));
                }
            }
            Err(err) => {
                warn!(
                    model = self.model_id,
                    error = %err,
                    "fastembed initialization failed, falling back to deterministic embeddings"
                );
                if let Ok(mut cache) = runtime_cache().lock() {
                    cache.insert(cache_key, None);
                }
            }
        }
    }

    fn embed_uncached(&mut self, uncached_inputs: &[String]) -> Vec<Vec<f32>> {
        self.ensure_runtime();
        if let Some(runtime) = self.runtime.as_ref() {
            let refs: Vec<&str> = uncached_inputs.iter().map(String::as_str).collect();
            let embed_result = runtime
                .lock()
                .ok()
                .and_then(|mut runtime| runtime.embed(refs, Some(self.batch_size)).ok());
            if let Some(vectors) = embed_result
                && vectors.iter().all(|v| v.len() == self.dimensions)
            {
                return vectors;
            }
            warn!(
                model = self.model_id,
                "fastembed runtime returned invalid embedding shape; switching to deterministic fallback"
            );
            self.runtime = None;
            if let Ok(mut cache) = runtime_cache().lock() {
                cache.insert(self.model_id.clone(), None);
            }
        }

        uncached_inputs
            .iter()
            .map(|input| deterministic_embedding(input, self.dimensions))
            .collect()
    }

    fn insert_cache_entry(&mut self, input: String, vector: Vec<f32>) {
        if self.cache_capacity == 0 {
            return;
        }

        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            self.cache.entry(input.clone())
        {
            entry.insert(vector);
            self.cache_order.retain(|k| k != &input);
            self.cache_order.push_back(input);
            return;
        }

        while self.cache.len() >= self.cache_capacity {
            let Some(evicted_key) = self.cache_order.pop_front() else {
                break;
            };
            self.cache.remove(&evicted_key);
        }

        self.cache_order.push_back(input.clone());
        self.cache.insert(input, vector);
    }
}

impl EmbeddingProvider for FastEmbedProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_version(&self) -> &str {
        &self.model_version
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_batch(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, StateError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let mut uncached = Vec::new();
        for input in inputs {
            if !self.cache.contains_key(input) {
                uncached.push(input.clone());
            }
        }
        if !uncached.is_empty() {
            let vectors = self.embed_uncached(&uncached);
            for (input, vector) in uncached.into_iter().zip(vectors.into_iter()) {
                self.insert_cache_entry(input, vector);
            }
        }

        let mut output = Vec::with_capacity(inputs.len());
        for input in inputs {
            if let Some(vector) = self.cache.get(input) {
                output.push(vector.clone());
            } else {
                output.push(deterministic_embedding(input, self.dimensions));
            }
        }
        Ok(output)
    }
}

pub struct ExternalEmbeddingProvider {
    provider: String,
    model_id: String,
    model_version: String,
    dimensions: usize,
    batch_size: usize,
    endpoint: String,
    client: Client,
}

impl ExternalEmbeddingProvider {
    fn new(
        provider: String,
        model_id: String,
        model_version: String,
        dimensions: usize,
        batch_size: usize,
    ) -> Result<Self, StateError> {
        let endpoint = match provider.as_str() {
            "voyage" => VOYAGE_EMBED_ENDPOINT.to_string(),
            "openai" => OPENAI_EMBED_ENDPOINT.to_string(),
            _ => OPENAI_EMBED_ENDPOINT.to_string(),
        };
        let client = shared_embedding_http_client(Duration::from_secs(5))?;
        Ok(Self {
            provider,
            model_id,
            model_version,
            dimensions,
            batch_size,
            endpoint,
            client,
        })
    }
}

fn shared_embedding_http_client(timeout: Duration) -> Result<Client, StateError> {
    let timeout_ms = timeout.as_millis() as u64;
    codecompass_core::cache::get_or_insert_cached(
        &EMBEDDING_HTTP_CLIENT_CACHE,
        timeout_ms,
        MAX_EMBEDDING_HTTP_CLIENT_CACHE_ENTRIES,
        || {
            Client::builder()
                .timeout(timeout)
                .build()
                .map_err(StateError::external)
        },
    )
}

impl EmbeddingProvider for ExternalEmbeddingProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_version(&self) -> &str {
        &self.model_version
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_batch(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>, StateError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let api_key = std::env::var("CODECOMPASS_EMBEDDING_API_KEY")
            .map_err(|_| StateError::external("missing_embedding_api_key"))?;
        let mut all_vectors = Vec::with_capacity(inputs.len());

        for chunk in inputs.chunks(self.batch_size) {
            let payload = if self.provider == "voyage" {
                serde_json::json!({
                    "model": self.model_id,
                    "input": chunk,
                    "input_type": "document"
                })
            } else {
                serde_json::json!({
                    "model": self.model_id,
                    "input": chunk
                })
            };

            let response = self
                .client
                .post(&self.endpoint)
                .bearer_auth(&api_key)
                .header("content-type", "application/json")
                .json(&payload)
                .send()
                .map_err(StateError::external)?;
            if !response.status().is_success() {
                return Err(StateError::external(format!(
                    "external_embedding_http_{}",
                    response.status().as_u16()
                )));
            }
            let body: EmbeddingApiResponse = response.json().map_err(StateError::external)?;
            let chunk_vectors = align_external_embeddings(body.data, chunk.len(), self.dimensions)?;
            all_vectors.extend(chunk_vectors);
        }

        if all_vectors.len() != inputs.len() {
            return Err(StateError::external(format!(
                "external_embedding_result_count_mismatch expected={} got={}",
                inputs.len(),
                all_vectors.len()
            )));
        }
        Ok(all_vectors)
    }
}

#[derive(Debug, Deserialize)]
struct EmbeddingApiResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    index: Option<usize>,
    embedding: Vec<f32>,
}

fn align_external_embeddings(
    data: Vec<EmbeddingData>,
    expected_count: usize,
    dimensions: usize,
) -> Result<Vec<Vec<f32>>, StateError> {
    let indexed_count = data.iter().filter(|item| item.index.is_some()).count();
    if indexed_count > 0 {
        if indexed_count != data.len() {
            return Err(StateError::external(
                "external_embedding_index_mixed_presence".to_string(),
            ));
        }

        let mut ordered = vec![None; expected_count];
        for item in data {
            let index = item
                .index
                .ok_or_else(|| StateError::external("external_embedding_missing_index"))?;
            if index >= expected_count {
                return Err(StateError::external(format!(
                    "external_embedding_index_out_of_range index={} expected_count={}",
                    index, expected_count
                )));
            }
            if item.embedding.len() != dimensions {
                return Err(StateError::external(format!(
                    "external_embedding_dimensions_mismatch expected={} got={}",
                    dimensions,
                    item.embedding.len()
                )));
            }
            if ordered[index].is_some() {
                return Err(StateError::external(format!(
                    "external_embedding_duplicate_index index={}",
                    index
                )));
            }
            ordered[index] = Some(item.embedding);
        }

        if let Some(missing_index) = ordered.iter().position(Option::is_none) {
            return Err(StateError::external(format!(
                "external_embedding_missing_index index={}",
                missing_index
            )));
        }

        return Ok(ordered
            .into_iter()
            .map(|value| value.expect("missing index already validated"))
            .collect());
    }

    if data.len() != expected_count {
        return Err(StateError::external(format!(
            "external_embedding_result_count_mismatch expected={} got={}",
            expected_count,
            data.len()
        )));
    }

    let mut vectors = Vec::with_capacity(expected_count);
    for item in data {
        if item.embedding.len() != dimensions {
            return Err(StateError::external(format!(
                "external_embedding_dimensions_mismatch expected={} got={}",
                dimensions,
                item.embedding.len()
            )));
        }
        vectors.push(item.embedding);
    }
    Ok(vectors)
}

fn deterministic_embedding(input: &str, dimensions: usize) -> Vec<f32> {
    if dimensions == 0 {
        return Vec::new();
    }
    let seed_hash = blake3::hash(input.as_bytes());
    let mut state = u64::from_le_bytes(
        seed_hash.as_bytes()[0..8]
            .try_into()
            .expect("seed hash has at least 8 bytes"),
    );
    if state == 0 {
        // xorshift generators must not use an all-zero state.
        state = 0x9e37_79b9_7f4a_7c15;
    }

    let mut vector = Vec::with_capacity(dimensions);
    for _ in 0..dimensions {
        // xorshift64*: deterministic, allocation-free pseudo random stream.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let n = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let scaled = (n as f64 / u64::MAX as f64) * 2.0 - 1.0;
        vector.push(scaled as f32);
    }

    let norm = vector
        .iter()
        .map(|v| {
            let value = *v as f64;
            value * value
        })
        .sum::<f64>()
        .sqrt();
    if norm == 0.0 {
        return vector;
    }
    vector
        .into_iter()
        .map(|v| (v as f64 / norm) as f32)
        .collect()
}

fn fastembed_cache_capacity() -> usize {
    std::env::var("CODECOMPASS_FASTEMBED_CACHE_CAP")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_FASTEMBED_CACHE_CAPACITY)
}

fn fastembed_runtime_enabled() -> bool {
    parse_fastembed_runtime_flag(
        std::env::var("CODECOMPASS_ENABLE_FASTEMBED_RUNTIME")
            .ok()
            .map(|value| value.to_ascii_lowercase()),
    )
}

fn parse_fastembed_runtime_flag(raw: Option<String>) -> bool {
    match raw {
        None => true,
        Some(value) => !matches!(value.as_str(), "0" | "false" | "no"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semantic_config() -> SemanticConfig {
        let mut config = SemanticConfig::default();
        config.embedding.profile = "fast_local".to_string();
        config.embedding.provider = "local".to_string();
        config.embedding.model = "NomicEmbedTextV15Q".to_string();
        config.embedding.model_version = "fastembed-1".to_string();
        config.embedding.dimensions = 768;
        config.embedding.batch_size = 2;
        config
    }

    #[test]
    fn profile_mapping_returns_expected_defaults() {
        let mut cfg = semantic_config();
        let selection = resolve_embedding_model(&cfg).unwrap();
        assert_eq!(selection.profile.model_name, "NomicEmbedTextV15Q");
        assert_eq!(selection.profile.dimensions, 768);

        cfg.embedding.profile = "code_quality".to_string();
        cfg.embedding.model = "".to_string();
        cfg.embedding.dimensions = 768;
        let selection = resolve_embedding_model(&cfg).unwrap();
        assert_eq!(selection.profile.model_name, "JinaEmbeddingsV2BaseCode");
    }

    #[test]
    fn dimensions_validation_rejects_mismatch() {
        let mut cfg = semantic_config();
        cfg.embedding.model = "BGESmallENV15Q".to_string();
        cfg.embedding.dimensions = 768;
        let err = resolve_embedding_model(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("embedding dimensions mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn batch_processing_uses_cache_for_repeated_inputs() {
        let selection = resolve_embedding_model(&semantic_config()).unwrap();
        let mut provider = FastEmbedProvider::new(selection, false);

        let first = provider
            .embed_batch(&["alpha".to_string(), "beta".to_string(), "alpha".to_string()])
            .unwrap();
        assert_eq!(provider.cache_entries(), 2);
        assert_eq!(first.len(), 3);
        assert_eq!(first[0], first[2]);

        let second = provider
            .embed_batch(&["alpha".to_string(), "beta".to_string()])
            .unwrap();
        assert_eq!(provider.cache_entries(), 2);
        assert_eq!(second.len(), 2);
        assert_eq!(first[0], second[0]);
    }

    #[test]
    fn cache_respects_configured_capacity() {
        let selection = resolve_embedding_model(&semantic_config()).unwrap();
        let mut provider = FastEmbedProvider::new(selection, false);
        provider.cache_capacity = 1;

        provider
            .embed_batch(&["alpha".to_string(), "beta".to_string()])
            .unwrap();
        assert_eq!(provider.cache_entries(), 1);
    }

    #[test]
    fn external_provider_is_blocked_without_privacy_gates() {
        let mut cfg = semantic_config();
        cfg.embedding.provider = "openai".to_string();
        cfg.external_provider_enabled = false;
        cfg.allow_code_payload_to_external = false;

        let built = build_embedding_provider(&cfg).unwrap();
        assert!(built.external_provider_blocked);
        assert_eq!(built.provider.model_id(), "NomicEmbedTextV15Q");
    }

    #[test]
    fn fastembed_runtime_flag_defaults_to_enabled() {
        assert!(parse_fastembed_runtime_flag(None));
        assert!(parse_fastembed_runtime_flag(Some("1".to_string())));
        assert!(parse_fastembed_runtime_flag(Some("true".to_string())));
        assert!(!parse_fastembed_runtime_flag(Some("0".to_string())));
        assert!(!parse_fastembed_runtime_flag(Some("false".to_string())));
    }

    #[test]
    fn external_embeddings_are_reordered_by_index_when_present() {
        let aligned = align_external_embeddings(
            vec![
                EmbeddingData {
                    index: Some(1),
                    embedding: vec![2.0, 0.0],
                },
                EmbeddingData {
                    index: Some(0),
                    embedding: vec![1.0, 0.0],
                },
            ],
            2,
            2,
        )
        .unwrap();
        assert_eq!(aligned[0], vec![1.0, 0.0]);
        assert_eq!(aligned[1], vec![2.0, 0.0]);
    }

    #[test]
    fn external_embeddings_reject_mixed_index_presence() {
        let err = align_external_embeddings(
            vec![
                EmbeddingData {
                    index: Some(0),
                    embedding: vec![1.0, 0.0],
                },
                EmbeddingData {
                    index: None,
                    embedding: vec![2.0, 0.0],
                },
            ],
            2,
            2,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("external_embedding_index_mixed_presence")
        );
    }

    #[test]
    fn external_embeddings_reject_out_of_range_index() {
        let err = align_external_embeddings(
            vec![EmbeddingData {
                index: Some(3),
                embedding: vec![1.0, 0.0],
            }],
            1,
            2,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("external_embedding_index_out_of_range")
        );
    }
}
