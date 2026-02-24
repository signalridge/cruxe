# MCP Tool Contracts: Multi-Workspace & Transport

Transport: JSON-RPC 2.0 over stdio (default) or HTTP POST (v0.3.0+).

All responses include Protocol v1 metadata fields (unchanged from 001-core-mvp).
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Workspace Parameter (All Tools)

Starting in v0.3.0, all MCP tools accept an optional `workspace` parameter.

```json
{
  "workspace": "/path/to/repo"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `workspace` | string | no | Absolute path to target workspace. Default: server's default project. |

### Workspace Resolution Logic

1. `workspace` omitted -> use default project (from `codecompass init` or `--workspace` startup flag).
2. `workspace` is a known registered project -> route to that project's indices.
3. `workspace` is unknown + `--auto-workspace` disabled -> return `workspace_not_registered` error.
4. `workspace` is unknown + `--auto-workspace` enabled:
   a. Resolve path via `realpath` (canonicalize).
   b. Verify resolved path starts with at least one `--allowed-root` prefix.
   c. If outside all allowed roots -> return `workspace_not_allowed` error.
   d. Register in `known_workspaces` table with `auto_discovered = 1`.
   e. Trigger on-demand bootstrap indexing.
   f. Return tool response with `indexing_status: "indexing"`, `result_completeness: "partial"`.

When server startup pins a workspace context, middleware auto-injects that
workspace for requests that omit `workspace`, while still allowing explicit
per-request override.

### Canonical Metadata Enums

All response examples in this contract use:

- `indexing_status`: `not_indexed | indexing | ready | failed`
- `result_completeness`: `complete | partial | truncated`

### Workspace Errors

| Code | Meaning |
|------|---------|
| `workspace_not_registered` | Workspace path is not registered and `--auto-workspace` is disabled. |
| `workspace_not_allowed` | Workspace path resolves outside all `--allowed-root` prefixes. |
| `workspace_limit_exceeded` | Max auto-discovered workspaces reached (default: 10). LRU eviction attempted first. |

### Affected Tools

The `workspace` parameter is added to all existing MCP tools:

- `index_repo`
- `sync_repo`
- `search_code`
- `locate_symbol`
- `index_status`
- `get_file_outline`
- `health_check`
- `get_symbol_hierarchy`
- `find_related_symbols`
- `get_code_context`

---

## Index Progress Notifications

### MCP Notification Protocol

Progress notifications use the standard MCP `notifications/progress` method.
These are server-to-client messages with no expected response.

Notifications are only sent when the client declares notification support in the
`initialize` handshake.

### Progress Report Notification

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "index-job-abc123",
    "value": {
      "kind": "report",
      "title": "Indexing project: backend",
      "message": "Parsing files: 1247/3891 (32%)",
      "percentage": 32
    }
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `progressToken` | string | Matches the job ID from `index_repo` response. Format: `index-job-{job_id}`. |
| `value.kind` | string | `"report"` for in-progress, `"end"` for completion. |
| `value.title` | string | Human-readable operation title. |
| `value.message` | string | Human-readable progress detail including counts. |
| `value.percentage` | int | Estimated completion percentage (0-100). |

### Progress Stages

Progress notifications are emitted at these stages:

| Stage | Message Pattern | Percentage Range |
|-------|----------------|-----------------|
| Discovery | `"Scanning files: {scanned} discovered"` | 0-10% |
| Parsing | `"Parsing files: {parsed}/{total} ({pct}%)"` | 10-70% |
| Indexing | `"Indexing: {indexed}/{total} files, {symbols} symbols"` | 70-95% |
| Finalizing | `"Finalizing index..."` | 95-99% |

### Completion Notification

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "index-job-abc123",
    "value": {
      "kind": "end",
      "title": "Indexing complete: backend",
      "message": "Indexed 3891 files, 12847 symbols in 14.2s"
    }
  }
}
```

### Failure Notification

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "index-job-abc123",
    "value": {
      "kind": "end",
      "title": "Indexing failed: backend",
      "message": "Error: Permission denied reading /path/to/file"
    }
  }
}
```

### Polling Fallback via `index_status`

When the client does not support notifications, the same progress data is available
by polling `index_status`:

```json
{
  "tool": "index_status",
  "input": {
    "workspace": "/path/to/repo"
  }
}
```

Response (during active indexing):

```json
{
  "project_id": "a1b2c3d4e5f6g7h8",
  "repo_root": "/path/to/repo",
  "index_status": "indexing",
  "active_job": {
    "job_id": "abc123",
    "progress_token": "index-job-abc123",
    "mode": "full",
    "status": "running",
    "files_scanned": 3891,
    "files_indexed": 1247,
    "symbols_extracted": 4523,
    "estimated_completion_pct": 32,
    "started_at": "2026-02-23T10:29:15Z"
  },
  "interrupted_recovery_report": {
    "detected": true,
    "interrupted_jobs": 1,
    "last_interrupted_at": "2026-02-23T10:11:00Z",
    "recommended_action": "run sync_repo or index_repo for the affected workspace"
  },
  "metadata": { ... }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `active_job.progress_token` | string | Same token used in notifications. |
| `active_job.files_scanned` | int | Total files discovered during scan phase. |
| `active_job.files_indexed` | int | Files fully indexed so far. |
| `active_job.symbols_extracted` | int | Symbols extracted so far. |
| `active_job.estimated_completion_pct` | int | Estimated completion (0-100). |
| `interrupted_recovery_report` | object | Present when startup reconciliation found interrupted jobs; omitted otherwise. |

### `health_check` Extension (v0.3.0)

`health_check` keeps its schema from `002-agent-protocol` and adds:

- `interrupted_recovery_report` (same shape as `index_status`)
- optional `workspace_warmset` status block for startup prewarm visibility

---

## Tool: `index_repo` (Updated)

### Input (v0.3.0)

```json
{
  "workspace": "/path/to/repo",
  "force": false
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `workspace` | string | no | Target workspace path. Default: server's default project. |
| `force` | bool | no | Force full re-index. Default: false. |

### Output (v0.3.0)

```json
{
  "job_id": "abc123",
  "progress_token": "index-job-abc123",
  "status": "running",
  "mode": "full",
  "file_count": 3891,
  "metadata": { ... }
}
```

New field: `progress_token` -- used to correlate `notifications/progress` messages
with this job. Clients can also use this token to query `index_status`.

---

## HTTP Transport

### Server Startup

```bash
codecompass serve-mcp --transport http --port 9100
codecompass serve-mcp --transport http --port 9100 --bind 0.0.0.0
codecompass serve-mcp --transport http --port 9100 --auto-workspace --allowed-root /home/dev
```

### `GET /health`

Readiness probe for load balancers, monitoring, and agent pre-flight checks.

**Response (200 OK)**:

```json
{
  "status": "ready",
  "projects": [
    {
      "project_id": "a1b2c3d4e5f6g7h8",
      "repo_root": "/Users/dev/backend",
      "index_status": "ready",
      "schema_status": "compatible",
      "current_schema_version": 1,
      "required_schema_version": 1,
      "last_indexed_at": "2026-02-23T10:30:00Z",
      "ref": "main",
      "file_count": 3891,
      "symbol_count": 12847
    }
  ],
  "version": "0.3.0",
  "uptime_seconds": 3600,
  "workspace_warmset": {
    "enabled": true,
    "capacity": 3,
    "members": ["/Users/dev/backend", "/Users/dev/frontend"]
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Server-level status: `ready`, `warming`, `indexing`, `error`. |
| `projects` | array | Per-project status entries. |
| `projects[].project_id` | string | Internal project identifier. |
| `projects[].repo_root` | string | Absolute path to project root. |
| `projects[].index_status` | string | `ready`, `indexing`, `stale`, `error`. |
| `projects[].schema_status` | string | `compatible`, `not_indexed`, `reindex_required`, `corrupt_manifest`. |
| `projects[].current_schema_version` | int | Schema version found in local index metadata. |
| `projects[].required_schema_version` | int | Schema version required by running binary. |
| `projects[].last_indexed_at` | string | ISO8601 timestamp of last completed index. |
| `projects[].ref` | string | Current default ref for this project. |
| `projects[].file_count` | int | Number of indexed files. |
| `projects[].symbol_count` | int | Number of indexed symbols. |
| `version` | string | CodeCompass version. |
| `uptime_seconds` | int | Seconds since server started. |
| `workspace_warmset` | object | Warmset status for startup prewarm optimization (capacity is configurable; examples use `3`). |

### Status Logic

| Condition | `status` value |
|-----------|---------------|
| Tantivy prewarm in progress | `warming` |
| Any project actively indexing | `indexing` |
| Any project in error state | `error` |
| All projects ready | `ready` |

Priority order (highest to lowest): `error` > `warming` > `indexing` > `ready`.

### `POST /` (MCP JSON-RPC)

All MCP tool calls are sent as JSON-RPC 2.0 POST requests to the root path.

**Request**:

```http
POST / HTTP/1.1
Host: 127.0.0.1:9100
Content-Type: application/json

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "locate_symbol",
    "arguments": {
      "name": "validate_token",
      "workspace": "/Users/dev/backend"
    }
  }
}
```

**Response**:

```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{ ... same JSON as stdio response ... }"
      }
    ]
  }
}
```

The response body is identical to stdio transport. HTTP is a transport wrapper,
not a separate API.

### Supported HTTP Methods

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health/readiness check |
| `POST` | `/` | MCP JSON-RPC tool dispatch |

All other paths return 404. All other methods return 405.

### Error Responses

| HTTP Status | Condition |
|-------------|-----------|
| 200 | Successful JSON-RPC response (including JSON-RPC error responses) |
| 400 | Malformed JSON or invalid JSON-RPC request |
| 405 | Method not allowed (e.g., GET on /) |
| 404 | Unknown path |
| 500 | Internal server error |

HTTP transport MUST preserve canonical JSON-RPC error envelopes and machine-stable
`error.code` values from `specs/meta/protocol-error-codes.md` (it is only a wrapper,
not a second error taxonomy).

Minimum mapping requirements:

| Condition | `error.code` |
|-----------|--------------|
| Malformed JSON / invalid JSON-RPC payload | `invalid_input` |
| Unknown workspace with auto-discovery disabled | `workspace_not_registered` |
| Workspace outside allowlist | `workspace_not_allowed` |
| Schema mismatch / corrupt manifest during tool call | `index_incompatible` |

---

## CLI Flags (Updated `serve-mcp`)

```
codecompass serve-mcp [OPTIONS]

Options:
  --transport <TRANSPORT>      Transport mode [default: stdio] [values: stdio, http]
  --port <PORT>                HTTP port [default: 9100] (only with --transport http)
  --bind <ADDR>                Bind address [default: 127.0.0.1] (only with --transport http)
  --workspace <PATH>           Pre-register workspace (repeatable)
  --auto-workspace             Enable on-demand workspace discovery (opt-in)
  --allowed-root <PATH>        Allowed root prefix for auto-discovery (repeatable, required with --auto-workspace)
  --no-prewarm                 Skip Tantivy index prewarming on startup
  -v, --verbose                Increase log verbosity
  --config <PATH>              Custom config file path
```

### Flag Validation Rules

1. `--auto-workspace` without at least one `--allowed-root` -> startup error.
2. `--port` and `--bind` are ignored when `--transport stdio`.
3. `--allowed-root` paths are resolved via `realpath` at startup; nonexistent paths are rejected.
