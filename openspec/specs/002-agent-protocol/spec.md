## Purpose
Define agent-facing query response controls for explainability, compact payload
shaping, dedup observability, and graceful hard-limit truncation behavior.
## Requirements
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

### Requirement: Protocol error registry conformance is transport-consistent
All MCP tool failures MUST emit the canonical error envelope and canonical error
codes defined by `specs/meta/protocol-error-codes.md`.

For equivalent failure conditions, stdio and HTTP transports MUST emit the same:
- `error.code`
- error semantics (same failure class and remediation meaning)

Transport-specific wrappers MAY differ only in transport envelope details and
MUST NOT introduce new protocol-level error codes.

#### Scenario: Equivalent invalid input maps to same protocol error code
- **WHEN** the same invalid tool input is submitted via stdio and HTTP
- **THEN** both responses MUST emit the same canonical `error.code` from the registry

#### Scenario: Equivalent compatibility failure maps to same protocol error code
- **WHEN** a tool request hits index compatibility failure (`not_indexed`, `reindex_required`, or `corrupt_manifest`)
- **THEN** both transports MUST emit canonical compatibility error codes and remediation-oriented error data

### Requirement: Explainability and freshness config normalization is canonical
Runtime config loading MUST normalize explainability and freshness settings to
canonical runtime enums while preserving legacy compatibility inputs.

Normalization requirements:
- `search.ranking_explain_level` MUST resolve to `off | basic | full`
- legacy `debug.ranking_reasons=true` MUST deterministically map to `full`
- invalid config values MUST fall back to canonical defaults (not arbitrary runtime branches)

#### Scenario: Legacy debug flag resolves to canonical explainability mode
- **WHEN** config provides `debug.ranking_reasons=true` and no explicit `search.ranking_explain_level`
- **THEN** runtime explainability mode MUST resolve to canonical `full`

#### Scenario: Invalid explainability config falls back safely
- **WHEN** config contains non-canonical explainability value
- **THEN** runtime MUST normalize to canonical default behavior without crashing and without emitting non-canonical modes

