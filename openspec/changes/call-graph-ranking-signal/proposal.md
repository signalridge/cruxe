## Why

Current ranking ignores structural graph signals or relies mainly on call-edge quality. For universality, a signal that depends only on language-specific call extraction is fragile.

Cruxe already stores multiple relation types in `symbol_edges` (calls/imports/references depending on extraction stage). We can derive a robust language-agnostic centrality signal from the **relation graph as a whole**, not just call graphs.

## What Changes

1. **Relation-graph centrality signal (universal)**
   - Compute file-level centrality from all resolved inter-file edges (`to_symbol_id IS NOT NULL`, non-self).
   - Edge-type agnostic in Phase 1.

2. **Tantivy materialization**
   - Store `file_centrality` as FAST field in symbol docs.

3. **Ranking integration**
   - Add bounded `centrality_boost` as query-independent tie-break signal.

4. **Explainability + safety rails**
   - Include centrality contribution in ranking explanation.
   - Keep bounded weight so centrality cannot overpower exact lexical relevance.

## Capabilities

### New Capabilities
- `relation-graph-ranking`: language-agnostic structural centrality ranking signal.

### Modified Capabilities
- `002-agent-protocol`: ranking explanation includes relation centrality contribution.

## Impact

- Affected crates: `cruxe-indexer`, `cruxe-state`, `cruxe-query`.
- API impact: additive explanation metadata only.
- Data impact: reindex required to populate centrality field.
