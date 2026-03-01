## Why

AI agents routinely work across multiple repositories in a single session. Without multi-workspace support, users must restart the MCP server or run separate instances per project, creating friction. Additionally, bootstrap indexing of large repositories can take 30-60 seconds with no progress feedback, and stdio transport limits MCP to a single connected client, preventing multi-client scenarios such as shared team workstations and CI integration.

## What Changes

1. Add multi-workspace auto-discovery with security-constrained opt-in: all MCP tools accept an optional `workspace` parameter, with centralized routing, `--auto-workspace` / `--allowed-root` enforcement, and LRU eviction (FR-300 through FR-309, FR-323, FR-324).
2. Add index progress notifications via MCP `notifications/progress` protocol, with polling fallback for clients that do not support notifications (FR-310 through FR-315).
3. Add HTTP transport mode (`serve-mcp --transport http`) with `/health` endpoint, JSON-RPC over HTTP POST, and local-only default bind (FR-316 through FR-322).
4. Add bounded workspace warmset prewarming on startup and interrupted-job recovery reporting (FR-325 through FR-327).

## Capabilities

### New Capabilities

- **multi-workspace-routing**: Centralized workspace resolution middleware for all MCP tool calls with on-demand indexing, path validation via `realpath`, and LRU eviction of auto-discovered workspaces.
  - FR-300: All query/path MCP tools MUST accept an optional `workspace` string parameter.
  - FR-301: When `workspace` is omitted, the system MUST use the default registered project.
  - FR-302: When `workspace` points to an indexed project, the system MUST route queries to that project's indices.
  - FR-303: When `workspace` points to an unknown path and `--auto-workspace` is disabled, the system MUST return error code `workspace_not_registered`.
  - FR-304: When `workspace` points to an unknown path and `--auto-workspace` is enabled, the system MUST validate the path via `realpath` against `--allowed-root` prefixes before registering.
  - FR-305: When `workspace` resolves outside all `--allowed-root` prefixes, the system MUST return error code `workspace_not_allowed`.
  - FR-306: On-demand indexing for auto-discovered workspaces MUST register the workspace in `known_workspaces` with `auto_discovered = 1` and trigger bootstrap indexing.
  - FR-307: The `known_workspaces` table MUST track `last_used_at` and support eviction of stale auto-discovered workspaces (configurable max count, default 10).
  - FR-308: The `--auto-workspace` CLI flag on `serve-mcp` MUST be opt-in (off by default).
  - FR-309: When `--auto-workspace` is enabled, at least one `--allowed-root` MUST be provided or the server MUST refuse to start.
  - FR-323: All `workspace` path inputs MUST be validated via `realpath` against the allowed roots allowlist before any filesystem or index operation.
  - FR-324: MCP server middleware MUST support workspace auto-injection from startup context when request payload omits `workspace`.

- **index-progress-notifications**: MCP `notifications/progress` messages during indexing with polling fallback via `index_status`.
  - FR-310: The system MUST emit MCP `notifications/progress` messages during indexing when the client declares notification support in `initialize`.
  - FR-311: Progress notifications MUST include `progressToken` (matching job ID), `percentage`, `title`, and `message` fields.
  - FR-312: Progress notifications MUST report: files scanned, files indexed, symbols extracted, and estimated completion.
  - FR-313: A `kind: "end"` notification MUST be emitted when indexing completes (success or failure).
  - FR-314: The `index_jobs` table MUST include a `progress_token` field for tracking notification state.
  - FR-315: When the client does not support notifications, the same progress data MUST be available via `index_status` tool polling.

- **http-transport**: axum-based HTTP server as alternative to stdio transport with `/health` endpoint.
  - FR-316: The system MUST support `serve-mcp --transport http --port PORT` for HTTP transport mode.
  - FR-317: HTTP transport mode MUST expose a `GET /health` endpoint returning server status, project list, version, and uptime.
  - FR-318: The `/health` endpoint MUST return status values: `ready`, `warming`, `indexing`, or `error`.
  - FR-319: HTTP transport mode MUST expose all MCP tools via HTTP POST with JSON-RPC 2.0 protocol.
  - FR-320: HTTP transport MUST bind to `127.0.0.1` by default (local-only).
  - FR-321: HTTP transport MUST support a `--bind` flag to configure the bind address.
  - FR-322: HTTP transport MUST NOT include authentication in v1 (local-only use case).

### Modified Capabilities

- **002-agent-protocol**: All tool responses use canonical metadata enums and include workspace-scoped context.
  - FR-325: All tool responses MUST use canonical metadata enums: `indexing_status` = `not_indexed | indexing | ready | failed`, `result_completeness` = `complete | partial | truncated`.
  - FR-326: System MUST maintain a bounded workspace warmset (default capacity: 3, configurable) derived from `known_workspaces.last_used_at` and prewarm only warmset members during startup. The `--no-prewarm` flag MUST skip warmset prewarming entirely.
  - FR-327: `index_status` and `health_check` MUST expose `interrupted_recovery_report` when interrupted jobs are detected after restart.

### Key Entities

- **KnownWorkspace**: A registered workspace entry with absolute path, associated project ID, auto-discovery flag, last-used timestamp, and index status. Stored in `known_workspaces` table (schema from 001-core-mvp).
- **ProgressNotification**: An MCP server-to-client notification carrying indexing progress data, identified by a progress token tied to an `index_jobs` entry.
- **HealthStatus**: Server-level readiness state (`ready`, `warming`, `indexing`, `error`) with per-project detail, exposed via `/health` (HTTP) or `health_check` tool (stdio).

## Impact

- SC-300: An AI agent can query across 3+ workspaces in a single MCP session without restarting the server.
- SC-301: On-demand indexing of a new workspace completes and subsequent queries return full results within 60 seconds for a 5,000-file repository.
- SC-302: Progress notifications are emitted at least every 5 seconds during indexing of repositories with 1,000+ files.
- SC-303: The `/health` endpoint responds in under 50ms regardless of server load.
- SC-304: HTTP transport tool responses are byte-identical in JSON content to stdio transport responses for the same inputs.
- SC-305: Path traversal and symlink escape attempts are blocked with zero false negatives in the security test suite.
- SC-306: Contract tests fail build when any query/path tool omits the `workspace` parameter in `tools/list` schema output.
- SC-307: Warmset-enabled startup reduces first-query p95 latency for recent workspaces to < 400ms without increasing total startup time by more than 15%.
- SC-308: After restart with interrupted jobs, `interrupted_recovery_report` is visible within 1s and cleared automatically after successful remediation.

Edge cases:
- `--auto-workspace` enabled without `--allowed-root`: server refuses to start with error.
- Symlink in workspace path resolving outside allowed root: `realpath` is applied before allowlist check; request rejected with `workspace_not_allowed`.
- Two concurrent requests trigger on-demand indexing for the same workspace: second request reuses the in-progress job; both receive `indexing_status: "indexing"`.
- HTTP port already in use: server exits with clear error message.
- Index job fails mid-progress: progress notification emits `kind: "end"` with error; `index_status` shows `failed` state.
- Process restarts while indexing: unfinished jobs marked `interrupted`; `interrupted_recovery_report` exposed until successful remediation.
- `workspace` is a relative path: resolved to absolute via `realpath` before validation.
- HTTP request with no Content-Type header: assumes `application/json` for MCP requests; returns 400 for malformed bodies.

Affected crates: `cruxe-mcp`, `cruxe-cli`, `cruxe-state`, `cruxe-core`.
API impact: additive `workspace` parameter on all tools, additive metadata fields, new `/health` endpoint.
Performance targets: `/health` p95 < 50ms, workspace routing overhead < 5ms per request, warmset first-query p95 < 400ms.

Note (2026-02-25 alignment update): `stdio` and HTTP now route through a shared transport-agnostic dispatch pipeline (`execute_transport_request`). Health aggregation logic is shared by `GET /health` and `health_check` through a common core payload builder. Runtime SQLite access uses a lightweight `ConnectionManager` abstraction for lazy open/reuse/invalidate semantics. `index_repo`/`sync_repo` and auto-bootstrap use a unified index process launcher with deterministic binary resolution and env propagation.
