# Feature Specification: Multi-Workspace & Transport

**Feature Branch**: `004-workspace-transport`
**Created**: 2026-02-23
**Status**: Draft
**Version**: v0.3.0
**Phase**: 1.5b
**Depends On**: 003-structure-nav
**Input**: `specs/meta/design.md` sections 10.7 (Multi-workspace auto-discovery), 10.9 (MCP health/readiness), 10.10 (Index progress notifications), and HTTP transport design

## Implementation Alignment Update (2026-02-25)

- `stdio` and HTTP now route through a shared transport-agnostic dispatch execution
  pipeline (`execute_transport_request`).
- Health aggregation logic is shared by `GET /health` and `health_check` through
  a common core payload builder, while preserving endpoint-specific envelopes.
- Runtime SQLite access now uses a lightweight `ConnectionManager` abstraction for
  lazy open/reuse/invalidate semantics across transports.
- `index_repo/sync_repo` and auto-bootstrap use a unified index process launcher
  with deterministic binary resolution and env propagation.

## User Scenarios & Testing

### User Story 1 - Multi-Workspace Search via MCP (Priority: P1)

An AI coding agent working across multiple projects in the same session sends a
`search_code` request with a `workspace` parameter pointing to a second project.
Cruxe resolves the workspace, routes the query to the correct project index,
and returns results scoped to that workspace. If the workspace has not been indexed
yet and `--auto-workspace` is enabled, Cruxe triggers on-demand indexing and
returns partial results with appropriate status metadata. When MCP server startup
pins a workspace context, the server auto-injects that workspace for requests that
omit `workspace`.

**Why this priority**: AI agents routinely work across multiple repositories in a
single session. Without multi-workspace support, users must restart the MCP server
or run separate instances per project, creating friction.

**Independent Test**: Start `cruxe serve-mcp --auto-workspace --allowed-root /tmp`,
send a `search_code` request with `workspace: "/tmp/project-b"` (a valid but
unindexed repo), verify the response includes `indexing_status: "indexing"` and
a subsequent query returns indexed results.

**Acceptance Scenarios**:

1. **Given** two indexed projects A and B, **When** `locate_symbol` is called with
   `workspace: "/path/to/project-b"`, **Then** results are returned from project B's
   index, not project A's.
2. **Given** `--auto-workspace` is enabled with `--allowed-root /home/dev`, **When**
   `search_code` is called with `workspace: "/home/dev/new-project"` (valid path,
   not yet indexed), **Then** the workspace is registered in `known_workspaces`,
   on-demand indexing starts, and the response includes
   `indexing_status: "indexing"`, `result_completeness: "partial"`.
3. **Given** `--auto-workspace` is disabled, **When** a request arrives with an
   unknown `workspace`, **Then** the response returns error code
   `workspace_not_registered` with guidance to pre-register or enable auto-workspace.
4. **Given** `--auto-workspace` is enabled with `--allowed-root /home/dev`, **When**
   a request arrives with `workspace: "/etc/shadow"` (outside allowed root), **Then**
   the response returns error code `workspace_not_allowed`.
5. **Given** a request with `workspace: "/home/dev/project/../../../etc/passwd"`,
   **When** the path is resolved via `realpath`, **Then** it resolves to `/etc/passwd`,
   fails the `--allowed-root` check, and returns `workspace_not_allowed`.
6. **Given** the `workspace` parameter is omitted, **When** any tool is called,
   **Then** the default registered project (from `cruxe init`) is used.
7. **Given** `--auto-workspace` is enabled, **When** 10 workspaces are already
   auto-discovered and an 11th is requested, **Then** the least-recently-used
   workspace is evicted before registering the new one (configurable max).
8. **Given** all existing MCP tools (`index_repo`, `sync_repo`, `search_code`,
   `locate_symbol`, `index_status`, `get_file_outline`, `health_check`,
   `get_symbol_hierarchy`, `find_related_symbols`, `get_code_context`), **When**
   `tools/list` is called, **Then** every tool schema includes `workspace` as an
   optional string parameter.
9. **Given** `serve-mcp --workspace /home/dev/project-a`, **When** `search_code`
   is called without a `workspace` field, **Then** server middleware auto-injects
   `/home/dev/project-a` and executes scoped search successfully.
10. **Given** warmset prewarm is enabled, **When** server starts with 5 known
   workspaces, **Then** only the most recently used bounded subset (for example
   top 3) is prewarmed and exposed in health metadata.

---

### User Story 2 - Index Progress Notifications (Priority: P2)

An AI agent triggers `index_repo` on a large workspace. Instead of polling
`index_status` repeatedly, the agent receives MCP `notifications/progress` messages
showing files scanned, files indexed, symbols extracted, and estimated completion
percentage. When the agent does not support notifications, the same progress
information is available via `index_status` polling.

**Why this priority**: Bootstrap indexing of large repositories can take 30-60
seconds. Without progress feedback, agents waste tokens polling or give up
prematurely.

**Independent Test**: Call `index_repo` on a fixture repository with 100+ files,
capture the notification stream, verify at least 3 progress notifications are
emitted before the completion notification.

**Acceptance Scenarios**:

1. **Given** an MCP client that supports notifications (declared in `initialize`),
   **When** `index_repo` is called, **Then** the server emits
   `notifications/progress` messages with `progressToken` matching the job ID,
   including `percentage`, `title`, and `message` fields.
2. **Given** a progress notification stream, **When** indexing completes, **Then**
   a final notification with `kind: "end"` is emitted containing total files indexed,
   symbols extracted, and duration.
3. **Given** an MCP client that does NOT declare notification support, **When**
   `index_repo` is called, **Then** no notifications are emitted but `index_status`
   returns the same progress data (files_scanned, files_indexed, symbols_extracted,
   estimated_completion_pct) via polling.
4. **Given** an active indexing job, **When** `index_status` is called with the
   job's `workspace`, **Then** the response includes `active_job` with
   `progress_token`, `files_scanned`, `files_indexed`, `symbols_extracted`,
   `estimated_completion_pct`.

---

### User Story 3 - HTTP Transport Mode (Priority: P2)

A developer wants to share a single Cruxe instance across multiple terminal
sessions or tools. They start `cruxe serve-mcp --transport http --port 9100`
and configure their AI agents to connect via HTTP. The `/health` endpoint confirms
the server is ready before the agent sends tool requests.

**Why this priority**: stdio transport limits MCP to a single connected client.
HTTP transport enables multi-client scenarios (multiple editors, CI integration,
shared team workstation) without running separate server instances.

**Independent Test**: Start `cruxe serve-mcp --transport http --port 9100`,
`curl http://127.0.0.1:9100/health` returns 200 with status `"ready"`, then send
an MCP `tools/list` request via HTTP POST and verify the response.

**Acceptance Scenarios**:

1. **Given** `serve-mcp --transport http --port 9100`, **When** `GET /health` is
   requested, **Then** the response is 200 OK with JSON body containing `status`,
   `projects`, `version`, and `uptime_seconds`.
2. **Given** a server that has just started and is prewarming, **When** `GET /health`
   is called, **Then** `status` is `"warming"`.
3. **Given** a server with one project actively indexing, **When** `GET /health` is
   called, **Then** `status` is `"indexing"` and the relevant project in `projects[]`
   shows `index_status: "indexing"`.
4. **Given** `serve-mcp --transport http --port 9100`, **When** an MCP JSON-RPC
   `tools/call` request is sent via HTTP POST to the server, **Then** the response
   matches the same format as stdio transport.
5. **Given** `serve-mcp --transport http` without `--port`, **Then** the server
   binds to the default port (configurable, e.g. 9100).
6. **Given** `serve-mcp --transport http --bind 0.0.0.0`, **When** the server starts,
   **Then** it binds to all interfaces (developer explicitly opted in).
7. **Given** default configuration (no `--bind`), **When** the server starts in HTTP
   mode, **Then** it binds to `127.0.0.1` only (local-only by default).
8. **Given** HTTP transport mode, **When** all existing MCP tools are called via
   HTTP POST, **Then** responses are identical to stdio transport responses.

---

### Edge Cases

- What happens when `--auto-workspace` is enabled but `--allowed-root` is not set?
  The server refuses to start and logs an error: `--allowed-root is required when
  --auto-workspace is enabled`.
- What happens when a symlink in a workspace path resolves outside the allowed root?
  `realpath` is applied before the allowlist check, so the resolved path is validated,
  not the symlink path. The request is rejected with `workspace_not_allowed`.
- What happens when two concurrent requests trigger on-demand indexing for the same workspace?
  The second request detects the in-progress job and reuses it. Both requests receive
  `indexing_status: "indexing"`.
- What happens when the HTTP port is already in use?
  The server exits with a clear error: `Port 9100 is already in use. Choose a
  different port with --port.`
- What happens when an index job fails mid-progress?
  The progress notification emits `kind: "end"` with an error message. `index_status`
  shows the job in `failed` state.
- What happens when the process restarts while indexing is running?
  Unfinished jobs are marked `interrupted`; `index_status` and `health_check`
  include an `interrupted_recovery_report` until a successful follow-up sync/index.
- What happens when `workspace` is a relative path?
  It is resolved to an absolute path via `realpath` before any validation or routing.
- What happens when the HTTP server receives a request with no Content-Type header?
  It assumes `application/json` for MCP requests, returns 400 for malformed bodies.

## Requirements

### Functional Requirements

- **FR-300**: All query/path MCP tools (existing and future additions) MUST accept an
  optional `workspace` string parameter specifying the target workspace path.
- **FR-301**: When `workspace` is omitted, the system MUST use the default registered
  project (from `cruxe init` or `--workspace` flag on startup).
- **FR-302**: When `workspace` points to an indexed project, the system MUST route
  queries to that project's indices.
- **FR-303**: When `workspace` points to an unknown path and `--auto-workspace` is
  disabled, the system MUST return error code `workspace_not_registered`.
- **FR-304**: When `workspace` points to an unknown path and `--auto-workspace` is
  enabled, the system MUST validate the path via `realpath` against `--allowed-root`
  prefixes before registering.
- **FR-305**: When `workspace` resolves outside all `--allowed-root` prefixes, the
  system MUST return error code `workspace_not_allowed`.
- **FR-306**: On-demand indexing for auto-discovered workspaces MUST register the
  workspace in `known_workspaces` with `auto_discovered = 1` and trigger bootstrap
  indexing.
- **FR-307**: The `known_workspaces` table MUST track `last_used_at` and support
  eviction of stale auto-discovered workspaces (configurable max count, default 10).
- **FR-308**: The `--auto-workspace` CLI flag on `serve-mcp` MUST be opt-in (off by
  default).
- **FR-309**: When `--auto-workspace` is enabled, at least one `--allowed-root` MUST
  be provided or the server MUST refuse to start.
- **FR-310**: The system MUST emit MCP `notifications/progress` messages during indexing
  when the client declares notification support in `initialize`.
- **FR-311**: Progress notifications MUST include `progressToken` (matching job ID),
  `percentage`, `title`, and `message` fields.
- **FR-312**: Progress notifications MUST report: files scanned, files indexed,
  symbols extracted, and estimated completion.
- **FR-313**: A `kind: "end"` notification MUST be emitted when indexing completes
  (success or failure).
- **FR-314**: The `index_jobs` table MUST include a `progress_token` field for
  tracking notification state.
- **FR-315**: When the client does not support notifications, the same progress data
  MUST be available via `index_status` tool polling.
- **FR-316**: The system MUST support `serve-mcp --transport http --port PORT` for
  HTTP transport mode.
- **FR-317**: HTTP transport mode MUST expose a `GET /health` endpoint returning
  server status, project list, version, and uptime.
- **FR-318**: The `/health` endpoint MUST return status values: `ready`, `warming`,
  `indexing`, or `error`.
- **FR-319**: HTTP transport mode MUST expose all MCP tools via HTTP POST with
  JSON-RPC 2.0 protocol.
- **FR-320**: HTTP transport MUST bind to `127.0.0.1` by default (local-only).
- **FR-321**: HTTP transport MUST support a `--bind` flag to configure the bind
  address.
- **FR-322**: HTTP transport MUST NOT include authentication in v1 (local-only
  use case).
- **FR-323**: All `workspace` path inputs MUST be validated via `realpath` against
  the allowed roots allowlist before any filesystem or index operation.
- **FR-324**: MCP server middleware MUST support workspace auto-injection from
  startup context when request payload omits `workspace`.
- **FR-325**: All tool responses in this spec MUST use canonical metadata enums:
  `indexing_status` = `not_indexed | indexing | ready | failed`,
  `result_completeness` = `complete | partial | truncated`.
- **FR-326**: System MUST maintain a bounded workspace warmset (default capacity: 3,
  configurable) derived from `known_workspaces.last_used_at` and prewarm only warmset
  members during startup. The `--no-prewarm` flag MUST skip warmset prewarming entirely.
- **FR-327**: `index_status` and `health_check` MUST expose
  `interrupted_recovery_report` when interrupted jobs are detected after restart.

### Key Entities

- **KnownWorkspace**: A registered workspace entry with absolute path, associated
  project ID, auto-discovery flag, last-used timestamp, and index status. Stored
  in `known_workspaces` table (schema from 001-core-mvp).
- **ProgressNotification**: An MCP server-to-client notification carrying indexing
  progress data, identified by a progress token tied to an `index_jobs` entry.
- **HealthStatus**: Server-level readiness state (`ready`, `warming`, `indexing`,
  `error`) with per-project detail, exposed via `/health` (HTTP) or `health_check`
  tool (stdio).

## Success Criteria

### Measurable Outcomes

- **SC-300**: An AI agent can query across 3+ workspaces in a single MCP session
  without restarting the server.
- **SC-301**: On-demand indexing of a new workspace completes and subsequent queries
  return full results within 60 seconds for a 5,000-file repository.
- **SC-302**: Progress notifications are emitted at least every 5 seconds during
  indexing of repositories with 1,000+ files.
- **SC-303**: The `/health` endpoint responds in under 50ms regardless of server load.
- **SC-304**: HTTP transport tool responses are byte-identical in JSON content to
  stdio transport responses for the same inputs.
- **SC-305**: Path traversal and symlink escape attempts are blocked with zero
  false negatives in the security test suite.
- **SC-306**: Contract tests fail build when any query/path tool omits the
  `workspace` parameter in `tools/list` schema output.
- **SC-307**: Warmset-enabled startup reduces first-query p95 latency for recent
  workspaces to < 400ms without increasing total startup time by more than 15%.
- **SC-308**: After restart with interrupted jobs, `interrupted_recovery_report`
  is visible within 1s and cleared automatically after successful remediation.
