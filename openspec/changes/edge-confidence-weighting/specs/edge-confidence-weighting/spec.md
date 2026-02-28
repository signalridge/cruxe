## ADDED Requirements

### Requirement: Relation edges MUST carry confidence semantics
Each persisted relation edge MUST have deterministic confidence semantics derived from provider/outcome quality.

Confidence model MUST define:
- canonical confidence buckets,
- numeric confidence weights,
- default mapping for unresolved/external outcomes.

#### Scenario: Resolved internal edge gets higher confidence than unresolved edge
- **WHEN** one edge is internally resolved and another is unresolved heuristic
- **THEN** resolved edge confidence weight MUST be greater than unresolved edge confidence weight

### Requirement: Structural ranking MUST use confidence-weighted aggregation
Graph-derived ranking signals MUST use confidence-weighted aggregation instead of raw edge counts.

#### Scenario: Low-confidence edge cluster is suppressed
- **WHEN** candidate centrality is driven mostly by low-confidence edges
- **THEN** weighted aggregation MUST reduce its structural boost relative to high-confidence alternatives

### Requirement: Confidence provenance MUST be explainable
Ranking explain metadata MUST include confidence-derived contribution details for structural boosts.

#### Scenario: Explain output includes confidence contribution
- **WHEN** structural boost contributes to final rank
- **THEN** explain output MUST include the confidence-weighted component and source confidence summary
