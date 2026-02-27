# MCP Tool Contracts: Agent Protocol Enhancement

Transport: JSON-RPC 2.0 over stdio (v1). Same as 001-core-mvp.

All responses include Protocol v1 metadata fields (see 001-core-mvp contracts).
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Changes to Existing Tools

### `search_code` — Added Parameters

New optional parameters added to the existing `search_code` tool:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `detail_level` | string | no | Response verbosity: `"location"`, `"signature"` (default), `"context"`. |
| `compact` | bool | no | Token-thrifty serialization flag. Keeps identity/location/score fields and omits large context blocks by default. |
| `freshness_policy` | string | no | Override freshness behavior: `"strict"`, `"balanced"` (default), `"best_effort"`. |
| `ranking_explain_level` | string | no | Explainability payload level: `"off"` (default), `"basic"`, `"full"`. |

### `locate_symbol` — Added Parameters

New optional parameters added to the existing `locate_symbol` tool:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `detail_level` | string | no | Response verbosity: `"location"`, `"signature"` (default), `"context"`. |
| `compact` | bool | no | Token-thrifty serialization flag. Works with any `detail_level`. |
| `freshness_policy` | string | no | Override freshness behavior: `"strict"`, `"balanced"` (default), `"best_effort"`. |
| `ranking_explain_level` | string | no | Explainability payload level: `"off"` (default), `"basic"`, `"full"`. |

### Detail Level Response Shapes

#### `detail_level: "location"` (~50 tokens per result)

```json
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token"
}
```

`location` is the minimal shape, but implementations MAY include deterministic
follow-up handles (for example `result_id`, `result_type`, `symbol_id`,
`symbol_stable_id`, `score`) so agents can chain calls without requiring
`signature`/`context`.

#### `detail_level: "signature"` (default, ~100 tokens per result)

```json
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token",
  "qualified_name": "auth::jwt::validate_token",
  "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
  "language": "rust",
  "visibility": "public"
}
```

#### `detail_level: "context"` (~300-500 tokens per result)

```json
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token",
  "qualified_name": "auth::jwt::validate_token",
  "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
  "language": "rust",
  "visibility": "public",
  "body_preview": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims> {\n    let decoded = decode(token, key, &Validation::default())?;\n    // ... truncated ...\n}",
  "parent": {
    "kind": "impl",
    "name": "JwtValidator",
    "path": "src/auth/jwt.rs",
    "line": 45
  },
  "related_symbols": [
    { "kind": "struct", "name": "Claims", "path": "src/auth/jwt.rs", "line": 12 },
    { "kind": "enum", "name": "TokenError", "path": "src/auth/error.rs", "line": 5 }
  ]
}
```

Fields that are unavailable (e.g., no parent, no related symbols) are omitted
from the response, not set to null.

`compact: true` applies after `detail_level` shaping and removes large optional
payloads (for example body previews) while preserving deterministic follow-up
handles.

### Ranking Reasons (`ranking_explain_level`)

When `ranking_explain_level` is set to `basic` or `full`, the response metadata
includes a `ranking_reasons` field:

```json
{
  "results": [ ... ],
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main",
    "ranking_reasons": [
      {
        "result_index": 0,
        "exact_match_boost": 1.0,
        "qualified_name_boost": 0.8,
        "path_affinity": 0.0,
        "definition_boost": 1.0,
        "kind_match": 0.5,
        "bm25_score": 12.34,
        "final_score": 0.95
      },
      {
        "result_index": 1,
        "exact_match_boost": 0.0,
        "qualified_name_boost": 0.6,
        "path_affinity": 0.3,
        "definition_boost": 0.0,
        "kind_match": 0.0,
        "bm25_score": 8.21,
        "final_score": 0.72
      }
    ]
  }
}
```

`ranking_explain_level` behavior:

- `off` (default): `ranking_reasons` is absent.
- `basic`: return compact normalized factors for agent routing.
- `full`: return complete debug scoring breakdown (example payload above).
- legacy fallback: `debug.ranking_reasons = true` maps to `full`, `false` maps to `off`.

For `full`, each entry includes:
`result_index`, `exact_match_boost`, `qualified_name_boost`, `path_affinity`,
`definition_boost`, `kind_match`, `bm25_score`, `final_score`.

### Freshness Policy Behavior

| Policy | Stale Index Behavior |
|--------|---------------------|
| `strict` | Block query. Return error `index_stale` with message guiding to `sync_repo`. |
| `balanced` (default) | Return results with `freshness_status: "stale"`. Trigger async background sync. |
| `best_effort` | Return results with `freshness_status: "stale"`. No sync triggered. |

#### Strict Mode Error Response

```json
{
  "error": {
    "code": "index_stale",
    "message": "Index is stale. Last indexed commit does not match HEAD. Run sync_repo to refresh.",
    "data": {
      "last_indexed_commit": "abc123",
      "current_head": "def456",
      "suggestion": "Call sync_repo to update the index before querying."
    }
  }
}
```

---

## Tool: `get_file_outline`

Return a nested symbol tree for a source file. This is a pure SQLite query
against the `symbol_relations` table — no Tantivy involvement.

### Input

```json
{
  "path": "src/auth/handler.rs",
  "ref": "main",
  "depth": "all",
  "language": "rust"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | string | yes | Source file path relative to repo root. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `depth` | string | no | `"top"` (top-level only) or `"all"` (nested). Default: `"all"`. |
| `language` | string | no | Filter hint (informational, language detected from file). |

### Output

```json
{
  "file_path": "src/auth/handler.rs",
  "language": "rust",
  "symbols": [
    {
      "kind": "use",
      "name": "crate::auth::Claims",
      "line_start": 1,
      "line_end": 1
    },
    {
      "kind": "struct",
      "name": "AuthHandler",
      "line_start": 12,
      "line_end": 18,
      "visibility": "pub",
      "signature": "pub struct AuthHandler"
    },
    {
      "kind": "impl",
      "name": "AuthHandler",
      "line_start": 20,
      "line_end": 130,
      "children": [
        {
          "kind": "fn",
          "name": "new",
          "line_start": 21,
          "line_end": 33,
          "visibility": "pub",
          "signature": "pub fn new(config: AuthConfig) -> Self"
        },
        {
          "kind": "fn",
          "name": "authenticate",
          "line_start": 35,
          "line_end": 66,
          "visibility": "pub",
          "signature": "pub fn authenticate(&self, req: &Request) -> Result<User>"
        }
      ]
    }
  ],
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh",
    "ref": "main",
    "symbol_count": 8
  }
}
```

### Errors

| Code | Meaning |
|------|---------|
| `file_not_found` | No file at the given path in the index for the given ref. |
| `project_not_found` | No project registered for the workspace. |

### Performance

- Pure SQLite query: `SELECT * FROM symbol_relations WHERE repo=? AND ref=? AND path=? ORDER BY line_start`
- Nested tree built in memory from `parent_symbol_id` chains.
- Target: p95 < 50ms for files with up to 200 symbols.

---

## Tool: `health_check`

Return project-level operational status for the server's default project.

### Input

```json
{
  "workspace": "/path/to/repo"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `workspace` | string | no | Absolute path to target workspace. Default: server's default project. |

### Output

```json
{
  "status": "ready",
  "version": "0.2.0",
  "uptime_seconds": 3600,
  "tantivy_ok": true,
  "sqlite_ok": true,
  "grammars": {
    "available": ["rust", "typescript", "python", "go"],
    "missing": []
  },
  "active_job": null,
  "interrupted_recovery_report": null,
  "workspace_warmset": {
    "enabled": true,
    "capacity": 3,
    "members": ["/path/to/repo"]
  },
  "prewarm_status": "complete",
  "startup_checks": {
    "index": {
      "status": "compatible",
      "current_schema_version": 1,
      "required_schema_version": 1,
      "message": null
    }
  },
  "projects": [
    {
      "project_id": "a1b2c3d4e5f6g7h8",
      "repo_root": "/path/to/repo",
      "index_status": "ready",
      "schema_status": "compatible",
      "current_schema_version": 1,
      "required_schema_version": 1,
      "freshness_status": "fresh",
      "last_indexed_at": "2026-02-23T10:30:00Z",
      "ref": "main",
      "file_count": 3891,
      "symbol_count": 12847
    }
  ],
  "metadata": {
    "codecompass_protocol_version": "1.0"
  }
}
```

### Status Values

| Status | Meaning |
|--------|---------|
| `ready` | All projects indexed and indices warm. Full query speed. |
| `warming` | Tantivy index prewarming in progress. Queries accepted but may be slower. |
| `indexing` | Bootstrap or large incremental sync running. Partial results available. |
| `error` | A project has failed indexing. Check `projects[].index_status` for details. |

### Startup Compatibility Behavior

`health_check` and `index_status` expose startup compatibility results.

If `startup_checks.index.status` is:

- `compatible`: query tools proceed normally.
- `not_indexed`: query tools may return empty/partial results until indexing completes.
- `reindex_required` or `corrupt_manifest`: query tools return `index_incompatible`
  with remediation guidance (`codecompass index --force`).

### Active Job Shape (when present)

```json
{
  "active_job": {
    "job_id": "abc123",
    "project_id": "a1b2c3d4e5f6g7h8",
    "mode": "incremental",
    "status": "running",
    "changed_files": 12,
    "started_at": "2026-02-23T10:35:00Z"
  }
}
```

### Recovery/Warmset Extensions

`004-workspace-transport` extends health surfaces with:

- `interrupted_recovery_report`: present when startup reconciliation finds
  interrupted jobs (otherwise null/omitted).
- `workspace_warmset`: bounded recent-workspace set chosen for startup prewarm.

### Errors

| Code | Meaning |
|------|---------|
| `workspace_not_registered` | The specified workspace path is not registered. |

---

## CLI Changes

| Command | Change |
|---------|--------|
| `codecompass serve-mcp` | Add `--no-prewarm` flag to disable Tantivy index prewarming on startup |

Config file additions (`config.toml`):

```toml
[query]
# Default freshness policy: "strict", "balanced", "best_effort"
freshness_policy = "balanced"

# Ranking explainability payload level: "off", "basic", "full"
ranking_explain_level = "off"
```

Compatibility note:

- legacy `debug.ranking_reasons` may still be accepted by pre-migration builds.
- when both are present, `ranking_explain_level` is authoritative.
- if only legacy `debug.ranking_reasons=true` is set, it maps to
  `ranking_explain_level="full"` for backward compatibility.
