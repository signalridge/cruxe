# Feature Specification: Semantic/Hybrid Search

**Feature Branch**: `008-semantic-hybrid`
**Created**: 2026-02-23
**Status**: Draft
**Phase**: 3 | **Version**: v1.2.0
**Depends On**: 007-call-graph
**Input**: User description: "Hybrid search toggle with local-first embedding backend, feature flags for semantic search, external rerank provider abstraction, confidence threshold and low-confidence guidance"

## Design Optimization Decisions (2026-02 Revision)

- Keep **lexical-first** as the default search path; semantic is additive for
  `natural_language` intent only.
- Treat `semantic_ratio` as a **maximum semantic weight**, not a forced fixed weight.
  The runtime may reduce it (including to `0.0`) when lexical confidence is already high.
- Use stable vector identity keys based on `symbol_stable_id`, not volatile line ranges.
- Require explicit privacy gates before sending code payloads to external providers.
- Use **composite confidence** (top score + score margin + channel agreement) instead of
  single-threshold top-score checks.
- Prioritize Rust-friendly local inference (`fastembed`) with tiered model profiles
  before any external embedding dependency.
- Add a **two-track rollout**:
  - Track A (minimal): lexical-first + optional external/local rerank + confidence guidance.
  - Track B (optional): vector hybrid (embedded vector index + embeddings) only when Track A quality is insufficient.

## User Scenarios & Testing

### User Story 1 - Semantic Search for Natural Language Queries (Priority: P1)

A developer or AI agent issues a natural language query like "where is authentication
handled" via `search_code`. The system detects `natural_language` intent and, if
semantic search is enabled, blends lexical results (Tantivy) with vector similarity
results (embedded vector store) using a configurable ratio cap. The blended results surface code
that is conceptually relevant even when exact keywords do not match. Symbol-intent
queries continue to use lexical search only.

**Why this priority**: Natural language queries are the weakest point of pure lexical
search. Semantic search directly addresses the most common agent query pattern.

**Independent Test**: Enable semantic search, index a fixture repo, search for
"handle user login" (a concept that maps to `authenticate_user` function), verify
the semantic match appears in results even though no keyword overlap exists.

**Acceptance Scenarios**:

1. **Given** semantic search is enabled with `semantic_ratio: 0.5`, **When**
   `search_code` is called with `"handle user login"` (natural_language intent),
   **Then** results include both lexical matches (keyword overlap) and semantic
   matches (conceptual similarity), blended by the configured ratio.
2. **Given** semantic search is enabled, **When** `search_code` is called with
   `"AuthHandler"` (symbol intent), **Then** only lexical search is used (semantic
   is not triggered for symbol queries).
3. **Given** semantic search is disabled (`semantic_mode: off`), **When** any
   query is executed, **Then** results are purely lexical, identical to pre-semantic
   behavior.
4. **Given** `semantic_ratio: 0.0`, **When** a natural language query is executed,
   **Then** results are purely lexical (semantic weight is zero).
5. **Given** semantic search is enabled, **When** lexical top results are already
   above `lexical_short_circuit_threshold`, **Then** semantic search is skipped for
   that query and metadata includes `semantic_triggered: false` with a skip reason.
6. **Given** query intent classification confidence is low, **When** `search_code`
   returns results, **Then** metadata includes `query_intent_confidence` and
   `intent_escalation_hint` so the agent can decide whether to reroute the query.

---

### User Story 2 - Configure Semantic Search Feature Flags (Priority: P1)

A developer configures semantic search behavior through `config.toml` or per-request
parameters. The system respects `semantic_mode` (global mode), `semantic_ratio`
(max blend weight), per-query-type overrides, and privacy controls for external
providers. Sensible defaults ensure zero configuration is needed for lexical-only
operation.

**Why this priority**: Feature flags are essential for gradual rollout and for users
who prefer pure lexical search or have resource constraints.

**Independent Test**: Set `semantic_mode: hybrid` and `semantic_ratio: 0.3` in config,
verify search behavior reflects these settings. Override ratio per-request and verify
the override takes effect.

**Acceptance Scenarios**:

1. **Given** `semantic_mode: off` in `config.toml` (the default), **When** the
   system starts, **Then** no vector index is created and no embeddings are generated.
2. **Given** `semantic_mode: hybrid` and `semantic_ratio: 0.3` in `config.toml`,
   **When** a natural language search is executed without per-request override, **Then**
   the blend uses up to 30% semantic weight (runtime may reduce it based on lexical confidence).
3. **Given** a per-request `semantic_ratio: 0.8`, **When** a search is executed,
   **Then** the per-request value overrides the config cap and sets the new runtime maximum.
4. **Given** per-query-type overrides in config (`natural_language.semantic_ratio: 0.6`,
   `symbol.semantic_ratio: 0.0`), **When** queries of each type are executed, **Then**
   each uses its configured ratio.
5. **Given** `external_provider_enabled: false` (default), **When** semantic embedding
   provider or rerank provider is configured as external, **Then** the query runs in
   local-only mode and no code payload is sent outbound.
6. **Given** semantic profile advisor mode is enabled, **When** `index_status` or
   semantic diagnostics are requested, **Then** the response includes a recommended
   profile (`fast_local` / `code_quality` / `high_quality`) with reason codes
   (repo size, language mix, latency budget).

---

### User Story 3 - External Rerank Provider (Priority: P2)

A developer enables an external reranking provider (e.g., Cohere Rerank v3) to improve
result quality. The system sends the query and candidate results to the provider, which
returns reranked scores. If the provider is unavailable or times out, the system falls
back to the local rule-based reranker without error.

**Why this priority**: External reranking can significantly improve result quality for
natural language queries, but must be optional and fail-soft.

**Independent Test**: Configure Cohere API key, execute a search query, verify results
are reranked by the provider. Remove the API key, verify fallback to local reranker
with no error surfaced to the user.

**Acceptance Scenarios**:

1. **Given** a Cohere API key is configured via `CODECOMPASS_RERANK_API_KEY` environment
   variable, **When** `search_code` is called with a natural language query, **Then**
   results are reranked by the external provider and scores reflect provider output.
2. **Given** no API key is configured, **When** `search_code` is called, **Then**
   the local rule-based reranker is used (default behavior, no change from current).
3. **Given** the external provider returns an error or times out (5s timeout), **When**
   `search_code` is called, **Then** the system falls back to local rule-based reranker,
   includes `rerank_fallback: true` in metadata, and does not surface the error.
4. **Given** the API key is configured, **When** the system logs, **Then** the API key
   is never included in log output, config dumps, or error messages.

---

### User Story 4 - Low-Confidence Guidance in Results (Priority: P2)

When search results have low confidence (composite score below a configurable threshold),
the system includes `low_confidence: true` in the response metadata along with a
`suggested_action` field. This enables AI agents to automatically adjust their search
strategy without needing to call `suggest_followup_queries` separately.

**Why this priority**: Inline confidence guidance reduces round-trips for agents,
making the search experience more efficient.

**Independent Test**: Execute a vague query that produces low-confidence results, verify
the response includes `low_confidence: true` and a non-empty `suggested_action`.

**Acceptance Scenarios**:

1. **Given** a search query where composite confidence is 0.2 and the confidence
   threshold is 0.5 (default), **When** results are returned, **Then** metadata
   includes `low_confidence: true` and `suggested_action` with a specific recommendation.
2. **Given** a search query where composite confidence is 0.8, **When** results are
   returned, **Then** metadata includes `low_confidence: false` and no
   `suggested_action`.
3. **Given** a per-request `confidence_threshold: 0.3`, **When** results are returned
   with composite confidence 0.35, **Then** `low_confidence: false` (above the custom threshold).
4. **Given** `suggested_action` is `"try locate_symbol with 'rate_limit'"`, **When**
   an agent follows the suggestion, **Then** the follow-up query should yield better
   results.

### Edge Cases

- What happens when vector index does not exist but semantic search is enabled?
  The system creates the embedded vector index on first semantic query (lazy initialization),
  logging a warning that initial query may be slow.
- What happens when the embedding model is unavailable?
  Semantic search is silently disabled for that query; results are purely lexical
  with `semantic_fallback: true` in metadata.
- What happens when `semantic_ratio` is set to a value outside 0.0-1.0?
  The value is clamped to the valid range and a warning is logged.
- What happens when the rerank provider returns fewer results than sent?
  Missing results retain their original lexical scores; the reranked subset is
  interleaved with the original scores.
- What happens when the vector index is out of sync with the Tantivy index?
  Embeddings are regenerated during `sync_repo` for changed files. If a document
  exists in Tantivy but not in vector store, it is treated as lexical-only.
- What happens when the embedding model produces different dimensions than expected?
  Index creation fails with a clear error message specifying expected vs actual
  dimensions. The system falls back to lexical-only.
- What happens when embedding model version changes?
  New vectors are written under the new `embedding_model_version`; similarity search
  is restricted to the same model version. Background re-embedding updates old records.
- What happens when external provider configuration is set but privacy gates are off?
  External calls are blocked, local-only path is used, and metadata indicates
  `external_provider_blocked: true`.

## Requirements

### Functional Requirements

- **FR-701**: System MUST support hybrid search blending lexical (Tantivy) and
  semantic (embedded vector store) results for `natural_language` query intent only.
- **FR-702**: System MUST NOT use semantic search for `symbol`, `path`, or `error` query
  intents; these remain lexical-first.
- **FR-703**: System MUST provide `semantic_mode` (`off` | `rerank_only` | `hybrid`,
  default: `off`) and `semantic_ratio` (f64, 0.0-1.0, default: 0.3) configuration
  options in `config.toml`, where `semantic_ratio` is treated as the maximum semantic contribution.
- **FR-704**: System MUST support per-request `semantic_ratio` override in `search_code`
  tool input.
- **FR-705**: System MUST support per-query-type `semantic_ratio` overrides in `config.toml`.
- **FR-706**: System MUST provide an embedded vector-store abstraction with no mandatory
  external service dependency. Default implementation uses local persisted storage
  (SQLite metadata + vector segment/table), with optional pluggable adapters.
- **FR-707**: System MUST generate vector embeddings using a configurable model
  provider (local first, external optional), with model selection in `config.toml`.
- **FR-708**: System MUST provide a `Rerank` trait abstraction:
  `Rerank(ctx, query, docs) -> Vec<(doc, score)>`.
- **FR-709**: System MUST implement at least one external rerank provider (Cohere Rerank v3
  or Voyage Rerank) behind the `Rerank` trait.
- **FR-710**: System MUST read rerank API keys from environment variables only
  (`CODECOMPASS_RERANK_API_KEY`), never from config files, and never log them.
- **FR-711**: System MUST fall back to local rule-based reranker when external provider
  is unavailable, times out (5s), or returns an error, with `rerank_fallback: true` in
  metadata.
- **FR-712**: System MUST include `low_confidence: true` and `suggested_action` in search
  response metadata when composite confidence is below the confidence threshold.
- **FR-713**: System MUST support a configurable `confidence_threshold` (default: 0.5) in
  `config.toml` and per-request.
- **FR-714**: System MUST regenerate embeddings for changed files during `sync_repo`
  incremental sync.
- **FR-715**: System MUST include `semantic_mode`, `semantic_enabled` (derived runtime
  flag), `semantic_ratio_used`, and `rerank_provider` in search response metadata for observability.
- **FR-716**: System MUST key vector records by stable identity
  `(repo, ref, symbol_stable_id, snippet_hash, embedding_model_version)` and MUST NOT
  use line-range-only identity for deduplication.
- **FR-717**: System MUST support lexical short-circuit behavior:
  if lexical confidence exceeds `lexical_short_circuit_threshold`, semantic search is skipped.
- **FR-718**: System MUST include `semantic_triggered` and optional `semantic_skipped_reason`
  in response metadata for semantic execution transparency.
- **FR-719**: System MUST gate outbound embedding/rerank calls behind
  `external_provider_enabled` and `allow_code_payload_to_external`, both default `false`.
- **FR-720**: System MUST store `embedding_model_id`, `embedding_model_version`, and
  `embedding_dimensions` with vector records and avoid cross-version similarity scoring.
- **FR-721**: Confidence evaluation MUST combine at least three signals:
  top score, top1-top2 margin, and lexical/semantic agreement.
- **FR-722**: System MUST provide two default semantic profiles:
  `fast_local` (small model, low latency) and `code_quality` (code-aware model, higher cost).
- **FR-723**: System MUST support `semantic_mode` with values:
  `off` (default), `rerank_only`, `hybrid`.
- **FR-724**: Under `semantic_mode = rerank_only`, vector index and embedding generation
  MUST be skipped; search remains lexical-first with optional rerank provider.
- **FR-725**: Local embedding profiles MUST map to concrete Rust-friendly model
  presets:
  - `fast_local`: `NomicEmbedTextV15Q` or `BGESmallENV15Q`
  - `code_quality`: `BGEBaseENV15Q` or `JinaEmbeddingsV2BaseCode`
  - `high_quality` (optional): `BGELargeENV15`, `GTELargeENV15`, `SnowflakeArcticEmbedL`
- **FR-726**: Default semantic profile promotion MUST be benchmark-gated by repo
  size bucket (`<10k`, `10k-50k`, `>50k` files) with latency, RSS, and MRR evidence.
- **FR-727**: Search responses MUST include `query_intent_confidence` (0.0-1.0)
  and optional `intent_escalation_hint` metadata when confidence is below threshold.
- **FR-728**: System MUST provide a profile advisor mode that recommends semantic
  profile tiers using repo-size bucket, language composition, and configured
  latency/resource budget, without automatically mutating active config.

### Key Entities

- **VectorIndex**: Embedded vector store containing code snippet embeddings, keyed
  by stable symbol identity + snippet hash + ref, with strict embedding model
  version partitioning (backend pluggable; local-first default).
- **EmbeddingModel**: A configurable model that converts code snippets into vector
  representations. Can be a local model (for example `BGE*`, `Nomic*`, `Jina*`)
  or external API.
- **RerankProvider**: An abstraction over external reranking services, with a trait
  interface and fail-soft behavior.
- **HybridResult**: A search result that combines lexical score, semantic score, and
  optionally reranked score, with provenance tracking.
- **ConfidenceGuidance**: Inline metadata in search responses indicating result
  confidence level and suggested follow-up actions, derived from composite confidence signals.
- **SemanticProfileAdvisor**: Diagnostic recommender that suggests an embedding
  profile (`fast_local`, `code_quality`, `high_quality`) with explicit reason codes.

## Success Criteria

### Measurable Outcomes

- **SC-701**: Hybrid search improves MRR (Mean Reciprocal Rank) by >= 15% over lexical-only
  for natural language queries on a benchmark set of >= 100 queries, stratified by language
  (Rust/TypeScript/Python/Go, >= 20 queries each).
- **SC-702**: Semantic search adds < 200ms to search latency (p95) for warm index queries.
- **SC-703**: External rerank provider timeout/fallback occurs transparently with zero
  user-visible errors in 100% of failure scenarios.
- **SC-704**: Embedded vector index size is < 2x the Tantivy index size for the same corpus.
- **SC-705**: Embedding generation during indexing adds < 30% to total index time.
- **SC-706**: With `external_provider_enabled: false` or
  `allow_code_payload_to_external: false`, outbound embedding/rerank HTTP calls are exactly zero.
- **SC-707**: Benchmarks are reported for all repo-size buckets (`<10k`, `10k-50k`,
  `>50k` files) before enabling non-`off` semantic defaults in any profile.
- **SC-708**: For queries with `query_intent_confidence >= 0.8`, top-3 precision
  is >= 85% on the stratified benchmark set.
- **SC-709**: Profile advisor recommendations are reproducible (same repo snapshot
  yields same recommendation) and deterministic across repeated runs.
