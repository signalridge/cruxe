## ADDED Requirements

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
