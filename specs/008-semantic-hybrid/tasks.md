# Tasks: Semantic/Hybrid Search

**Input**: Design documents from `/specs/008-semantic-hybrid/`
**Prerequisites**: plan.md (required), spec.md (required), research.md (required), contracts/mcp-tools.md (required)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US4)
- Include exact file paths in descriptions

## Phase 1: Configuration & Types

**Purpose**: Feature flags, config fields, new types for semantic search

- [ ] T364 [US2] Add semantic search config fields to `crates/codecompass-core/src/config.rs`: `semantic_mode` (`off` | `rerank_only` | `hybrid`, default `off`), `semantic_enabled` (derived convenience flag), `semantic_ratio` (f64, default: 0.3, treated as cap), `lexical_short_circuit_threshold` (f64, default: 0.85), `confidence_threshold` (f64, default: 0.5), `external_provider_enabled` (bool, default: false), `allow_code_payload_to_external` (bool, default: false), per-query-type overrides, embedding profile config, rerank provider config
- [ ] T365 [P] [US2] Add `[semantic]` section to `configs/default.toml`: all fields with defaults and inline documentation comments
- [ ] T366 [P] [US1] Add new types to `crates/codecompass-core/src/types.rs`: `HybridResult` (lexical_score, semantic_score, blended_score, provenance, semantic_triggered, semantic_skipped_reason), `ConfidenceGuidance` (low_confidence, suggested_action, threshold, top_score, score_margin, channel_agreement), `RerankResult` (doc, score, provider)
- [ ] T367 [US2] Write unit tests for config parsing: verify defaults, verify per-request override precedence, verify `semantic_ratio` clamping to 0.0-1.0, verify external provider calls are blocked when privacy gates are false

**Checkpoint**: Config system supports all semantic search settings

---

## Phase 2: Vector Store (Embedded Local Backend, `semantic_mode = hybrid` only)

**Purpose**: Embedded vector storage for code snippet embeddings

- [ ] T368 [US1] Add `fastembed` dependency to `crates/codecompass-state/Cargo.toml`; keep optional `lancedb` adapter feature-gated for advanced deployments (hybrid mode only)
- [ ] T369 [US1] Implement embedded vector index management in `crates/codecompass-state/src/vector_index.rs`: create store (lazy, on first use), insert vectors with stable metadata key (`project_id`, `ref`, `symbol_stable_id`, `snippet_hash`, `embedding_model_version`), query nearest neighbors with model-version filter, delete vectors by `symbol_stable_id`+ref, schema versioning (hybrid mode only)
- [ ] T370 [US1] Implement embedding abstraction in `crates/codecompass-state/src/embedding.rs`: `EmbeddingProvider` trait with model id/version/dimension handshake, local `fastembed` implementation, external API adapters (Voyage/OpenAI), model profile selection (`fast_local`/`code_quality`/`high_quality`) (hybrid mode only)
- [ ] T371 [US1] Implement local profile presets in `crates/codecompass-state/src/embedding.rs`: map config profiles to concrete models (`NomicEmbedTextV15Q`, `BGESmallENV15Q`, `BGEBaseENV15Q`, `JinaEmbeddingsV2BaseCode`, optional high-quality models) and validate dimensions at startup (hybrid mode only)
- [ ] T372 [P] [US1] Implement external embedding API client in `crates/codecompass-state/src/embedding.rs`: support Voyage Code 2 and OpenAI embedding endpoints, read API key from `CODECOMPASS_EMBEDDING_API_KEY` env var, enforce `external_provider_enabled && allow_code_payload_to_external` gates before outbound requests (hybrid mode only)
- [ ] T373 [US1] Write unit tests for vector index: insert/query/delete by stable symbol key, verify model-version partitioning, verify schema versioning, verify graceful fallback when optional adapter is unavailable (hybrid mode only)
- [ ] T374 [P] [US1] Write unit tests for embedding profiles: verify output dimensions match config, verify profile->model mapping, verify batch processing and cache behavior (hybrid mode only)

**Checkpoint**: Embedded vector index can store and query code embeddings

---

## Phase 3: Embedding Pipeline (Indexing Integration, `semantic_mode = hybrid` only)

**Purpose**: Generate and store embeddings during the indexing pipeline

- [ ] T375 [US1] Implement embedding writer in `crates/codecompass-indexer/src/embed_writer.rs`: accept code snippets with `symbol_stable_id`, generate embeddings via `EmbeddingProvider`, batch-write to embedded vector index with model version metadata, skip unless `semantic_mode = hybrid`
- [ ] T376 [US1] Integrate embedding writer into indexing pipeline in `crates/codecompass-indexer/src/writer.rs`: after snippet extraction, call embed_writer for each snippet when semantic is enabled
- [ ] T377 [US1] Update incremental sync in `crates/codecompass-indexer/src/writer.rs`: when a file changes, delete old embeddings by stable symbol key for that ref+model version before re-generating
- [ ] T378 [US1] Write integration test: enable semantic search, index a fixture repo, verify embedded vector store contains embeddings for all indexed snippets with correct metadata
- [ ] T379 [P] [US1] Write performance benchmark: index a 1,000-file fixture repo with and without semantic enabled, verify embedding overhead < 30%

**Checkpoint**: Indexing pipeline produces embeddings when semantic search is enabled

---

## Phase 4: Hybrid Search Blending (`semantic_mode = hybrid` only)

**Purpose**: Combine lexical and semantic search results

- [ ] T380 [US1] Implement hybrid search blending in `crates/codecompass-query/src/hybrid.rs`: accept lexical results (from Tantivy) and semantic results (from embedded vector store), compute weighted RRF scores using runtime `semantic_ratio_used` (capped by config), merge and deduplicate by `symbol_stable_id` (fallback to path+line only when symbol id absent), and cap per-branch fan-out
- [ ] T381 [US1] Implement semantic query in `crates/codecompass-query/src/hybrid.rs`: embed the query text using `EmbeddingProvider`, query vector store for nearest neighbors scoped to current ref and embedding model version, return ranked results with cosine similarity scores
- [ ] T382 [US1] Integrate hybrid search into `crates/codecompass-query/src/search.rs`: when `semantic_mode = hybrid` and intent is `natural_language`, apply lexical short-circuit policy (`lexical_short_circuit_threshold`) before running semantic branch; use adaptive `semantic_ratio_used` up to configured cap; for other intents, use lexical only
- [ ] T383 [US1] Update `search_code` response to include semantic metadata in `crates/codecompass-query/src/search.rs`: `semantic_enabled`, `semantic_ratio_used`, `semantic_triggered`, `semantic_skipped_reason`, `result_provenance` (lexical/semantic/hybrid per result), `embedding_model_version`
- [ ] T384 [US1] Write integration test: enable semantic search, index fixture repo, search for conceptual query (no keyword overlap), verify semantic match appears in results
- [ ] T385 [P] [US1] Write unit test for RRF blending: verify ratio=0.0 gives lexical-only, ratio=1.0 allows semantic-dominant ranking when semantic branch is triggered, ratio=0.5 gives balanced blend

**Checkpoint**: Natural language queries use hybrid search; symbol queries remain lexical

---

## Phase 5: Rerank Provider Abstraction

**Purpose**: External reranking with fail-soft fallback

- [ ] T386 [US3] Implement `Rerank` trait in `crates/codecompass-query/src/rerank.rs`: `async fn rerank(&self, query: &str, docs: &[RerankDocument], top_n: usize) -> Result<Vec<RerankResult>>`
- [ ] T387 [US3] Implement local rule-based reranker as `Rerank` trait impl in `crates/codecompass-query/src/rerank.rs`: wrap existing `ranking.rs` logic behind the trait
- [ ] T388 [US3] Implement Cohere Rerank v3 provider in `crates/codecompass-query/src/rerank.rs`: HTTP client using `reqwest`, read API key from `CODECOMPASS_RERANK_API_KEY` env var, enforce external-provider privacy gates, 5s timeout, parse response
- [ ] T389 [P] [US3] Implement Voyage Rerank provider in `crates/codecompass-query/src/rerank.rs`: similar structure to Cohere, different API format
- [ ] T390 [US3] Implement fail-soft fallback in `crates/codecompass-query/src/rerank.rs`: on provider error/timeout/policy block, fall back to local reranker, set `rerank_fallback: true` and include fallback reason in metadata
- [ ] T391 [US3] Integrate reranking into search pipeline in `crates/codecompass-query/src/search.rs`: after hybrid blending, apply reranker if configured, include `rerank_provider` in metadata
- [ ] T392 [US3] Write unit test: verify `Rerank` trait dispatch to correct provider based on config
- [ ] T393 [P] [US3] Write unit test: verify fail-soft fallback when provider returns error, verify API key is never logged
- [ ] T394 [US3] Write integration test: configure mock rerank endpoint, verify results are reranked, verify fallback on timeout

**Checkpoint**: External reranking works with fail-soft fallback

---

## Phase 6: Confidence Guidance

**Purpose**: Inline low-confidence detection and suggested actions

- [ ] T395 [US4] Implement composite confidence logic in `crates/codecompass-query/src/confidence.rs`: combine top result score, top1-top2 margin, and lexical/semantic agreement, compare against threshold, generate `ConfidenceGuidance` with `low_confidence` flag and `suggested_action` string
- [ ] T396 [US4] Implement suggested action generation in `crates/codecompass-query/src/confidence.rs`: for `natural_language` intent with low confidence, suggest `locate_symbol` with extracted identifiers; for `symbol` intent with low confidence, suggest broader `search_code`; for zero results, suggest alternative query formulations
- [ ] T397 [US4] Integrate confidence guidance into search response in `crates/codecompass-query/src/search.rs`: after ranking, compute confidence guidance, include in response metadata
- [ ] T398 [US4] Update `search_code` MCP tool to include confidence metadata in `crates/codecompass-mcp/src/tools/search_code.rs`: add `low_confidence`, `suggested_action`, `confidence_threshold`, `top_score`, `score_margin`, `channel_agreement` to response
- [ ] T399 [US4] Write unit test: verify low_confidence=true when top score < threshold, false when above
- [ ] T400 [P] [US4] Write unit test: verify suggested_action content for each query intent type
- [ ] T457 [US4] Extend intent classifier output in `crates/codecompass-query/src/intent.rs` and `crates/codecompass-query/src/search.rs` to emit `query_intent_confidence` + `intent_escalation_hint` metadata for low-confidence classifications
- [ ] T458 [P] [US4] Add integration tests for intent-confidence metadata: verify low confidence emits escalation hints and high confidence suppresses them

**Checkpoint**: Search responses include inline confidence guidance

---

## Phase 7: MCP Tool Updates

**Purpose**: Update search_code tool contract with new parameters

- [ ] T401 Update `search_code` input schema in `crates/codecompass-mcp/src/tools/search_code.rs`: add optional `semantic_ratio` and `confidence_threshold` per-request parameters and validate semantic overrides against runtime policy gates
- [ ] T402 Update MCP `tools/list` response to include new parameters in `search_code` tool schema
- [ ] T403 Write integration test: call `search_code` via MCP with `semantic_ratio` override, verify override is applied
- [ ] T404 [P] Write integration test: call `search_code` via MCP, verify confidence metadata is included in response, and verify semantic/rerank hard-failure paths preserve canonical error envelope and codes from `specs/meta/protocol-error-codes.md` (non-fail-soft cases only)

**Checkpoint**: MCP contract updated with semantic search parameters

---

## Phase 8: Polish & Validation

**Purpose**: End-to-end validation, performance benchmarking, documentation

- [ ] T405 Run full test suite (`cargo test --workspace`) and fix any failures
- [ ] T406 [P] Benchmark hybrid search latency: measure p95 for natural language queries with semantic enabled vs disabled across repo-size buckets (`<10k`, `10k-50k`, `>50k` files), verify overhead < 200ms
- [ ] T407 [P] Benchmark embedded vector index size: verify < 2x Tantivy index size for the same corpus
- [ ] T408 [P] Create benchmark query set: >= 100 natural language queries with known relevant results, stratified by language (Rust/TypeScript/Python/Go, >= 20 each) for MRR measurement
- [ ] T409 Run MRR benchmark: measure hybrid search MRR vs lexical-only MRR on stratified benchmark and report per repo-size bucket, target >= 15% improvement without regressing symbol-intent precision
- [ ] T410 Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [ ] T411 Run `cargo fmt --check --all` and fix formatting
- [ ] T459 Add reproducible benchmark-kit harness in `benchmarks/semantic/`: pin fixture commits + query pack version, produce deterministic reports for reruns
- [ ] T460 [P] Implement semantic profile advisor in `crates/codecompass-query/src/semantic_advisor.rs`: recommend `fast_local`/`code_quality`/`high_quality` based on repo-size bucket, language mix, and target latency budget
- [ ] T461 [P] Add advisor determinism tests + docs: same snapshot must yield same recommendation across repeated runs

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1** (Config & Types): No new dependencies - foundational
- **Phase 2** (Vector Store): Depends on Phase 1; required only for `semantic_mode = hybrid`
- **Phase 3** (Embedding Pipeline): Depends on Phase 2; required only for `semantic_mode = hybrid`
- **Phase 4** (Hybrid Search): Depends on Phase 3; required only for `semantic_mode = hybrid`
- **Phase 5** (Rerank): Depends on Phase 1; can run without Phases 2-4 (`rerank_only` path)
- **Phase 6** (Confidence): Depends on Phase 5 and search pipeline wiring (Phase 4 if hybrid, otherwise lexical + rerank signals)
- **Phase 7** (MCP Updates): Depends on Phases 5 and 6, plus Phase 4 only when hybrid metadata is exposed
- **Phase 8** (Polish): Depends on selected execution track

### Parallel Opportunities

- Phase 1: T365 and T366 can run in parallel
- Phase 2: T372 and T374 can run in parallel with T371
- Phase 3: T379 can run in parallel after T378
- Phase 4: T385 can run in parallel after T384
- Phase 5: T389 and T393 can run in parallel; Phase 5 can run in parallel with Phases 3-4
- Phase 6: Can begin after Phase 4 and run in parallel with late Phase 5 validation tasks
- Phase 7: T404 can run in parallel with T403
- Phase 8: T406, T407, T408 can run in parallel

## Implementation Strategy

### Incremental Delivery

1. Phase 1 -> Config system ready (feature-flagged, defaults to off)
2. **Track A (minimal, recommended first)**: Phase 5 -> Phase 6 -> Phase 7 -> Phase 8 (`semantic_mode = rerank_only`)
3. **Track B (optional quality extension)**: Phase 2 -> Phase 3 -> Phase 4 -> Phase 8 (`semantic_mode = hybrid`)

## Notes

- Total: 53 tasks, 8 phases
- Semantic path defaults to OFF - existing behavior preserved
- Rerank-only track avoids vector indexing complexity and should be delivered first unless benchmark evidence requires hybrid
- Local model materialization happens lazily on first semantic use, not during install
