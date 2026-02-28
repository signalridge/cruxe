## ADDED Requirements

### Requirement: Local cross-encoder reranker via fastembed TextRerank
The rerank pipeline MUST support a `cross-encoder` provider that runs a local ONNX cross-encoder model for semantic reranking, without any external network calls during inference.

Implementation:
- A new `LocalCrossEncoderReranker` struct MUST implement the existing `Rerank` trait (`rerank.rs:47-54`).
- The reranker MUST use fastembed's `TextRerank` API (`TextRerank::try_new(options)` → `reranker.rerank(query, documents, return_documents, batch_size)`) rather than managing ONNX Runtime sessions directly.
- The `rerank_documents()` function (`rerank.rs:240-297`) MUST dispatch to the cross-encoder when `provider = "cross-encoder"`.
- `cruxe-query` MUST declare a `fastembed` workspace dependency to use `TextRerank` in this crate. The workspace already pins fastembed and required features (`hf-hub-rustls-tls`).

#### Scenario: Cross-encoder reranks candidates by semantic relevance
- **WHEN** `search.semantic.rerank.provider = "cross-encoder"` is configured
- **AND** a search produces 20 lexical candidates
- **THEN** the cross-encoder MUST score each `(query, candidate_text)` pair via `TextRerank::rerank()`
- **AND** results MUST be reordered by cross-encoder score (descending)
- **AND** `RerankExecution.provider` MUST be `"cross-encoder"`
- **NOTE** Expected latency: ~20-50ms for 20 candidates on CPU (reference: ms-marco-MiniLM-L-6-v2 benchmarks). No academic code-specific reranker exists; models are trained on MS MARCO/BEIR and rely on transfer learning for code.

#### Scenario: Cross-encoder respects top_n truncation
- **WHEN** `top_n = 10` and there are 20 candidates
- **THEN** cruxe MUST truncate rerank output to exactly 10 results (the highest scoring)

#### Scenario: Input exceeding max_length is truncated before inference
- **WHEN** a `(query, document)` pair produces tokens exceeding `cross_encoder_max_length` (default 512)
- **THEN** the input MUST be truncated to `cross_encoder_max_length` tokens
- **AND** truncation MUST NOT cause a runtime error

### Requirement: Three-tier reranker provider hierarchy with fallback
The rerank pipeline MUST support three provider tiers with automatic fallback.

Provider tiers:
1. `none`/`local`: rule-based reranker (existing `LocalRuleReranker`, no change).
2. `cross-encoder`: local ONNX cross-encoder model (new).
3. `cohere`/`voyage`: external API reranker (existing `ExternalRerankProvider`, no change).

Fallback behavior:
- `cross-encoder` model load failure MUST fall back to `local` rule-based reranker.
- `cross-encoder` inference failure MUST fall back to `local` rule-based reranker.
- Fallback MUST set `RerankExecution.fallback = true` and `fallback_reason` to a deterministic code.

Fallback reason codes:
- `cross_encoder_model_load_failed`: model file not found or ONNX load error.
- `cross_encoder_inference_failed`: runtime inference error.
- `cross_encoder_timeout`: inference exceeded configured timeout.

#### Scenario: Cross-encoder model load failure falls back to local
- **WHEN** `provider = "cross-encoder"` but the ONNX model file is missing or corrupted
- **THEN** the reranker MUST fall back to `LocalRuleReranker`
- **AND** `RerankExecution.fallback` MUST be `true`
- **AND** `RerankExecution.fallback_reason` MUST be `"cross_encoder_model_load_failed"`
- **AND** search MUST still return valid results (non-fatal)

#### Scenario: Cross-encoder inference error falls back to local
- **WHEN** the ONNX model is loaded but inference fails on a specific input
- **THEN** the reranker MUST fall back to `LocalRuleReranker` for the entire batch
- **AND** `fallback_reason` MUST be `"cross_encoder_inference_failed"`

#### Scenario: Provider none/local unchanged
- **WHEN** `provider = "none"` or `provider = "local"`
- **THEN** behavior MUST be identical to current `LocalRuleReranker` (no regression)

### Requirement: Model lifecycle management with lazy loading and caching
The cross-encoder model MUST be loaded lazily on first use and cached for subsequent rerank calls within the same process.

Lifecycle:
- Model MUST NOT be loaded at process startup (no startup penalty).
- First `rerank()` call with `provider = "cross-encoder"` MUST trigger model load.
- Loaded model MUST be cached in-process (`OnceLock` or equivalent) for reuse.
- Model files MUST be cached on disk in fastembed cache path (`cache_dir` if configured; otherwise fastembed default HF cache path).

Auto-download:
- If model files are not present locally, the reranker MUST attempt to download from HuggingFace Hub (fastembed handles this via its built-in `hf-hub` integration).
- Download failure MUST trigger fallback to `local` (non-fatal).
- Download/inference timeout SHOULD be bounded by cruxe outer execution budget (best-effort; fastembed API does not expose dedicated model-load timeout).
- Model sizes: `rozgo/bge-reranker-v2-m3` ~1.1GB, `jinaai/jina-reranker-v1-turbo-en` ~37MB. The lighter model MAY be recommended for first-time setup or low-bandwidth environments.

#### Scenario: First rerank call triggers lazy model load
- **WHEN** the first search with `provider = "cross-encoder"` is executed
- **THEN** the model MUST be loaded on that first call (not at process startup)
- **AND** subsequent calls MUST reuse the cached model without reload

#### Scenario: Model auto-downloaded on first use
- **WHEN** local fastembed cache does not contain `<model-name>`
- **THEN** the reranker MUST download the model from HuggingFace Hub
- **AND** store it in fastembed cache directory
- **AND** subsequent runs MUST use the local cache without re-downloading

#### Scenario: Offline environment with no cached model
- **WHEN** `provider = "cross-encoder"` and no model is cached and network is unavailable
- **THEN** the reranker MUST fall back to `local` rule-based reranker
- **AND** `fallback_reason` MUST be `"cross_encoder_model_load_failed"`

### Requirement: Cross-encoder configuration
The cross-encoder reranker MUST be configurable via the existing `SemanticRerankConfig` structure.

New configuration fields:
- `search.semantic.rerank.cross_encoder_model`: model identifier (default `"rozgo/bge-reranker-v2-m3"`). MUST correspond to a fastembed built-in model or a valid HuggingFace Hub model path loadable via `UserDefinedRerankingModel`.
- `search.semantic.rerank.cross_encoder_max_length`: maximum input token length (default 512).

The `normalize_rerank_provider()` function (`config.rs:1103-1109`) MUST recognize `"cross-encoder"` as a valid provider value.

Built-in model options (fastembed v5.11):
- `BAAI/bge-reranker-base` (~278MB, English-only, fastest)
- `rozgo/bge-reranker-v2-m3` (~1.1GB, multilingual, recommended default)
- `jinaai/jina-reranker-v1-turbo-en` (~37MB, English-only, smallest)
- `jinaai/jina-reranker-v2-base-multilingual` (~1.1GB, multilingual, 8192 max tokens)

#### Scenario: Cross-encoder provider recognized in config
- **WHEN** `search.semantic.rerank.provider = "cross-encoder"` is set in configuration
- **THEN** `normalize_rerank_provider("cross-encoder")` MUST return `"cross-encoder"` (not fall through to default `"none"`)

#### Scenario: Default model used when not specified
- **WHEN** `cross_encoder_model` is not set in configuration
- **THEN** the default model `"rozgo/bge-reranker-v2-m3"` MUST be used

#### Scenario: Custom model override
- **WHEN** `search.semantic.rerank.cross_encoder_model = "jinaai/jina-reranker-v2-base-multilingual"` is set
- **THEN** the specified model MUST be loaded instead of the default
