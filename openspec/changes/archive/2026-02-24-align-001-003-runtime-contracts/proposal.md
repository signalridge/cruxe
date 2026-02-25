## Why

`specs/001` through `specs/003` were already aligned to the newer protocol
contract (canonical metadata enums, `compact`, `ranking_explain_level`,
`truncated`, etc.), but runtime behavior still kept legacy outputs in several
paths. That mismatch created a “docs are correct, runtime differs” state that
hurt stability and predictability for agents.

## What Changes

- Align runtime `indexing_status` and `result_completeness` outputs to canonical
  `001-003` enums (`idle -> ready`, `partial_available -> ready`; add
  `not_indexed`, `failed`, `truncated`) while preserving legacy-read
  compatibility.
- Migrate `search_code` / `locate_symbol` explainability control to
  `ranking_explain_level` (`off|basic|full`) with backward-compatible fallback
  from legacy `debug.ranking_reasons`.
- Implement remaining `002` items FR-105b/FR-105c:
  deduplication, payload safety limit, `truncated` semantics, and deterministic
  `suggested_next_actions`.
- Align `002` `compact` request/serialization behavior while keeping `003` tools
  on token-budget behavior (no new `compact` parameter for `003` scope).
- Add tests that lock contract fields, compatibility behavior, and payload limit
  semantics.

## Capabilities

### New Capabilities

None. This change focused on runtime alignment with existing specs.

### Modified Capabilities

- `001-core-mvp`: enforce canonical Protocol v1 metadata enums and compatibility
  mapping in actual runtime output.
- `002-agent-protocol`: convert documented requirements for `compact`,
  `ranking_explain_level`, deduplication, and payload safety into runtime
  guarantees.
- `003-structure-nav`: keep metadata completeness semantics aligned with
  `001/002` while preserving the “no `compact` parameter” boundary for structure
  tools.

## Impact

- Affected code:
  - `codecompass-core` (types/config)
  - `codecompass-query` (detail/ranking/search)
  - `codecompass-mcp` (protocol/tool handlers/schema)
  - `codecompass-cli` (`serve-mcp` config entrypoint)
- API impact:
  - compatibility upgrade for MCP tool input/output contracts
    (new normalized enum values + fields, legacy-read compatibility retained).
- Test impact:
  - additional contract-alignment and regression coverage tied to
    `001-003` implementation tracking.
