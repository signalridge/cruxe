## 1. Cross-change dependency gate (with `refactor-multilang-symbol-contract`)

- [x] 1.1 Confirm upstream dependency is satisfied: `refactor-multilang-symbol-contract` task 6.8 (semantic hit enrichment parity) is implemented or explicitly marked as prerequisite for this change.
- [x] 1.2 Add/verify code comments in semantic query path documenting that this change extends runtime quality contracts on top of upstream semantic enrichment.

## 2. Query-layer semantic quality metadata (cruxe-query)

- [x] 2.1 Extend `SearchMetadata` in `crates/cruxe-query/src/search.rs` with additive fields:
  - `semantic_degraded: bool`
  - `semantic_limit_used: usize`
  - `lexical_fanout_used: usize`
  - `semantic_fanout_used: usize`
  - `semantic_budget_exhausted: bool`
- [x] 2.2 Set `semantic_degraded` deterministically as `semantic_fallback`.
- [x] 2.3 Populate `*_used` fields from effective runtime budget values actually applied in search execution.
- [x] 2.4 Populate `semantic_budget_exhausted` when semantic branch hits effective fanout cap and may be recall-limited.

## 3. Protocol metadata surface (cruxe-mcp)

- [x] 3.1 Extend `ProtocolMetadata` in `crates/cruxe-mcp/src/protocol.rs` with additive optional fields:
  - `semantic_degraded`
  - `semantic_limit_used`
  - `lexical_fanout_used`
  - `semantic_fanout_used`
  - `semantic_budget_exhausted`
- [x] 3.2 Map new `SearchMetadata` fields into protocol payload in `crates/cruxe-mcp/src/server/tool_calls/query.rs`.
- [x] 3.3 Keep backward compatibility: existing semantic fields remain unchanged; new fields are optional/omittable.

## 4. Budget clamp contract (cruxe-query + cruxe-core)

- [x] 4.1 Update `semantic_fanout_limits` (`crates/cruxe-query/src/search.rs`) to apply explicit caps:
  - `semantic_limit <= 1000`
  - `lexical_fanout <= 2000`
  - `semantic_fanout <= 1000`
- [x] 4.2 Preserve existing floors:
  - `semantic_limit >= 20`
  - `lexical_fanout >= 40`
  - `semantic_fanout >= 30`
- [x] 4.3 Add/adjust tests to verify floor/cap behavior under extreme multipliers.

## 5. Deterministic fail-soft reasons and logging (cruxe-query)

- [x] 5.1 Keep lexical-only return path on semantic backend failure (no hard error).
- [x] 5.2 Ensure failure reason uses deterministic code set:
  - existing `semantic_backend_error`
  - reserved `semantic_backend_timeout`, `semantic_backend_unavailable` (documented for forward compatibility)
- [x] 5.3 Add structured warning logs with query/ref/project scope when semantic fallback happens.
- [x] 5.4 Extend/adjust tests around existing fallback behavior (`semantic_backend_error_sets_semantic_fallback_metadata`) to assert `semantic_degraded` semantics.

## 6. Scale-tier warnings for SQLite semantic vectors (cruxe-state + cruxe-query)

- [x] 6.1 Add helper to compute semantic vector count by `(project_id, ref)` from `semantic_vectors`.
- [x] 6.2 Emit non-fatal warning when count enters Tier 2 (`50k–200k`).
- [x] 6.3 Emit stronger warning with migration guidance (`search.semantic.embedding.vector_backend = "lancedb"`) when Tier 3 (`>200k`) is reached.
- [x] 6.4 Thread warnings into response metadata/warning channel without breaking query success.

## 7. Benchmark acceptance contract (cruxe-query example)

- [x] 7.1 Extend `crates/cruxe-query/examples/semantic_benchmark_eval.rs` report schema to include:
  - degraded-query rate
  - semantic budget-exhaustion rate
- [x] 7.2 Collect and aggregate these counters from `SearchResponse.metadata` over the query pack run.
- [x] 7.3 Document Tier-1 reference acceptance profile (p95 latency target 500ms + zero-result-rate/MRR evidence) in benchmark usage notes.

## 8. OpenSpec artifact coherence

- [x] 8.1 Keep `proposal.md`, `design.md`, `tasks.md`, and `specs/*` consistent on field names (`semantic_degraded`, not `semantic_available`).
- [x] 8.2 Ensure this change’s spec deltas include both:
  - capability-level runtime behavior (`specs/semantic-retrieval-quality/spec.md`)
  - protocol metadata contract (`specs/002-agent-protocol/spec.md`)

## 9. Verification and evidence

- [x] 9.1 Run targeted tests:
  - `cargo test -p cruxe-query semantic_backend_error_sets_semantic_fallback_metadata`
  - semantic fanout/cap tests added in this change
- [x] 9.2 Run protocol-level tests for MCP metadata serialization changes.
- [x] 9.3 Run OpenSpec validation:
  - `openspec status --change semantic-retrieval-quality-contract`
  - `openspec validate semantic-retrieval-quality-contract`
- [x] 9.4 Record command outputs and key metrics as implementation evidence in change notes.

## Recommended implementation order

1 (dependency gate) → 2 (query metadata) → 3 (protocol metadata) → 4 (budget caps) → 5 (fail-soft reasons) → 6 (scale warnings) → 7 (benchmark contract) → 8/9 (artifact + validation)

## Implementation evidence (2026-02-28)

- `cargo test --workspace --no-run` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test -p cruxe-query semantic_backend_error_sets_semantic_fallback_metadata -- --nocapture` ✅
- `cargo test -p cruxe-query semantic_fanout_limits_apply_caps -- --nocapture` ✅
- `cargo test -p cruxe-mcp t403_search_code_exposes_semantic_and_confidence_metadata -- --nocapture` ✅
- `openspec status --change semantic-retrieval-quality-contract --json` ✅
- `openspec validate semantic-retrieval-quality-contract` ✅
