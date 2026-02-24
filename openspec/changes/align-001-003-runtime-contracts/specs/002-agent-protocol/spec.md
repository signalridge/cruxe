## MODIFIED Requirements

### Requirement: Explainability control via ranking_explain_level
`search_code` and `locate_symbol` MUST support explainability control using
`ranking_explain_level` with values `off | basic | full`.

Precedence MUST be:

1. request argument `ranking_explain_level`
2. config default `search.ranking_explain_level`
3. legacy compatibility fallback from `debug.ranking_reasons`

Legacy fallback mapping MUST be deterministic:

- `debug.ranking_reasons = true` -> `full`
- `debug.ranking_reasons = false` -> `off`

Behavior:

- `off`: `ranking_reasons` MUST be omitted
- `basic`: `ranking_reasons` MUST include compact normalized factors only
- `full`: `ranking_reasons` MUST include full deterministic scoring breakdown

#### Scenario: Off mode omits ranking reasons
- **WHEN** `ranking_explain_level` resolves to `off`
- **THEN** response metadata MUST NOT include `ranking_reasons`

#### Scenario: Basic mode emits compact factors
- **WHEN** `ranking_explain_level` resolves to `basic`
- **THEN** response metadata MUST include `ranking_reasons` with compact normalized fields:
  `result_index`, `exact_match`, `path_boost`, `definition_boost`, `semantic_similarity`, `final_score`

#### Scenario: Full mode emits complete factors
- **WHEN** `ranking_explain_level` resolves to `full`
- **THEN** response metadata MUST include per-result full scoring fields:
  `result_index`, `exact_match_boost`, `qualified_name_boost`, `path_affinity`,
  `definition_boost`, `kind_match`, `bm25_score`, `final_score`

### Requirement: Compact response shaping is serialization-only
`compact` MUST be applied at serialization stage after retrieval and ranking.
`compact` MUST preserve identity/location/score/follow-up handles and MUST drop
large optional context payload fields.

#### Scenario: Compact preserves ranking order and identifiers
- **WHEN** a query is executed with `compact: true`
- **THEN** result ordering and stable identifiers MUST match non-compact output for the same query

#### Scenario: Compact suppresses heavy optional fields
- **WHEN** a query is executed with `compact: true`
- **THEN** large optional fields such as body previews MUST be omitted from serialized results

### Requirement: Near-duplicate suppression with explicit metadata
Query response assembly for `search_code` and `locate_symbol` MUST deduplicate
near-identical hits by symbol/file-region identity before final output and MUST
expose suppression count via `suppressed_duplicate_count`.

#### Scenario: Duplicate-heavy result set is deduplicated
- **WHEN** retrieval returns repeated hits for the same symbol/file region
- **THEN** final emitted results MUST keep only one representative per identity key

#### Scenario: Suppression count is observable
- **WHEN** one or more duplicates are removed
- **THEN** metadata MUST include `suppressed_duplicate_count` greater than zero

### Requirement: Hard payload safety limits use graceful truncation
Query tools MUST enforce a hard payload safety limit and MUST degrade
gracefully instead of failing.

When the safety limit is hit:

- runtime MUST emit deterministic prefix results
- `result_completeness` MUST be `truncated`
- `safety_limit_applied` MUST be `true` (field in `metadata`, omitted when `false`)
- `suggested_next_actions` MUST provide deterministic follow-up guidance

#### Scenario: Safety limit triggers truncation contract
- **WHEN** serialized response size exceeds the configured hard limit
- **THEN** response metadata MUST set `result_completeness: "truncated"` and `safety_limit_applied: true`

#### Scenario: Safety limit does not hard-fail requests
- **WHEN** hard payload limit is reached
- **THEN** the tool MUST return a valid response with deterministic `suggested_next_actions`
