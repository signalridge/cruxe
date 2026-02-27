# MCP Tool Contracts: VCS Core Routing

This contract defines VCS-core behavior changes for existing MCP tools.
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Scope

Tools in scope:

- `search_code` (updated)
- `locate_symbol` (updated)

Out of scope (moved to `006-vcs-ga-tooling`):

- `diff_context`
- `find_references`
- `explain_ranking`
- `list_refs`
- `switch_ref`

## Protocol v1 Metadata Extension

When a query is executed in VCS mode with merged base+overlay resolution,
results MUST include:

- `source_layer`: `"base" | "overlay"`

And response metadata MUST include the standard Protocol v1 fields:

- `cruxe_protocol_version`
- `ref`
- `freshness_status`
- `indexing_status`
- `result_completeness`
- `schema_status`

## Tool: `search_code` (VCS mode)

### Input

Unchanged from prior specs (`query`, optional `ref`, optional `detail_level`, etc.).

### Output (VCS extension)

Each result includes:

- `path`, `line_start`, `line_end`, `name`, `kind`, ...
- stable follow-up handles: `symbol_id` and `symbol_stable_id` when result maps to a symbol
- `source_layer` (`base` or `overlay`)

### Merge Behavior

- Query base + overlay in parallel
- Dedup by merge key
- Overlay wins collisions
- Tombstoned base paths are filtered out

## Tool: `locate_symbol` (VCS mode)

### Input

Unchanged from prior specs.

### Output (VCS extension)

Result shape is unchanged except added:

- stable follow-up handles: `symbol_id` and `symbol_stable_id`
- `source_layer` (`base` or `overlay`)

### Correctness Behavior

- Must honor requested `ref`
- Must not leak deleted/tombstoned base symbols into feature-branch results

## Errors

New operational errors introduced by VCS-core execution path:

| Code | Meaning |
|------|---------|
| `sync_in_progress` | A sync job is already active for the same `(project, ref)`. |
| `ref_not_indexed` | The requested ref has no indexed overlay/base state yet. |
| `overlay_not_ready` | The ref overlay exists but is not yet queryable (bootstrap or recovery in progress). |

All errors MUST include actionable remediation hints.
