## Why

Relation-graph and import-resolution improvements increase structural signal usage, but not all edges are equally trustworthy. Treating all edges equally can amplify noisy or unresolved links and distort ranking.

## What Changes

1. Introduce a confidence model for relation edges (provider/source/outcome aware).
2. Weight centrality and graph-derived ranking signals by edge confidence instead of raw edge count.
3. Persist edge confidence/provenance metadata for deterministic explainability.
4. Add confidence-aware quality checks to avoid over-promoting low-confidence graph hubs.

## Capabilities

### New Capabilities
- `edge-confidence-weighting`: confidence-aware structural signal computation for graph-derived ranking factors.

### Modified Capabilities
- `symbol-contract-v2`: relation edge records include confidence/provenance semantics and deterministic defaults for unresolved/external outcomes.

## Impact

- Affected crates: `cruxe-indexer`, `cruxe-state`, `cruxe-query`.
- Data impact: schema/index updates for edge confidence fields; reindex required.
- API impact: additive explain metadata for confidence-weighted structural boosts.
- Quality impact: reduces noisy graph dominance in ranking.
