## ADDED Requirements

### Requirement: Semantic metadata MUST expose async enrichment freshness state
When semantic enrichment is asynchronous, search metadata MUST expose freshness/backlog state fields.

Required additive fields:
- `semantic_enrichment_state` (`ready | backlog | degraded`)
- `semantic_backlog_size`
- `semantic_lag_hint`
- optional `degraded_reason`

#### Scenario: Backlog state is visible to clients
- **WHEN** enrichment queue depth exceeds configured threshold
- **THEN** metadata MUST set `semantic_enrichment_state=backlog`
- **AND** include backlog size and lag hint

### Requirement: Semantic freshness lag MUST preserve fail-soft retrieval behavior
Async semantic lag MUST NOT break query availability.

#### Scenario: Semantic lag falls back safely
- **WHEN** enrichment for latest generation is not complete
- **THEN** query path MUST continue serving lexical/symbol results
- **AND** metadata MUST communicate lag/degraded context deterministically
