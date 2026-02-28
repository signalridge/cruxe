## Why

Applying the same retrieval pipeline to all query intents wastes latency budget and compute. Some queries need only lexical search, while others benefit from hybrid semantic reranking.

## What Changes

1. Add deterministic query-plan selection (`lexical_fast`, `hybrid_standard`, `semantic_deep`) based on intent and confidence.
2. Add per-plan runtime budgets (fanout, rerank depth, latency target) with fail-soft downgrades.
3. Expose selected plan and downgrade reason in metadata for observability.
4. Add configuration controls for plan thresholds and per-plan knobs.

## Capabilities

### New Capabilities
- `adaptive-query-plan`: intent-aware retrieval planning with deterministic budgeted execution profiles.

### Modified Capabilities
- `002-agent-protocol`: search metadata includes selected query plan and optional downgrade reasons.

## Impact

- Affected crates: `cruxe-query`, `cruxe-core`, `cruxe-mcp`.
- API impact: additive metadata fields only.
- Performance impact: lower average latency while preserving quality for high-ambiguity queries.
- Operational impact: easier diagnosis of why a query did or did not trigger expensive semantic stages.
