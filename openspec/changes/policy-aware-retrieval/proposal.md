## Why

For private and enterprise codebases, retrieval quality alone is insufficient. Teams need enforceable policy controls to prevent exposing sensitive paths/secrets while keeping search and context assembly usable.

## What Changes

1. Add retrieval policy profiles (`strict`, `balanced`, `off`) with deterministic semantics.
2. Add path/type allow-deny controls applied before result emission.
3. Add lightweight secret/PII redaction for snippet payloads and context packs.
4. Add policy decision metadata and audit counters for blocked/redacted results.

## Capabilities

### New Capabilities
- `policy-aware-retrieval`: policy-governed retrieval filtering and redaction across search and context pack outputs.

### Modified Capabilities
- `002-agent-protocol`: search/context responses include policy mode and redaction/filtering metadata fields.

## Impact

- Affected crates: `cruxe-query`, `cruxe-core`, `cruxe-mcp`.
- API impact: additive request options and metadata fields.
- Security impact: reduces risk of accidental sensitive context exposure.
- Operational impact: requires policy config defaults and rollout guidance.
