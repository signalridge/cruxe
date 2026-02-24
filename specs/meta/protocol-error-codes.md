# Protocol Error Codes (Canonical)

> Canonical error envelope and code registry for CodeCompass CLI/MCP/HTTP transports.
> This prevents error drift across specs and implementations.

## Scope

- Applies to all MCP tool responses and HTTP transport wrappers.
- CLI errors MAY map to this registry when surfaced in machine-readable JSON mode.
- Feature specs can add codes, but additions MUST be registered here.

## Error Envelope

All protocol errors should use:

```json
{
  "error": {
    "code": "index_incompatible",
    "message": "Index schema is incompatible. Run codecompass index --force.",
    "data": {
      "current_schema_version": 1,
      "required_schema_version": 2,
      "remediation": "codecompass index --force"
    }
  }
}
```

Rules:

1. `code` is stable and machine-consumable.
2. `message` is human-readable and concise.
3. `data` is optional, structured, and remediation-oriented.

## Canonical Metadata Enums

All MCP/HTTP contracts must use these exact response metadata enums:

- `indexing_status`: `not_indexed | indexing | ready | failed`
- `result_completeness`: `complete | partial | truncated`

Any legacy values (for example `idle`, `partial_available`) are deprecated and
must not appear in new contracts.

Implementation migration note:

- some pre-migration runtimes may still emit legacy metadata values.
- clients should map legacy values as:
  - `idle` -> `not_indexed`
  - `partial_available` -> `ready`
- migration target remains canonical values only in runtime responses.

## Core Registry

| Code | Category | Meaning | Typical Remediation |
|---|---|---|---|
| `invalid_input` | Validation | Generic input validation failure | Fix parameters and retry |
| `invalid_strategy` | Validation | `strategy` not in allowed enum | Use supported strategy value |
| `invalid_max_tokens` | Validation | `max_tokens < 1` | Provide positive token budget |
| `project_not_found` | Workspace | No registered project for requested workspace | Run `codecompass init` / correct workspace |
| `workspace_not_registered` | Workspace | Unknown workspace and auto-discovery disabled | Pre-register workspace or enable auto-workspace |
| `workspace_not_allowed` | Workspace | Workspace outside allowed roots | Use allowed root or adjust allowlist |
| `workspace_limit_exceeded` | Workspace | Auto-discovered workspace cap reached | Retry after eviction/cleanup |
| `index_in_progress` | Indexing | Index job already running for project | Wait for completion / poll `index_status` |
| `index_not_ready` | Indexing | Query requested against a `not_indexed` or `failed` index state | Run `index_repo` or inspect failure details |
| `sync_in_progress` | Indexing | Sync job active for same `(project, ref)` | Wait and retry |
| `index_stale` | Freshness | Strict freshness policy blocks stale index query | Run `sync_repo` |
| `index_incompatible` | Compatibility | Schema mismatch or corrupt manifest | Run `codecompass index --force` |
| `ref_not_indexed` | VCS | Requested ref lacks indexed state | Index requested ref first |
| `overlay_not_ready` | VCS | Ref overlay exists but not queryable yet | Retry after sync/index completion |
| `merge_base_failed` | VCS | Could not compute merge-base | Validate refs and repository integrity |
| `symbol_not_found` | Query | No matching symbol found | Broaden query or disambiguate path |
| `ambiguous_symbol` | Query | Multiple symbol matches require disambiguation | Provide `path` or qualified name |
| `file_not_found` | Query | File absent in indexed ref | Verify path/ref and index freshness |
| `result_not_found` | Query | Requested result target absent | Re-run query and refresh target selection |
| `no_edges_available` | Graph | Graph edges not populated yet | Ensure graph extraction/indexing completed |
| `internal_error` | Runtime | Unexpected internal execution failure | Retry, then inspect server logs |

## Warning vs Error

- Non-fatal conditions (for example value clamping) SHOULD be surfaced in
  `metadata.warnings[]`, not as protocol `error.code`.
- Size/dedup degradations SHOULD use metadata, not hard errors:
  - `result_completeness: "truncated"`
  - `safety_limit_applied: true`
  - `suppressed_duplicate_count: <n>`
- Only hard failures use the error envelope and MUST use a registry code.

## Evolution Rules

1. Additive only within a minor version.
2. Never repurpose an existing `code` with different semantics.
3. If deprecating a code, keep alias compatibility for at least one minor version.
4. All new codes MUST be documented here and referenced by the owning spec contract.
