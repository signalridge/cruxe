## ADDED Requirements

### Requirement: Symbol relation records MUST align with existing confidence schema
`symbol_edges` persistence for confidence-aware ranking MUST extend the existing schema without ambiguity.

Schema contract in this change:
- existing `confidence` bucket column remains the canonical categorical value (`high | medium | low` or equivalent mapped buckets),
- additive provenance fields include `edge_provider` and `resolution_outcome`,
- additive numeric field `confidence_weight` stores the deterministic numeric mapping for ranking aggregation.

This change MUST NOT introduce competing confidence columns with overlapping semantics.

#### Scenario: Existing confidence bucket is preserved and mapped
- **WHEN** an edge is persisted with confidence bucket `medium`
- **THEN** the record MUST preserve `confidence='medium'`
- **AND** MUST include mapped numeric `confidence_weight` according to deterministic mapping

#### Scenario: External reference edge persists provenance
- **WHEN** an import resolves as external reference
- **THEN** persisted edge record MUST include provider/outcome and corresponding confidence bucket/weight

#### Scenario: Missing provider data uses deterministic fallback mapping
- **WHEN** provider-specific confidence input is unavailable
- **THEN** runtime MUST apply deterministic default confidence bucket and numeric weight
- **AND** MUST keep schema-valid edge persistence (no null-contract break)
