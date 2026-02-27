# MCP Tool Contracts: CodeCompass Core MVP

Transport: JSON-RPC 2.0 over stdio (v1). HTTP transport deferred to Phase 1.5.

All responses include Protocol v1 metadata fields.
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Protocol v1 Response Metadata

Included in every tool response:

```json
{
  "metadata": {
    "codecompass_protocol_version": "1.0",
    "freshness_status": "fresh | stale | syncing",
    "indexing_status": "not_indexed | indexing | ready | failed",
    "result_completeness": "complete | partial | truncated",
    "ref": "main",
    "schema_status": "compatible | not_indexed | reindex_required | corrupt_manifest"
  }
}
```

### Stable Follow-up Handle Contract

For agent workflows, all retrieval results MUST include stable handles:

- symbol-like result: `symbol_id` (stable row identifier) and `symbol_stable_id` (location-insensitive identity),
- non-symbol result: `result_id` (stable per-index record identity),
- these handles are intended for deterministic follow-up calls (outline, context, references) without repeating broad search.

## Tool: `index_repo`

Trigger full or incremental indexing of a registered project.

### Input

```json
{
  "force": false,
  "ref": "main"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `force` | bool | no | Force full re-index (ignore manifest). Default: false. |
| `ref` | string | no | Branch/ref to index. Default: current HEAD or project default. |

> **Note:** `workspace` is determined at MCP server startup (`--workspace` flag)
> and is not a per-call parameter. Multi-workspace routing is implemented in spec 004 (Workspace & Transport).

### Output

```json
{
  "job_id": "abc123",
  "status": "running",
  "mode": "full",
  "file_count": null,
  "metadata": { ... }
}
```

`file_count` is unknown when a job is just queued/running, so it is returned as
`null` until completion.

### Errors

| Code | Meaning |
|------|---------|
| `project_not_found` | No project registered for the given workspace path. |
| `index_in_progress` | An indexing job is already running for this project. |
| `internal_error` | Failed to spawn index process. |

---

## Tool: `sync_repo`

Trigger incremental sync based on file changes since last indexed state.

### Input

```json
{
  "force": false,
  "ref": "main"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `force` | bool | no | Force full sync. Default: false (incremental). |
| `ref` | string | no | Branch/ref to sync. Default: current HEAD or project default. |

### Output

```json
{
  "job_id": "def456",
  "status": "running",
  "mode": "incremental",
  "changed_files": null,
  "metadata": { ... }
}
```

`changed_files` is filled by `index_status` after the job transitions to
`published`; for initial `running` responses it is `null`.

---

## Tool: `locate_symbol`

Find symbol definitions by name. Returns precise `file:line` locations.
Definitions are ranked before references (definition-first policy).

### Input

```json
{
  "name": "validate_token",
  "kind": "fn",
  "language": "rust",
  "ref": "main",
  "limit": 10
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Symbol name to locate. |
| `kind` | string | no | Filter by kind (fn, struct, class, method, etc.). |
| `language` | string | no | Filter by language. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD; if unavailable, project `default_ref`; fallback `"live"`. |
| `limit` | int | no | Max results. Default: 10. |

### Output

```json
{
  "results": [
    {
      "symbol_id": "sym_01HQ6Q0F8N8N4YQKJ0Y3W6M5VN",
      "symbol_stable_id": "b3:7d2a6f0f8f...",
      "path": "src/auth/jwt.rs",
      "line_start": 87,
      "line_end": 112,
      "kind": "fn",
      "name": "validate_token",
      "qualified_name": "auth::jwt::validate_token",
      "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
      "language": "rust",
      "score": 0.95
    }
  ],
  "total_candidates": 3,
  "metadata": { ... }
}
```

### Errors

| Code | Meaning |
|------|---------|
| `invalid_input` | Missing required input (`name`). |
| `project_not_found` | Workspace not initialized. Run `codecompass init`. |
| `index_incompatible` | Index schema is incompatible (`reindex_required` / `corrupt_manifest`). Run `codecompass index --force`. |
| `internal_error` | Unexpected runtime failure while executing query. |

---

## Tool: `search_code`

Search across symbols, snippets, and files with query intent classification.

### Input

```json
{
  "query": "where is rate limiting implemented",
  "ref": "main",
  "language": "rust",
  "limit": 10
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Search query (symbol name, path, error string, or natural language). |
| `ref` | string | no | Branch/ref scope. Default: current HEAD; if unavailable, project `default_ref`; fallback `"live"`. |
| `language` | string | no | Filter by language. |
| `limit` | int | no | Max results. Default: 10. |

### Output

```json
{
  "results": [
    {
      "result_id": "snip_01HQ6Q3Q3SHYP2B7Z1H5Y8M7KR",
      "symbol_id": "sym_01HQ6Q3G2QJ8S5CAV10H7B2J0R",
      "symbol_stable_id": "b3:9a9f0ef7c2...",
      "result_type": "snippet",
      "path": "src/middleware/rate_limit.rs",
      "line_start": 15,
      "line_end": 48,
      "kind": "fn",
      "name": "check_rate_limit",
      "qualified_name": "middleware::rate_limit::check_rate_limit",
      "language": "rust",
      "score": 0.82,
      "snippet": "pub fn check_rate_limit(req: &Request) -> Result<()> { ... }"
    }
  ],
  "query_intent": "natural_language",
  "total_candidates": 47,
  "suggested_next_actions": [
    { "tool": "locate_symbol", "name": "check_rate_limit", "ref": "main" },
    { "tool": "search_code", "query": "rate limit middleware", "ref": "main", "limit": 5 }
  ],
  "debug": {
    "join_status": {
      "hits": 7,
      "misses": 2
    }
  },
  "metadata": { ... }
}
```

> Note: `debug` is optional and appears only when the server runs in debug mode.

### Errors

| Code | Meaning |
|------|---------|
| `invalid_input` | Missing required input (`query`). |
| `project_not_found` | Workspace not initialized. Run `codecompass init`. |
| `index_incompatible` | Index schema is incompatible (`reindex_required` / `corrupt_manifest`). Run `codecompass index --force`. |
| `internal_error` | Unexpected runtime failure while executing query. |

### Query Intent Classification

| Intent | Trigger Pattern | Index Priority |
|--------|----------------|---------------|
| `symbol` | Looks like an identifier (CamelCase, snake_case) | symbols first |
| `path` | Contains `/` or `.` with file extension | files first |
| `error` | Contains quotes, stack trace patterns, error codes | snippets first |
| `natural_language` | Default fallback | all three indices |

---

## Tool: `index_status`

Get current indexing status and job history for a project.

### Input

```json
{
  "ref": "main"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `ref` | string | no | Branch/ref to query status for. Default: current HEAD or project default. |

### Output

```json
{
  "project_id": "a1b2c3d4e5f6g7h8",
  "repo_root": "/path/to/repo",
  "index_status": "ready",
  "schema_status": "compatible",
  "current_schema_version": 1,
  "required_schema_version": 1,
  "last_indexed_at": "2026-02-23T10:30:00Z",
  "ref": "main",
  "file_count": 3891,
  "symbol_count": 12847,
  "active_job": null,
  "recent_jobs": [
    {
      "job_id": "abc123",
      "ref": "main",
      "mode": "full",
      "status": "published",
      "changed_files": 3891,
      "duration_ms": 45000,
      "created_at": "2026-02-23T10:29:15Z"
    }
  ],
  "metadata": { ... }
}
```

---

## CLI Commands (non-MCP)

These are direct CLI commands, not MCP tools:

| Command | Description |
|---------|-------------|
| `codecompass init` | Register project, create indices, detect VCS mode |
| `codecompass doctor` | Health check: Tantivy, SQLite, tree-sitter, ignore rules |
| `codecompass index [--force] [--path PATH] [--ref REF]` | Index or re-index a project |
| `codecompass sync [--force] [--workspace PATH]` | Incremental sync (CLI wrapper over `sync_repo`) |
| `codecompass search QUERY [--ref REF] [--lang LANG]` | CLI search interface |
| `codecompass serve-mcp [--workspace PATH]` | Start MCP server (stdio) |

All CLI commands support `--verbose` / `-v` for increased log output and
`--config PATH` for custom config file path.
