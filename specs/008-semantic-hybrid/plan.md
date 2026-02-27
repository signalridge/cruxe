# Implementation Plan: Semantic/Hybrid Search

**Branch**: `008-semantic-hybrid` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md) | **Depends On**: 007-call-graph  
**Input**: Feature specification from `/specs/008-semantic-hybrid/spec.md`

## Summary

Introduce optional semantic/hybrid retrieval for `natural_language` intent while keeping
lexical-first behavior as the default and safety baseline. Execution is split into two tracks:

- **Track A (recommended first)**: `semantic_mode = rerank_only` (no vector index, lower complexity).
- **Track B (optional extension)**: `semantic_mode = hybrid` (embedded vectors + embeddings).

This plan optimizes the original design in five areas:

1. **Execution gating**: semantic path runs only when lexical confidence is insufficient.
2. **Stable identity**: vector records keyed by `symbol_stable_id` + `snippet_hash`, not line range.
3. **Privacy guardrails**: external providers blocked unless explicitly allowed.
4. **Confidence robustness**: low-confidence detection uses composite signals.
5. **Model lifecycle**: explicit embedding model versioning and re-embed policy.
6. **Intent observability**: emit `query_intent_confidence` + escalation hints for agents.
7. **Profile guidance**: optional advisor recommends profile tiers without mutating config.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)  
**New Dependencies**:
- Track A: `reqwest` (external rerank API, optional path)
- Track B only: `fastembed` (local embedding/rerank runtime), optional pluggable vector backend adapter (`lancedb` feature-gated if needed)

**Storage**: Track A uses existing Tantivy + SQLite; Track B adds embedded vector persistence via local vector segment/table (adapter-pluggable)  
**Testing**: cargo test + fixture repos + stratified NL benchmark suite  
**Performance Goals**:
- hybrid path latency overhead < 200ms p95 (warm)
- embedding overhead < 30% indexing cost
- zero outbound provider calls when privacy gates are off

**Constraints**:
- semantic must remain opt-in
- no external service required for baseline install/use
- symbol/path/error intents remain lexical-only

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Symbol/path/error intents remain lexical-first |
| II. Single Binary Distribution | PASS | Local-first path requires no external service |
| III. Branch/Worktree Correctness | PASS | Vectors are ref-scoped and version-partitioned |
| IV. Incremental by Design | PASS | Re-embed only changed snippets |
| V. Agent-Aware Response Design | PASS | Semantic metadata + confidence guidance in response |
| VI. Fail-Soft Operation | PASS | Any semantic/rerank failure falls back to lexical/local |
| VII. Explainable Ranking | PASS | Metadata exposes trigger/skip, ratio used, rerank provider |

## Architecture Decisions (Optimized)

### 1) Semantic Trigger Policy

Semantic branch executes only when all are true:

- `semantic_mode = hybrid`
- query intent is `natural_language`
- lexical top confidence is below `lexical_short_circuit_threshold`

Otherwise, semantic branch is skipped and response metadata includes:

- `semantic_triggered: false`
- `semantic_skipped_reason: <reason>`

### 2) Ratio Policy

`semantic_ratio` is treated as a **cap**, not a forced fixed blend. Runtime computes
`semantic_ratio_used` dynamically:

- high lexical confidence -> lower semantic weight (possibly `0.0`)
- low lexical confidence -> up to configured cap

### 3) Vector Identity and Dedup

Vector record identity:

`(project_id, ref, symbol_stable_id, snippet_hash, embedding_model_version)`

Hybrid merge dedup key is `symbol_stable_id` first, then fallback keys only when symbol
identity is unavailable.

### 4) Model Versioning

Each vector record stores:

- `embedding_model_id`
- `embedding_model_version`
- `embedding_dimensions`

Similarity scoring must not mix vectors from different model versions.

### 5) External Provider Guardrails

External embedding/rerank paths are allowed only when:

- `external_provider_enabled = true`
- `allow_code_payload_to_external = true`

Defaults are `false`, enforcing local-only behavior by default.

## Source Code Changes

```text
crates/
├── cruxe-core/
│   └── src/
│       ├── config.rs                # UPDATE: semantic gating/privacy/version config
│       └── types.rs                 # UPDATE: hybrid/confidence/provenance metadata types
├── cruxe-state/
│   └── src/
│       ├── vector_index.rs          # NEW (Track B): embedded vector CRUD keyed by stable symbol identity
│       └── embedding.rs             # NEW (Track B): fastembed-first provider abstraction + profile support
├── cruxe-indexer/
│   └── src/
│       ├── embed_writer.rs          # NEW (Track B): embedding generation/storage pipeline
│       └── writer.rs                # UPDATE: incremental re-embed on changed snippets (Track B)
├── cruxe-query/
│   └── src/
│       ├── hybrid.rs                # NEW (Track B): semantic trigger + weighted hybrid fusion
│       ├── rerank.rs                # NEW: provider trait + fail-soft fallback
│       ├── confidence.rs            # NEW: composite confidence evaluation
│       ├── search.rs                # UPDATE: pipeline orchestration + metadata emission
│       └── ranking.rs               # UPDATE: local reranker as provider-compatible fallback
└── cruxe-mcp/
    └── src/tools/search_code.rs     # UPDATE: request overrides + semantic metadata exposure

configs/
└── default.toml                     # UPDATE: semantic/privacy/trigger/profile defaults
```

## Configuration Model

```toml
[semantic]
mode = "off" # off | rerank_only | hybrid
ratio = 0.3
lexical_short_circuit_threshold = 0.85
confidence_threshold = 0.5
profile_advisor_mode = "off" # off | suggest
external_provider_enabled = false
allow_code_payload_to_external = false

[semantic.embedding]
profile = "fast_local" # fast_local | code_quality | high_quality | external
provider = "local"     # local | voyage | openai
model = "NomicEmbedTextV15Q"
model_version = "fastembed-1"
dimensions = 768
batch_size = 32

[semantic.rerank]
provider = "none"      # none | cohere | voyage
timeout_ms = 5000
```

## Rollout Strategy

1. **Stage A (safe baseline)**: `semantic_mode = rerank_only`, rerank provider `none` or local-only.
2. **Stage B (quality uplift)**: enable external rerank under explicit privacy flags.
3. **Stage C (optional hybrid)**: enable `semantic_mode = hybrid` and embedded vector path.
4. **Stage D (profile expansion)**: add `code_quality` / `high_quality` local profiles only when repo-bucket benchmarks prove gain.

## Complexity Tracking

- **fastembed**: accepted as primary Rust-native local embedding/rerank runtime.
- **Vector backend adapter**: keep default embedded local segment/table, optional LanceDB adapter behind feature flag when needed.
- **reqwest**: optional external path only; behind privacy and provider flags.

Primary complexity risk is lifecycle drift (model upgrades + vector compatibility). Mitigated by:

- explicit model version partitioning,
- background re-embed policy,
- strict no-cross-version similarity matching.
