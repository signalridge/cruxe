## Context

Extend the single-workspace stdio MCP server from 001-core-mvp into a multi-project, multi-transport system suitable for shared development environments. This adds multi-workspace auto-discovery (with security-constrained opt-in), index progress notifications via MCP protocol, restart-safe interrupted-job reporting, workspace warmset prewarming, and HTTP transport mode.

**Language/Version**: Rust (latest stable, 2024 edition)
**New Dependencies**: `axum` (HTTP framework), `tokio` (already present, add HTTP listener)
**Modified Crates**: `cruxe-mcp`, `cruxe-cli`, `cruxe-state`, `cruxe-core`
**Storage Changes**: `index_jobs.progress_token` column addition; `known_workspaces` table activation
**Testing**: cargo test + fixture repos with multi-workspace scenarios
**Constraints**: No authentication in HTTP v1 (explicit non-goal; local-only), `--auto-workspace` off by default
**Performance Goals**: `/health` p95 < 50ms, workspace routing overhead < 5ms per request, `index_status` polling p95 < 50ms

### Constitution Alignment

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

## Goals / Non-Goals

**Goals:**
1. Add `workspace` parameter to all MCP tools with centralized routing middleware.
2. Implement on-demand workspace auto-discovery with security constraints (`--auto-workspace`, `--allowed-root`, `realpath` validation).
3. Emit MCP `notifications/progress` during indexing with polling fallback.
4. Add axum-based HTTP transport as alternative to stdio with `/health` endpoint.
5. Implement bounded workspace warmset prewarming and interrupted-job recovery reporting.

**Non-Goals:**
1. Authentication for HTTP transport (deferred; local-only in v1).
2. New crate creation (all changes fit within existing crate boundaries).

## Decisions

### D1. Workspace routing is middleware, not per-tool logic

A workspace router intercepts all tool calls, resolves the workspace to a `ProjectContext`, and injects it into the tool handler. Tools do not implement workspace resolution individually.

**Why:** centralizes routing logic, reduces per-tool changes to parameter addition only.

### D2. On-demand indexing is async with immediate partial response

When a new workspace triggers indexing, the tool call returns immediately with `indexing_status: "indexing"`. The agent can poll `index_status` or listen for notifications.

**Why:** avoids blocking tool calls on potentially long indexing operations.

### D3. Progress notification is best-effort

Notifications are fire-and-forget (server-to-client, no ACK). If the client disconnects, notifications are dropped. Progress state is always queryable via `index_status`.

**Why:** MCP protocol has a defined `notifications/progress` spec; implementation is a thin wrapper with guaranteed fallback.

### D4. HTTP transport reuses the same tool dispatch

The HTTP handler deserializes JSON-RPC from HTTP POST body, dispatches through the same tool handler pipeline used by stdio, and serializes the response. No separate tool implementations.

**Why:** ensures response parity between transports; `stdio` and HTTP route through a shared transport-agnostic dispatch pipeline (`execute_transport_request`).

### D5. Workspace auto-injection is middleware-owned

When server startup sets a workspace context, omitted `workspace` fields are auto-filled at dispatch time. This keeps per-tool schemas simple while preserving explicit override behavior.

**Why:** avoids repetitive default-workspace logic in each tool handler.

### D6. Metadata enums are normalized

Responses use canonical `indexing_status` and `result_completeness` values shared with cross-spec contracts.

**Why:** consistent agent experience across tools and transports.

### D7. Interrupted recovery is status-first

Interrupted jobs are reconciled on startup and surfaced via status APIs before background reprocessing kicks in.

**Why:** agents get immediate visibility into recovery state without waiting for reprocessing to complete.

### D8. No new crates

All changes fit within existing crate boundaries. `workspace_router.rs` and `http.rs` are new modules within `cruxe-mcp`. `workspace.rs` is a new module within `cruxe-state`.

**Why:** avoids unnecessary crate proliferation for cohesive feature additions.

### Project Structure

Documentation:

```text
openspec/changes/archive/2026-02-22-004-workspace-transport/
├── proposal.md
├── design.md
├── contracts/
│   └── mcp-tools.md
└── tasks.md
```

Source code changes:

```text
crates/
├── cruxe-core/
│   └── src/
│       ├── types.rs              # + WorkspaceConfig, AllowedRoots types
│       └── error.rs              # + WorkspaceNotRegistered, WorkspaceNotAllowed errors
├── cruxe-state/
│   └── src/
│       ├── schema.rs             # + progress_token column migration for index_jobs
│       ├── workspace.rs          # NEW: known_workspaces CRUD, eviction logic
│       └── jobs.rs               # + progress_token field, interrupted reconciliation, progress data getters
├── cruxe-mcp/
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
└── cruxe-cli/
    └── src/
        └── commands/
            └── serve_mcp.rs      # + --transport, --port, --bind, --auto-workspace, --allowed-root flags
```

## Risks / Trade-offs

- **[Risk] `axum` adds moderate dependency weight** → **Mitigation:** `axum` is the standard Rust HTTP framework, already in the tokio ecosystem; no new runtime required.
- **[Risk] Auto-workspace could expose unintended directories** → **Mitigation:** `--auto-workspace` off by default, `--allowed-root` mandatory when enabled, all paths validated via `realpath` before allowlist check.
- **[Risk] HTTP transport without authentication** → **Mitigation:** bind to `127.0.0.1` by default (local-only); authentication deferred to a future phase with explicit opt-in for non-local bind.

## Migration Plan

### Phased Delivery

1. **Multi-workspace routing** (US1, P1) - Add `workspace` parameter to all tools, implement workspace resolution and validation. This is the foundation.
2. **Progress notifications** (US2, P2) - Wire notification protocol into the indexer pipeline. Independent of workspace routing.
3. **HTTP transport** (US3, P2) - Add axum-based HTTP server as alternative transport. Independent of workspace routing but benefits from it.
4. **Security hardening & cross-cutting** - Path security tests, warmset prewarming, interrupted-job recovery, CLI documentation.

### MVP First (US1)

1. Complete core types and state layer.
2. Complete workspace router.
3. **STOP and VALIDATE**: `workspace` parameter works on all tools, security constraints enforced.

Rollback: disable `--auto-workspace` and `--transport http` flags to revert to single-workspace stdio behavior.
