## Context

`refactor-multilang-symbol-contract` already fixes the **ranking side** of semantic parity via task 6.8:

- semantic-only hits are enriched by `symbol_stable_id` lookup before final ranking,
- `kind_weight` / `query_intent_boost` / `test_file_penalty` become applicable to semantic hits.

This change does **not** duplicate that work. It defines the missing runtime contracts around semantic retrieval quality:

1. explicit degraded-mode semantics,
2. hybrid candidate budget bounds and visibility,
3. scale tier boundaries for SQLite vector search,
4. lightweight observability and benchmark gates.

Code evidence in current repo:

- Hybrid flow and fallback: `crates/cruxe-query/src/search.rs:263-314`
- Semantic branch: `crates/cruxe-query/src/hybrid.rs:22-98`
- Fanout calculation: `crates/cruxe-query/src/search.rs:611-622`
- SQLite vector scaling note: `crates/cruxe-state/src/vector_index.rs:601-604`
- Existing semantic metadata fields: `crates/cruxe-mcp/src/protocol.rs:24-60`
- Benchmark harness: `crates/cruxe-query/examples/semantic_benchmark_eval.rs:48-62`

Competitive references that directly informed the design:

- Claude Context: dense+sparse hybrid with RRF and explicit indexing states
- Elastic SCS: bounded candidate loop + locations join model
- Zoekt: mature ranking discipline and low-priority file handling
- code_intelligence_mcp_server: fail-soft vector degradation strategy

### Competitive implementation matrix (repo-level evidence)

All references below were validated against local `ghq` clones for concrete implementation details (not marketing docs):

| Project | Concrete implementation | Contract signal adopted for Cruxe |
|---|---|---|
| `zilliztech/claude-context` | `packages/mcp/src/config.ts` defines explicit indexing states (`indexing`, `indexed`, `indexfailed`) with progress and failure payload; `packages/mcp/src/handlers.ts` returns user-visible status transitions; `packages/core/src/context.ts` enforces chunk limit (`CHUNK_LIMIT = 450000`) with `limit_reached` outcome. | Keep semantic fail-soft + user-visible degraded metadata explicit; surface quality-state transitions instead of silent fallback. |
| `elastic/semantic-code-search-mcp-server` + `elastic/semantic-code-search-indexer` | Two-index model: `<index>` stores content-deduplicated chunks, `<index>_locations` stores per-file locations joined by `chunk_id` (`src/utils/elasticsearch.ts` in indexer + `src/mcp_server/tools/semantic_code_search.ts` in MCP). Candidate scan bounded by `MAX_SEMANTIC_SEARCH_CANDIDATES = 5000`. | Preserve bounded candidate execution and explicit budget caps/metadata in Cruxe semantic branch. |
| `iceinvein/code_intelligence_mcp_server` | `src/retrieval/hybrid.rs` degrades to keyword-only on embedding/vector failures; dynamic RRF weights for NL intent (`keyword*0.5`, `vector*1.5`); config exposes `VECTOR_SEARCH_LIMIT`, `VECTOR_GUARANTEED_RESULTS`, `RRF_*`. | Adopt deterministic degraded semantics + observable budget contract; keep lexical-first fail-soft behavior. |
| `shinpr/mcp-local-rag` | `HYBRID_SEARCH_CANDIDATE_MULTIPLIER = 2` (`src/vectordb/types.ts`); prefetch-then-rerank flow in `src/vectordb/index.ts`; keyword boost formula in `src/vectordb/search-filters.ts`: `distance / (1 + normalized_keyword * weight)`. | Keep candidate over-fetch bounded, then apply lightweight reranking; expose effective budget and exhaustion state. |
| `yoanbernabeu/grepai` | Hybrid pipeline in `search/search.go` + `search/hybrid.go` uses RRF (`k` default 60) and fetch-limit expansion (`limit * 2`); path-based boost/penalty rules are explicit in `config/config.go` + `search/boost.go`. | Keep deterministic, low-complexity scoring knobs and config-backed defaults; avoid hidden heuristic branches. |

What this change adopts now vs later:

- **Adopt now**: explicit degraded flag, bounded/capped budgets, budget observability, scale-tier warnings, benchmark gates.
- **Defer**: dual-index storage split (`<index>` + `<index>_locations`) and ANN backend migration (kept out of scope for this contract-focused change).

---

## Goals

1. Keep lexical-first architecture intact while making semantic degradation explicit.
2. Make hybrid candidate budgeting configurable **and** observable.
3. Define scale tiers so users understand when SQLite vector mode is acceptable.
4. Add benchmark-level quality gates without introducing heavy infra requirements.

## Non-Goals

1. Replacing SQLite vector backend with ANN in this change.
2. Rewriting retrieval around Elastic-style dual index in this change.
3. Introducing a Prometheus/OpenTelemetry subsystem in this change.

---

## D1. Contract boundary with `refactor-multilang-symbol-contract`

### Decision

This change extends (not duplicates) semantic parity work:

- Upstream dependency: `refactor-multilang-symbol-contract` task 6.8 (semantic hit metadata enrichment)
- This change layers runtime quality contracts on top:
  - degraded mode signaling,
  - budget bound enforcement,
  - scale tier warnings,
  - benchmark acceptance.

### Rationale

If implemented in one giant change, failure diagnosis and rollback are harder. Splitting by concern keeps risk localized:

- change A: symbol contract + ranking semantics,
- change B: semantic runtime quality contracts.

---

## D2. Semantic degradation state model

### Decision

Introduce a normalized degraded signal in metadata:

- add `semantic_degraded: bool` (additive metadata field)
- derive as `semantic_fallback == true`

State interpretation:

| Runtime state | semantic_triggered | semantic_fallback | semantic_skipped_reason | semantic_degraded |
|---|---:|---:|---|---:|
| semantic_active | true | false | null | false |
| semantic_intentionally_skipped | false | false | semantic_disabled / intent_not_nl / mode_rerank_only / lexical_high_confidence / project_scope_unresolved / semantic_requires_state_connection | false |
| semantic_degraded_failure | false | true | semantic_backend_error (and future backend timeout/unavailable reasons) | true |

### Rationale

`semantic_fallback` already exists, but agents currently must infer degradation by combining multiple fields. A single boolean is a stable contract for clients while preserving backward compatibility.

---

## D3. Hybrid candidate budget contract

### Decision

Budgeting remains multiplier-based, but now has explicit clamps and exposed effective values:

- Inputs (existing config):
  - `search.semantic.semantic_limit_multiplier`
  - `search.semantic.lexical_fanout_multiplier`
  - `search.semantic.semantic_fanout_multiplier`
- Existing floors remain:
  - `semantic_limit >= 20`
  - `lexical_fanout >= 40`
  - `semantic_fanout >= 30`
- New caps (this change):
  - `semantic_limit <= 1000`
  - `lexical_fanout <= 2000`
  - `semantic_fanout <= 1000`

Expose effective budgets in response metadata:

- `semantic_limit_used`
- `lexical_fanout_used`
- `semantic_fanout_used`
- `semantic_budget_exhausted` (true when semantic branch returns at cap and may be recall-limited)

### Rationale

Without caps, accidental config values can explode latency/memory. Without exposed effective values, quality debugging is guesswork.

---

## D4. Fail-soft behavior contract

### Decision

When semantic branch fails at runtime (embedding/vector/backend path), query must:

1. return lexical results (no hard error),
2. set `semantic_fallback=true` and `semantic_degraded=true`,
3. set `semantic_skipped_reason` to a deterministic code,
4. emit structured warning logs with query/ref/project scope.

Deterministic reason set for failure path:

- `semantic_backend_error` (existing)
- reserved for extension:
  - `semantic_backend_timeout`
  - `semantic_backend_unavailable`

### Rationale

This codifies current fail-soft intent into a stable API contract for agents and reduces silent quality regressions.

---

## D5. Scale tier contract for SQLite vector mode

### Decision

Formalize three size tiers for `semantic_vectors` per `(project_id, ref)`:

- Tier 1 (< 50k vectors): supported baseline for SQLite brute-force cosine.
- Tier 2 (50k–200k): degraded-latency tier; must warn at index/search time.
- Tier 3 (> 200k): unsupported-for-SLO tier; strongly recommend ANN backend (`lancedb`).

Behavior:

- Tier 2+: emit warnings (non-fatal)
- Tier 3: warnings include explicit migration guidance (`search.semantic.embedding.vector_backend = "lancedb"`)

### Rationale

`vector_index.rs` already documents degradation risk. This change converts comment-level knowledge into runtime/user-visible contract.

---

## D6. Benchmark and acceptance contract

### Decision

Use existing `semantic_benchmark_eval` as acceptance gate source of truth.

Additions:

- include degraded-query rate and budget-exhaustion rate in report,
- document default acceptance profile for Tier 1 corpora:
  - p95 latency target: 500ms,
  - zero-result rate and MRR tracked as release evidence.

Important: the 500ms threshold is treated as **benchmark target under reference profile**, not a hard runtime assertion on all developer machines.

### Rationale

This avoids fragile machine-dependent runtime assertions while still enforcing measurable quality gates before release.

---

## D7. Interaction map (combined execution order)

Recommended execution order across two changes:

1. `refactor-multilang-symbol-contract` task 6.8 (semantic enrichment parity)
2. this change D2/D3 metadata + budget contract
3. this change D4 fail-soft deterministic reasons
4. this change D5 scale-tier warnings
5. this change D6 benchmark gate updates

Rollback safety:

- If new metadata fields cause client friction, fields are additive and can be omitted without data migration.
- Budget caps can be relaxed via config defaults in a patch release.

---

## D8. OpenSpec reference mapping for semantic-search implementation

To reduce implementation drift, map competitor patterns directly into Cruxe artifacts:

| Cruxe artifact | Reference pattern | Concrete mapping |
|---|---|---|
| `specs/semantic-retrieval-quality/spec.md` | code_intelligence + claude-context fail-soft/status | degraded-state semantics and deterministic reason codes |
| `specs/002-agent-protocol/spec.md` (delta in this change) | claude-context status + mcp-local-rag observability | additive metadata: `semantic_degraded`, `*_fanout_used`, `semantic_budget_exhausted` |
| `tasks.md` | elastic SCS bounded candidate loop | explicit cap/clamp implementation tasks and test assertions |
| benchmark acceptance section | grepai + mcp-local-rag configurable knobs | reproducible benchmark profile with measurable degraded/budget metrics |

This section is normative for implementation planning: if code and spec diverge, update spec deltas first, then implementation.

---

## Risks & Mitigations

- **Risk:** over-constraining budget caps hurts recall in large repos
  - **Mitigation:** expose `*_used` and exhaustion flags; tune caps with benchmark evidence.

- **Risk:** degraded signal misinterpreted as user misconfiguration
  - **Mitigation:** deterministic reason codes + warning logs with scope context.

- **Risk:** tier thresholds too strict/loose for mixed-language repos
  - **Mitigation:** thresholds are config-backed constants and can be revised without schema changes.
