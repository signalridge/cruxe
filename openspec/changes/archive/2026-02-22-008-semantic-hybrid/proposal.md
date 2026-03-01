## Why

Pure lexical search fails for natural language queries where developers ask conceptual questions like "where is authentication handled" -- exact keyword matching cannot surface code that is semantically relevant but uses different terminology. Adding optional hybrid search with local-first embeddings and external rerank provider abstraction closes this gap while preserving lexical-first behavior as the safe default. Feature flags, confidence thresholds, and privacy gates ensure gradual rollout without forcing any external dependency.

## What Changes

1. Add hybrid search blending lexical (Tantivy) and semantic (embedded vector store) results for `natural_language` query intent, with configurable ratio cap and lexical short-circuit.
2. Add feature flag system (`semantic_mode`: off/rerank_only/hybrid, `semantic_ratio`, per-query-type overrides, privacy gates) for gradual rollout.
3. Add external rerank provider abstraction (`Rerank` trait) with Cohere/Voyage implementations and fail-soft fallback to local rule-based reranker.
4. Add inline low-confidence guidance (`low_confidence`, `suggested_action`) in search response metadata using composite confidence signals.
5. Add embedded vector store with stable identity keys (`symbol_stable_id` + `snippet_hash`), model versioning, and local-first embedding profiles (`fast_local`/`code_quality`/`high_quality`).
6. Add semantic profile advisor mode that recommends embedding profile tiers based on repo size, language mix, and latency budget.

## Capabilities

### New Capabilities

- **Hybrid search blending** (FR-701, FR-706, FR-707, FR-716, FR-720, FR-725): Embedded vector store with local-first embedding backend (`fastembed`), stable symbol identity keys, model version partitioning, and tiered profiles (`fast_local`/`code_quality`/`high_quality`). Blends lexical and semantic results for `natural_language` intent using weighted RRF with configurable ratio cap.
- **Rerank provider abstraction** (FR-708, FR-709, FR-710, FR-711): `Rerank` trait with external provider implementations (Cohere Rerank v3, Voyage Rerank), fail-soft fallback to local rule-based reranker on error/timeout (5s), API keys from environment variables only.
- **Composite confidence guidance** (FR-712, FR-713, FR-721, FR-727): Inline `low_confidence` flag and `suggested_action` in response metadata, using composite signals (top score, top1-top2 margin, lexical/semantic agreement). Configurable `confidence_threshold` (default 0.5). Emits `query_intent_confidence` and `intent_escalation_hint` for agent routing.
- **Semantic profile advisor** (FR-722, FR-726, FR-728): Diagnostic recommender suggesting embedding profile tier with explicit reason codes (repo size bucket, language mix, latency budget). Deterministic and reproducible. Does not mutate active config.

### Modified Capabilities

- **Search pipeline** (FR-702, FR-717, FR-718): Symbol/path/error intents remain lexical-only. Lexical short-circuit skips semantic when lexical confidence exceeds threshold. Metadata includes `semantic_triggered`, `semantic_skipped_reason`.
- **Configuration system** (FR-703, FR-704, FR-705, FR-723, FR-724): `semantic_mode` (off/rerank_only/hybrid), `semantic_ratio` (cap, not forced blend), per-query-type overrides, embedding profile config, rerank provider config in `config.toml`.
- **Privacy and external provider gates** (FR-719): `external_provider_enabled` and `allow_code_payload_to_external` (both default false) gate all outbound embedding/rerank calls.
- **Search response metadata** (FR-715, FR-718): `semantic_mode`, `semantic_enabled`, `semantic_ratio_used`, `rerank_provider`, `semantic_triggered`, `semantic_skipped_reason`, `result_provenance`, `embedding_model_version`.
- **Agent protocol** (FR-714): Incremental re-embedding during `sync_repo` for changed files. Response metadata extended with semantic and confidence fields.

### Functional Requirements

- **FR-701**: System MUST support hybrid search blending lexical (Tantivy) and semantic (embedded vector store) results for `natural_language` query intent only.
- **FR-702**: System MUST NOT use semantic search for `symbol`, `path`, or `error` query intents; these remain lexical-first.
- **FR-703**: System MUST provide `semantic_mode` (`off` | `rerank_only` | `hybrid`, default: `off`) and `semantic_ratio` (f64, 0.0-1.0, default: 0.3) configuration options in `config.toml`, where `semantic_ratio` is treated as the maximum semantic contribution.
- **FR-704**: System MUST support per-request `semantic_ratio` override in `search_code` tool input.
- **FR-705**: System MUST support per-query-type `semantic_ratio` overrides in `config.toml`.
- **FR-706**: System MUST provide an embedded vector-store abstraction with no mandatory external service dependency. Default implementation uses local persisted storage (SQLite metadata + vector segment/table), with optional pluggable adapters.
- **FR-707**: System MUST generate vector embeddings using a configurable model provider (local first, external optional), with model selection in `config.toml`.
- **FR-708**: System MUST provide a `Rerank` trait abstraction: `Rerank(ctx, query, docs) -> Vec<(doc, score)>`.
- **FR-709**: System MUST implement at least one external rerank provider (Cohere Rerank v3 or Voyage Rerank) behind the `Rerank` trait.
- **FR-710**: System MUST read rerank API keys from environment variables only (`CRUXE_RERANK_API_KEY`), never from config files, and never log them.
- **FR-711**: System MUST fall back to local rule-based reranker when external provider is unavailable, times out (5s), or returns an error, with `rerank_fallback: true` in metadata.
- **FR-712**: System MUST include `low_confidence: true` and `suggested_action` in search response metadata when composite confidence is below the confidence threshold.
- **FR-713**: System MUST support a configurable `confidence_threshold` (default: 0.5) in `config.toml` and per-request.
- **FR-714**: System MUST regenerate embeddings for changed files during `sync_repo` incremental sync.
- **FR-715**: System MUST include `semantic_mode`, `semantic_enabled` (derived runtime flag), `semantic_ratio_used`, and `rerank_provider` in search response metadata for observability.
- **FR-716**: System MUST key vector records by stable identity `(repo, ref, symbol_stable_id, snippet_hash, embedding_model_version)` and MUST NOT use line-range-only identity for deduplication.
- **FR-717**: System MUST support lexical short-circuit behavior: if lexical confidence exceeds `lexical_short_circuit_threshold`, semantic search is skipped.
- **FR-718**: System MUST include `semantic_triggered` and optional `semantic_skipped_reason` in response metadata for semantic execution transparency.
- **FR-719**: System MUST gate outbound embedding/rerank calls behind `external_provider_enabled` and `allow_code_payload_to_external`, both default `false`.
- **FR-720**: System MUST store `embedding_model_id`, `embedding_model_version`, and `embedding_dimensions` with vector records and avoid cross-version similarity scoring.
- **FR-721**: Confidence evaluation MUST combine at least three signals: top score, top1-top2 margin, and lexical/semantic agreement.
- **FR-722**: System MUST provide two default semantic profiles: `fast_local` (small model, low latency) and `code_quality` (code-aware model, higher cost).
- **FR-723**: System MUST support `semantic_mode` with values: `off` (default), `rerank_only`, `hybrid`.
- **FR-724**: Under `semantic_mode = rerank_only`, vector index and embedding generation MUST be skipped; search remains lexical-first with optional rerank provider.
- **FR-725**: Local embedding profiles MUST map to concrete Rust-friendly model presets: `fast_local`: `NomicEmbedTextV15Q` or `BGESmallENV15Q`; `code_quality`: `BGEBaseENV15Q` or `JinaEmbeddingsV2BaseCode`; `high_quality` (optional): `BGELargeENV15`, `GTELargeENV15`, `SnowflakeArcticEmbedL`.
- **FR-726**: Default semantic profile promotion MUST be benchmark-gated by repo size bucket (`<10k`, `10k-50k`, `>50k` files) with latency, RSS, and MRR evidence.
- **FR-727**: Search responses MUST include `query_intent_confidence` (0.0-1.0) and optional `intent_escalation_hint` metadata when confidence is below threshold.
- **FR-728**: System MUST provide a profile advisor mode that recommends semantic profile tiers using repo-size bucket, language composition, and configured latency/resource budget, without automatically mutating active config.

### Key Entities

- **VectorIndex**: Embedded vector store containing code snippet embeddings, keyed by stable symbol identity + snippet hash + ref, with strict embedding model version partitioning (backend pluggable; local-first default).
- **EmbeddingModel**: A configurable model that converts code snippets into vector representations. Can be a local model (for example `BGE*`, `Nomic*`, `Jina*`) or external API.
- **RerankProvider**: An abstraction over external reranking services, with a trait interface and fail-soft behavior.
- **HybridResult**: A search result that combines lexical score, semantic score, and optionally reranked score, with provenance tracking.
- **ConfidenceGuidance**: Inline metadata in search responses indicating result confidence level and suggested follow-up actions, derived from composite confidence signals.
- **SemanticProfileAdvisor**: Diagnostic recommender that suggests an embedding profile (`fast_local`, `code_quality`, `high_quality`) with explicit reason codes.

### Design Optimization Decisions (2026-02 Revision)

- Keep **lexical-first** as the default search path; semantic is additive for `natural_language` intent only.
- Treat `semantic_ratio` as a **maximum semantic weight**, not a forced fixed weight. The runtime may reduce it (including to `0.0`) when lexical confidence is already high.
- Use stable vector identity keys based on `symbol_stable_id`, not volatile line ranges.
- Require explicit privacy gates before sending code payloads to external providers.
- Use **composite confidence** (top score + score margin + channel agreement) instead of single-threshold top-score checks.
- Prioritize Rust-friendly local inference (`fastembed`) with tiered model profiles before any external embedding dependency.
- Add a **two-track rollout**:
  - Track A (minimal): lexical-first + optional external/local rerank + confidence guidance.
  - Track B (optional): vector hybrid (embedded vector index + embeddings) only when Track A quality is insufficient.

## Impact

### Success Criteria

- **SC-701**: Hybrid search improves MRR (Mean Reciprocal Rank) by >= 15% over lexical-only for natural language queries on a benchmark set of >= 100 queries, stratified by language (Rust/TypeScript/Python/Go, >= 20 queries each).
- **SC-702**: Semantic search adds < 200ms to search latency (p95) for warm index queries.
- **SC-703**: External rerank provider timeout/fallback occurs transparently with zero user-visible errors in 100% of failure scenarios.
- **SC-704**: Embedded vector index size is < 2x the Tantivy index size for the same corpus.
- **SC-705**: Embedding generation during indexing adds < 30% to total index time.
- **SC-706**: With `external_provider_enabled: false` or `allow_code_payload_to_external: false`, outbound embedding/rerank HTTP calls are exactly zero.
- **SC-707**: Benchmarks are reported for all repo-size buckets (`<10k`, `10k-50k`, `>50k` files) before enabling non-`off` semantic defaults in any profile.
- **SC-708**: For queries with `query_intent_confidence >= 0.8`, top-3 precision is >= 85% on the stratified benchmark set.
- **SC-709**: Profile advisor recommendations are reproducible (same repo snapshot yields same recommendation) and deterministic across repeated runs.

### Edge Cases

- What happens when vector index does not exist but semantic search is enabled? The system creates the embedded vector index on first semantic query (lazy initialization), logging a warning that initial query may be slow.
- What happens when the embedding model is unavailable? Semantic search is silently disabled for that query; results are purely lexical with `semantic_fallback: true` and `semantic_degraded: true` in metadata.
- What happens when `semantic_ratio` is set to a value outside 0.0-1.0? The value is clamped to the valid range and a warning is logged.
- What happens when the rerank provider returns fewer results than sent? Missing results retain their original lexical scores; the reranked subset is interleaved with the original scores.
- What happens when the vector index is out of sync with the Tantivy index? Embeddings are regenerated during `sync_repo` for changed files. If a document exists in Tantivy but not in vector store, it is treated as lexical-only.
- What happens when the embedding model produces different dimensions than expected? Index creation fails with a clear error message specifying expected vs actual dimensions. The system falls back to lexical-only.
- What happens when embedding model version changes? New vectors are written under the new `embedding_model_version`; similarity search is restricted to the same model version. Background re-embedding updates old records.
- What happens when external provider configuration is set but privacy gates are off? External calls are blocked, local-only path is used, and metadata indicates `external_provider_blocked: true`.

### Affected Crates

- `cruxe-core`: config fields, metadata types
- `cruxe-state`: vector index, embedding provider
- `cruxe-indexer`: embedding writer, incremental sync
- `cruxe-query`: hybrid blending, rerank trait, confidence, search pipeline
- `cruxe-mcp`: search_code tool updates

### API Impact

Additive only. New optional parameters (`semantic_ratio`, `confidence_threshold`) on `search_code`. New metadata fields in search responses. No breaking changes; `semantic_mode: off` default preserves existing behavior.

### Performance Impact

- Hybrid path latency overhead < 200ms p95 (warm index).
- Embedding generation overhead < 30% of total index time.
- Zero outbound HTTP calls when privacy gates are disabled.
- Local model materialization happens lazily on first semantic use, not during install.

### Readiness Baseline Update (2026-02-25)

Config now includes a typed semantic sub-structure (`search.semantic`) with canonical normalization for `semantic_mode` and profile selection, while keeping legacy-compatible parsing behavior.
