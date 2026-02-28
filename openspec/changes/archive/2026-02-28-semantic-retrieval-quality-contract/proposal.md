## Why

Cruxe already has lexical+semantic hybrid retrieval, but still lacks a **runtime quality contract** for semantic behavior. The previous change (`refactor-multilang-symbol-contract`) addresses semantic hit enrichment parity (task 6.8). This new change focuses on the missing operational guarantees:

1. **Degraded-state signaling gap**
   - Semantic fallback exists internally, but clients must infer failure from multiple fields.
   - There is no normalized `semantic_degraded` signal for agent behavior.

2. **Budget contract gap**
   - Fanout/limit are multiplier-based but uncapped.
   - Effective values are not exposed, so diagnosing recall/latency regressions is difficult.

3. **Fail-soft observability gap**
   - Fail-soft lexical fallback exists, but deterministic failure semantics and structured runtime observability are incomplete.

4. **Scale contract gap**
   - SQLite vector path documents ~50k degradation in code comments, but there is no tiered contract (warning thresholds + migration guidance) visible to users.

5. **Release acceptance gap**
   - Semantic benchmark harness exists, but degraded-query and budget-exhaustion rates are not first-class acceptance evidence.

Competitive implementation evidence (validated via local `ghq` clones):
- `zilliztech/claude-context`: explicit indexing/degraded status state machine and partial-operation messaging.
- `elastic/semantic-code-search-*`: bounded candidate scan and split storage model (`<index>` + `<index>_locations`).
- `iceinvein/code_intelligence_mcp_server`: deterministic keyword-only degradation on vector/embed failures.
- `shinpr/mcp-local-rag`: bounded over-fetch + rerank (`candidate_multiplier`) with explicit hybrid weight controls.
- `yoanbernabeu/grepai`: deterministic config-driven RRF + path penalty/bonus policy.

## What Changes

1. **Semantic degraded-state normalization**
- Add additive metadata field: `semantic_degraded`.
- Define deterministic rule: `semantic_degraded = semantic_fallback`.
- Keep existing semantic metadata fields for backward compatibility.

2. **Hybrid budget bounds + visibility**
- Keep multiplier inputs, but enforce explicit floors/caps:
  - floors: `semantic_limit>=20`, `lexical_fanout>=40`, `semantic_fanout>=30`
  - caps: `semantic_limit<=1000`, `lexical_fanout<=2000`, `semantic_fanout<=1000`
- Expose effective runtime values:
  - `semantic_limit_used`, `lexical_fanout_used`, `semantic_fanout_used`, `semantic_budget_exhausted`.

3. **Fail-soft deterministic contract**
- On semantic backend failure, always return lexical results (no hard error).
- Set deterministic reason codes (starting with `semantic_backend_error`; reserved timeout/unavailable variants).
- Emit structured warning logs with query/ref/project context.

4. **Scale-tier contract for SQLite vectors**
- Tier 1 `<50k`: supported baseline.
- Tier 2 `50k–200k`: degraded-latency warning tier.
- Tier 3 `>200k`: unsupported-for-SLO warning tier + ANN migration guidance (`lancedb`).

5. **Benchmark-gated acceptance profile**
- Extend `semantic_benchmark_eval` report with:
  - degraded-query rate,
  - semantic budget-exhaustion rate.
- Keep p95 500ms as **reference benchmark target** for Tier-1 profile (not hard runtime assertion).

## Capabilities

### Added Capabilities
- `semantic-retrieval-quality`: runtime degraded-state semantics, budget contract, scale-tier warnings, and benchmark acceptance contract.

### Modified Capabilities
- `002-agent-protocol`: additive semantic quality metadata fields (`semantic_degraded`, `*_fanout_used`, `semantic_budget_exhausted`).

## Impact

- Affected crates:
  - `cruxe-query` (`search.rs`, hybrid execution metadata, budget clamp logic)
  - `cruxe-mcp` (`protocol.rs`, metadata serialization in tool handlers)
  - `cruxe-state` (vector-count-based tier warnings)
- API impact: additive metadata only (backward-compatible).
- Data/index impact: none required for this contract change.
- Dependency: this change assumes `refactor-multilang-symbol-contract` task 6.8 semantic enrichment parity as upstream prerequisite.
