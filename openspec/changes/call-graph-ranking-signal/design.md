## Context

Cruxe extracts structural relations but ranking still depends heavily on lexical/kind boosts. Existing call-graph-only framing is useful yet too tied to language-specific call extraction completeness.

A universal structural signal should:

- degrade gracefully when call extraction is partial,
- still work using other resolved edge types,
- remain query-independent and cheap.

## Goals / Non-Goals

**Goals**
1. Compute structural centrality from resolved inter-file relation edges.
2. Keep centrality computation O(|E|) and deterministic.
3. Integrate as bounded tie-break signal.

**Non-Goals**
1. Full graph-embedding or iterative PageRank in Phase 1.
2. Per-language centrality formulas.

## Decisions

### D1. Relation graph centrality (edge-type agnostic)

#### Decision

Compute file-level inbound centrality using all resolved inter-file edges:

```sql
SELECT sr.path AS target_file,
       COUNT(DISTINCT se.source_file) AS inbound_file_count
FROM symbol_edges se
JOIN symbol_relations sr ON sr.symbol_stable_id = se.to_symbol_id
WHERE se.to_symbol_id IS NOT NULL
  AND se.source_file != sr.path
GROUP BY sr.path
```

Normalize to `[0.0, 1.0]` by max inbound count.

#### Rationale

This remains robust even when call extraction is weaker for some languages.

### D2. Materialize centrality in index

#### Decision

Store `file_centrality` as FAST f64 in Tantivy symbols schema and populate during index write.

#### Rationale

Avoids query-time SQL joins and keeps ranking fast.

### D3. Bounded ranking contribution

#### Decision

`centrality_boost = file_centrality * CENTRALITY_WEIGHT`, with conservative default `CENTRALITY_WEIGHT = 1.0` (smaller than exact-match and major lexical boosts).

#### Rationale

Centrality should break ties, not dominate relevance.

### D4. Explainability

#### Decision

Expose `centrality_boost` in explain output with both raw centrality and weighted contribution.

#### Rationale

Maintain transparent ranking decomposition.

## Risks / Trade-offs

- **Risk: generated/shared utility files may be over-promoted.**
  - Mitigation: keep low weight and combine with existing penalties (tests/generated patterns where available).

- **Trade-off: no edge-type weighting in Phase 1.**
  - Accepted for simplicity and universality; edge-type weighting can be additive later if metrics justify.
