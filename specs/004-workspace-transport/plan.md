# Implementation Plan: Multi-Workspace & Transport

**Branch**: `004-workspace-transport` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/004-workspace-transport/spec.md`
**Depends On**: 003-structure-nav
**Version**: v0.3.0

## Summary

Add multi-workspace auto-discovery (with security-constrained opt-in), index
progress notifications via MCP protocol, restart-safe interrupted-job reporting,
workspace warmset prewarming, and HTTP transport mode to the existing
CodeCompass MCP server. This extends the single-workspace stdio server from
001-core-mvp into a multi-project, multi-transport system suitable for shared
development environments.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**New Dependencies**: `axum` (HTTP framework), `tokio` (already present, add HTTP listener)
**Modified Crates**: `codecompass-mcp`, `codecompass-cli`, `codecompass-state`, `codecompass-core`
**Storage Changes**: `index_jobs.progress_token` column addition; `known_workspaces` table activation
**Testing**: cargo test + fixture repos with multi-workspace scenarios
**Constraints**: No authentication in HTTP v1 (local-only), `--auto-workspace` off by default
**Performance Goals**: `/health` p95 < 50ms, workspace routing overhead < 5ms per request, `index_status` polling p95 < 50ms

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | All existing navigation tools gain `workspace` parameter |
| II. Single Binary Distribution | PASS | `axum` compiles into the binary; no external service |
| III. Branch/Worktree Correctness | PASS | Workspace routing is orthogonal to ref scoping; ref isolation preserved |
| IV. Incremental by Design | PASS | On-demand indexing reuses existing incremental pipeline |
| V. Agent-Aware Response Design | PASS | Progress notifications reduce polling; workspace parameter reduces reconfiguration |
| VI. Fail-Soft Operation | PASS | Notification fallback to polling; auto-workspace disabled by default |
| VII. Explainable Ranking | N/A | No ranking changes in this spec |

### Security Constitution Alignment

| Guardrail | Implementation | Status |
|-----------|---------------|--------|
| Auto-workspace requires explicit opt-in | `--auto-workspace` flag, off by default | PASS |
| Mandatory allowed-root constraints | `--allowed-root` required when `--auto-workspace` enabled | PASS |
| Path validation via realpath | All workspace paths resolved via `realpath` before allowlist check | PASS |
| No shell execution from parameters | Workspace paths used only for index routing, never passed to shell | PASS |
| HTTP local-only by default | Bind to `127.0.0.1` unless `--bind` explicitly overrides | PASS |

## Project Structure

### Documentation (this feature)

```text
specs/004-workspace-transport/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # Updated tool contracts
│   └── mcp-tools.md    # Workspace param, progress notifications, /health
└── tasks.md             # Actionable task list
```

### Source Code Changes

```text
crates/
├── codecompass-core/
│   └── src/
│       ├── types.rs              # + WorkspaceConfig, AllowedRoots types
│       └── error.rs              # + WorkspaceNotRegistered, WorkspaceNotAllowed errors
├── codecompass-state/
│   └── src/
│       ├── schema.rs             # + progress_token column migration for index_jobs
│       ├── workspace.rs          # NEW: known_workspaces CRUD, eviction logic
│       └── jobs.rs               # + progress_token field, interrupted reconciliation, progress data getters
├── codecompass-mcp/
│   └── src/
│       ├── server.rs             # + workspace routing middleware, notification support
│       ├── http.rs               # NEW: axum HTTP transport, /health endpoint
│       ├── notifications.rs      # NEW: progress notification emitter
│       ├── workspace_router.rs   # NEW: workspace resolution, validation, on-demand index
│       ├── warmset.rs            # NEW: bounded recent-workspace prewarm selection
│       └── tools/
│           ├── mod.rs            # + workspace param in all tool handlers
│           ├── index_repo.rs     # + workspace param, progress token plumbing
│           ├── sync_repo.rs      # + workspace param
│           ├── search_code.rs    # + workspace param
│           ├── locate_symbol.rs  # + workspace param
│           ├── index_status.rs   # + workspace param, progress data in response
│           ├── get_file_outline.rs    # + workspace param
│           ├── health_check.rs        # + workspace param (optional scope)
│           ├── get_symbol_hierarchy.rs # + workspace param
│           ├── find_related_symbols.rs # + workspace param
│           └── get_code_context.rs     # + workspace param
└── codecompass-cli/
    └── src/
        └── commands/
            └── serve_mcp.rs      # + --transport, --port, --bind, --auto-workspace, --allowed-root flags
```

**Structure Decision**: No new crates. All changes fit within existing crate
boundaries. `workspace_router.rs` and `http.rs` are new modules within
`codecompass-mcp`. `workspace.rs` is a new module within `codecompass-state`.

## Implementation Strategy

### Phased Delivery

1. **Multi-workspace routing** (US1) - Add `workspace` parameter to all tools,
   implement workspace resolution and validation. This is the foundation.
2. **Progress notifications** (US2) - Wire notification protocol into the indexer
   pipeline. Independent of workspace routing.
3. **HTTP transport** (US3) - Add axum-based HTTP server as alternative transport.
   Independent of workspace routing but benefits from it.

### Key Design Decisions

1. **Workspace routing is middleware, not per-tool logic**: A workspace router
   intercepts all tool calls, resolves the workspace to a `ProjectContext`, and
   injects it into the tool handler. Tools do not implement workspace resolution
   individually.

2. **On-demand indexing is async with immediate partial response**: When a new
   workspace triggers indexing, the tool call returns immediately with
   `indexing_status: "indexing"`. The agent can poll `index_status` or listen for
   notifications.

3. **Progress notification is best-effort**: Notifications are fire-and-forget
   (server-to-client, no ACK). If the client disconnects, notifications are dropped.
   Progress state is always queryable via `index_status`.

4. **HTTP transport reuses the same tool dispatch**: The HTTP handler deserializes
   JSON-RPC from HTTP POST body, dispatches through the same tool handler pipeline
   used by stdio, and serializes the response. No separate tool implementations.

5. **Workspace auto-injection is middleware-owned**: When server startup sets a
   workspace context, omitted `workspace` fields are auto-filled at dispatch time.
   This keeps per-tool schemas simple while preserving explicit override behavior.

6. **Metadata enums are normalized in this phase**: Responses emitted by this
   pipeline use canonical `indexing_status` and `result_completeness` values
   shared with cross-spec contracts.
7. **Interrupted recovery is status-first**: interrupted jobs are reconciled on
   startup and surfaced via status APIs before background reprocessing kicks in.

## Complexity Tracking

| Decision | Complexity | Justification |
|----------|-----------|---------------|
| `axum` for HTTP | Moderate | Required for HTTP transport; `axum` is the standard Rust HTTP framework, already in the tokio ecosystem |
| Workspace middleware | Low | Centralizes routing logic, reduces per-tool changes to parameter addition only |
| Progress notifications | Low | MCP protocol has a defined `notifications/progress` spec; implementation is a thin wrapper |
| `--allowed-root` enforcement | Low | Simple prefix check after `realpath`; critical for security |
