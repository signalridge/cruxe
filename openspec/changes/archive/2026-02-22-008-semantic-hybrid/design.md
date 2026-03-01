## Context

Introduce optional semantic/hybrid retrieval for `natural_language` intent while keeping lexical-first behavior as the default and safety baseline. Execution is split into two tracks:

- **Track A (recommended first)**: `semantic_mode = rerank_only` (no vector index, lower complexity).
- **Track B (optional extension)**: `semantic_mode = hybrid` (embedded vectors + embeddings).

This plan optimizes the original design in seven areas: execution gating (semantic path runs only when lexical confidence is insufficient), stable identity (vector records keyed by `symbol_stable_id` + `snippet_hash`), privacy guardrails (external providers blocked unless explicitly allowed), confidence robustness (composite signals), model lifecycle (explicit embedding model versioning and re-embed policy), intent observability (`query_intent_confidence` + escalation hints), and profile guidance (optional advisor recommends tiers without mutating config).

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

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Symbol/path/error intents remain lexical-first |
| II. Single Binary Distribution | PASS | Local-first path requires no external service |
| III. Branch/Worktree Correctness | PASS | Vectors are ref-scoped and version-partitioned |
| IV. Incremental by Design | PASS | Re-embed only changed snippets |
| V. Agent-Aware Response Design | PASS | Semantic metadata + confidence guidance in response |
| VI. Fail-Soft Operation | PASS | Any semantic/rerank failure falls back to lexical/local |
| VII. Explainable Ranking | PASS | Metadata exposes trigger/skip, ratio used, rerank provider |

## Goals / Non-Goals

**Goals:**
1. Add optional semantic/hybrid retrieval for `natural_language` intent with lexical-first default.
2. Gate semantic execution so it runs only when lexical confidence is insufficient.
3. Use stable symbol identity keys for vector records, not volatile line ranges.
4. Enforce privacy guardrails: external providers blocked unless explicitly allowed.
5. Use composite confidence signals for robust low-confidence detection.
6. Support explicit embedding model versioning with re-embed policy.
7. Emit intent confidence and escalation hints for agent observability.
8. Provide profile advisor that recommends tiers without mutating config.

**Non-Goals:**
1. Dynamic or online policy training for semantic routing.
2. User-specific personalization of search behavior.
3. Mandatory external service dependency for baseline operation.
4. Automatic promotion of semantic profiles without benchmark evidence.

## Decisions

### D1. Semantic Trigger Policy

Semantic branch executes only when all are true:

- `semantic_mode = hybrid`
- query intent is `natural_language`
- lexical top confidence is below `lexical_short_circuit_threshold`

Otherwise, semantic branch is skipped and response metadata includes:

- `semantic_triggered: false`
- `semantic_skipped_reason: <reason>`

**Why:** preserves lexical-first behavior and avoids unnecessary compute for high-confidence or non-NL queries.

### D2. Ratio Policy

`semantic_ratio` is treated as a **cap**, not a forced fixed blend. Runtime computes `semantic_ratio_used` dynamically:

- high lexical confidence -> lower semantic weight (possibly `0.0`)
- low lexical confidence -> up to configured cap

**Why:** avoids diluting strong lexical results with weak semantic signals.

### D3. Vector Identity and Dedup

Vector record identity:

`(project_id, ref, symbol_stable_id, snippet_hash, embedding_model_version)`

Hybrid merge dedup key is `symbol_stable_id` first, then fallback keys only when symbol identity is unavailable.

**Why:** stable identity avoids churn from line-range shifts and enables reliable incremental updates.

### D4. Model Versioning

Each vector record stores:

- `embedding_model_id`
- `embedding_model_version`
- `embedding_dimensions`

Similarity scoring must not mix vectors from different model versions.

**Why:** prevents silent quality degradation when models change and enables safe background re-embedding.

### D5. External Provider Guardrails

External embedding/rerank paths are allowed only when:

- `external_provider_enabled = true`
- `allow_code_payload_to_external = true`

Defaults are `false`, enforcing local-only behavior by default.

**Why:** explicit privacy gates prevent accidental code exfiltration to third-party services.

### D6. Configuration Model

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

**Why:** typed semantic sub-structure with sensible defaults ensures zero-config lexical-only operation while providing full control for semantic rollout.

### D7. Source Code Changes

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

**Why:** changes are organized by track (A vs B) to support incremental delivery; new files are isolated modules behind the semantic feature surface.

## Risks / Trade-offs

- **[Risk] Model lifecycle drift (model upgrades + vector compatibility)** -> **Mitigation:** explicit model version partitioning, background re-embed policy, strict no-cross-version similarity matching.
- **[Risk] fastembed dependency adds binary size and build complexity** -> **Mitigation:** accepted as primary Rust-native local embedding/rerank runtime; feature-gated for hybrid mode only.
- **[Risk] Vector backend adapter complexity** -> **Mitigation:** keep default embedded local segment/table, optional LanceDB adapter behind feature flag when needed.
- **[Risk] reqwest dependency for external path** -> **Mitigation:** optional external path only; behind privacy and provider flags.

## Migration Plan

1. **Stage A (safe baseline)**: `semantic_mode = rerank_only`, rerank provider `none` or local-only.
2. **Stage B (quality uplift)**: enable external rerank under explicit privacy flags.
3. **Stage C (optional hybrid)**: enable `semantic_mode = hybrid` and embedded vector path.
4. **Stage D (profile expansion)**: add `code_quality` / `high_quality` local profiles only when repo-bucket benchmarks prove gain.

### Incremental Delivery

1. Phase 1 -> Config system ready (feature-flagged, defaults to off)
2. **Track A (minimal, recommended first)**: Phase 5 -> Phase 6 -> Phase 7 -> Phase 8 (`semantic_mode = rerank_only`)
3. **Track B (optional quality extension)**: Phase 2 -> Phase 3 -> Phase 4 -> Phase 8 (`semantic_mode = hybrid`)

Rollback: set `semantic_mode = off` in config to restore pure lexical behavior. Semantic defaults to OFF -- existing behavior preserved. Rerank-only track avoids vector indexing complexity and should be delivered first unless benchmark evidence requires hybrid. Local model materialization happens lazily on first semantic use, not during install.
