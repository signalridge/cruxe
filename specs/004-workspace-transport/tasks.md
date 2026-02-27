# Tasks: Multi-Workspace & Transport

**Input**: Design documents from `/specs/004-workspace-transport/`
**Prerequisites**: plan.md (required), spec.md (required), contracts/mcp-tools.md
**Depends On**: 003-structure-nav must be complete before starting

> Status note (2026-02-24): All listed implementation, integration/security test,
> and benchmark tasks are now covered in code and tests. This checklist remains
> as the execution trace for 004 delivery.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US3)
- Include exact file paths in descriptions

## Phase 1: Core Types and State Layer

**Purpose**: Workspace types, error codes, state table operations, schema migration

- [X] T196 [US1] Add workspace error variants to `crates/cruxe-core/src/error.rs`: `WorkspaceNotRegistered { path }`, `WorkspaceNotAllowed { path, reason }`, `WorkspaceAutoDiscoveryDisabled`, `WorkspaceLimitExceeded { max }`, `AllowedRootRequired`
- [X] T197 [P] [US1] Add workspace config types to `crates/cruxe-core/src/types.rs`: `WorkspaceConfig { auto_workspace: bool, allowed_roots: Vec<PathBuf>, max_auto_workspaces: usize }`, `AllowedRoots` newtype with `contains(path) -> bool` prefix check method
- [X] T198 [P] [US1] Add `validate_workspace_path(path, allowed_roots) -> Result<PathBuf>` function to `crates/cruxe-core/src/types.rs`: resolve via `std::fs::canonicalize` (realpath equivalent), verify resolved path starts with at least one allowed root prefix
- [X] T199 [US1] Add `progress_token` column to `index_jobs` table in `crates/cruxe-state/src/schema.rs`: `ALTER TABLE index_jobs ADD COLUMN progress_token TEXT` migration, update DDL for fresh installs
- [X] T200 [US1] Implement `crates/cruxe-state/src/workspace.rs`: `register_workspace(path, project_id, auto_discovered) -> Result`, `get_workspace(path) -> Option<KnownWorkspace>`, `update_last_used(path)`, `list_workspaces() -> Vec<KnownWorkspace>`, `evict_lru_auto_discovered(max_count) -> Vec<evicted_paths>`, `delete_workspace(path)`
- [X] T201 [P] [US1] Write unit tests for `AllowedRoots::contains()`: verify prefix matching, path traversal rejection, symlink resolution, case sensitivity on macOS
- [X] T202 [P] [US1] Write unit tests for `validate_workspace_path()`: verify `realpath` resolution, allowed root check, rejection of paths outside allowlist, handling of nonexistent paths
- [X] T203 [P] [US1] Write unit tests for workspace CRUD in `crates/cruxe-state/src/workspace.rs`: register, get, update, evict LRU, delete

**Checkpoint**: Workspace types defined, state operations tested, schema migration ready

---

## Phase 2: Workspace Router (Multi-Workspace Core)

**Purpose**: Centralized workspace resolution middleware for all MCP tool calls

- [X] T204 [US1] Implement `crates/cruxe-mcp/src/workspace_router.rs`: `WorkspaceRouter` struct holding `WorkspaceConfig`, state DB handle, and indexer handle. Core method: `resolve_workspace(workspace_param: Option<String>) -> Result<ProjectContext>` that handles: (1) None -> default or auto-injected startup workspace, (2) known workspace -> load project, (3) unknown + auto-workspace enabled -> validate path, register, trigger on-demand index, (4) unknown + auto-workspace disabled -> error
- [X] T205 [US1] Implement on-demand indexing trigger in `workspace_router.rs`: when auto-discovering a new workspace, call `register_workspace(auto_discovered=1)`, create project entry, spawn async indexing task, return `ProjectContext` with `indexing_status: "indexing"`
- [X] T206 [US1] Implement startup validation in `workspace_router.rs`: if `auto_workspace` is true and `allowed_roots` is empty, return `AllowedRootRequired` error at server startup (not at first request)
- [X] T207 [US1] Add `workspace: Option<String>` parameter to all 10 MCP tool input schemas in `crates/cruxe-mcp/src/tools/`: `index_repo`, `sync_repo`, `search_code`, `locate_symbol`, `index_status`, `get_file_outline`, `health_check`, `get_symbol_hierarchy`, `find_related_symbols`, `get_code_context`; add a contract test guard that fails when future query/path tools omit `workspace`
- [X] T208 [US1] Wire `WorkspaceRouter::resolve_workspace()` into the MCP server dispatch loop in `crates/cruxe-mcp/src/server.rs`: extract `workspace` param from tool call input, resolve to `ProjectContext`, pass context to tool handler
- [X] T209 [US1] Write integration test: start MCP server with two pre-registered workspaces, call `locate_symbol` with each workspace, verify results come from the correct project index
- [X] T210 [US1] Write integration test: start MCP server with `--auto-workspace --allowed-root /tmp`, call `search_code` with unknown workspace under `/tmp`, verify workspace is registered and indexing starts
- [X] T211 [US1] Write integration test: call with workspace outside allowed root, verify `workspace_not_allowed` error
- [X] T212 [US1] Write integration test: call with unknown workspace and `--auto-workspace` disabled, verify `workspace_not_registered` error

**Checkpoint**: Multi-workspace routing works for all tools, security constraints enforced

---

## Phase 3: Index Progress Notifications

**Purpose**: MCP `notifications/progress` during indexing operations

- [X] T213 [US2] Implement `crates/cruxe-mcp/src/notifications.rs`: `ProgressNotifier` trait with `emit_progress(token, title, message, percentage)` and `emit_end(token, title, message)` methods. Two implementations: `McpProgressNotifier` (sends JSON-RPC notifications to client) and `NullProgressNotifier` (no-op for clients without notification support)
- [X] T214 [US2] Detect client notification capability in `crates/cruxe-mcp/src/server.rs`: check `initialize` request for notification support declaration, select `McpProgressNotifier` or `NullProgressNotifier` accordingly
- [X] T215 [US2] Wire `ProgressNotifier` into the indexer pipeline in `crates/cruxe-indexer/src/writer.rs` (or `lib.rs`): emit progress at scanner completion (files discovered), during parsing (files parsed / total), during writing (files indexed / total, symbols extracted), and at completion
- [X] T216 [US2] Generate `progress_token` in `index_repo` tool handler (`crates/cruxe-mcp/src/tools/index_repo.rs`): use `format!("index-job-{}", job_id)`, store in `index_jobs.progress_token`, pass to indexer pipeline
- [X] T217 [US2] Update `index_status` tool response in `crates/cruxe-mcp/src/tools/index_status.rs`: add `files_scanned`, `files_indexed`, `symbols_extracted`, `estimated_completion_pct` to `active_job` in response when a job is running
- [X] T218 [US2] Update `crates/cruxe-state/src/jobs.rs`: add `progress_token`, `files_scanned`, `files_indexed`, `symbols_extracted` fields to job record, add `update_progress(job_id, files_scanned, files_indexed, symbols_extracted)` method
- [X] T219 [US2] Write integration test: start MCP server, call `index_repo` on fixture repo with 50+ files, capture notification stream via test client, verify at least 3 progress notifications and 1 end notification
- [X] T220 [P] [US2] Write integration test: start MCP server with client that does NOT declare notification support, call `index_repo`, verify no notifications emitted, then call `index_status` and verify progress fields are populated
- [X] T221 [P] [US2] Write unit test for `ProgressNotifier` trait: verify JSON-RPC notification format matches MCP spec (`method: "notifications/progress"`, `params.progressToken`, `params.value.kind`, `params.value.percentage`)

**Checkpoint**: Progress notifications emitted during indexing, fallback to polling works

---

## Phase 4: HTTP Transport Mode

**Purpose**: axum-based HTTP server as alternative to stdio transport

- [X] T222 [US3] Add `axum` and `tower` dependencies to `crates/cruxe-mcp/Cargo.toml`
- [X] T223 [US3] Implement `crates/cruxe-mcp/src/http.rs`: `HttpTransport` struct with `start(bind_addr, port, tool_dispatcher) -> Result` method. Routes: `GET /health` -> health handler (including compatibility aggregation), `POST /` -> JSON-RPC MCP handler (reuses existing tool dispatch)
- [X] T224 [US3] Implement `/health` endpoint in `crates/cruxe-mcp/src/http.rs`: return `{ status, projects, version, uptime_seconds }` per contract with per-project compatibility fields (`schema_status`, `current_schema_version`, `required_schema_version`). Status logic: `warming` if prewarm in progress, `indexing` if any project is actively indexing, `error` if any project has failed status, `ready` otherwise
- [X] T225 [US3] Implement HTTP JSON-RPC handler in `crates/cruxe-mcp/src/http.rs`: parse JSON-RPC request from POST body, route `tools/list` and `tools/call` through the same dispatcher used by stdio, serialize JSON-RPC response
- [X] T226 [US3] Update `serve-mcp` CLI command in `crates/cruxe-cli/src/commands/serve_mcp.rs`: add `--transport` flag (values: `stdio` [default], `http`), add `--port` flag (default: 9100), add `--bind` flag (default: `127.0.0.1`), add `--auto-workspace` flag (default: false), add `--allowed-root` flag (repeatable)
- [X] T227 [US3] Implement transport selection in `serve_mcp.rs`: if `--transport stdio` -> start existing stdio server loop, if `--transport http` -> start axum HTTP server with configured bind address and port
- [X] T228 [US3] Write integration test: start HTTP server on random port, send `GET /health`, verify 200 response with expected fields (`schema_status`, `current_schema_version`, `required_schema_version`) and `status: "ready"`
- [X] T229 [US3] Write integration test: start HTTP server with indexed fixture repo, send `tools/list` via POST, verify all tools listed
- [X] T230 [US3] Write integration test: send `locate_symbol` via HTTP POST, verify response matches stdio format
- [X] T231 [P] [US3] Write integration test: start HTTP server, index a project, call `/health` during indexing, verify `status: "indexing"`
- [X] T232 [P] [US3] Write test: attempt to start HTTP server on already-bound port, verify clear error message

**Checkpoint**: HTTP transport serves all MCP tools with identical responses to stdio

---

## Phase 5: Security Hardening & Cross-Cutting

**Purpose**: Path security tests, documentation, CLI help text

- [X] T233 [US1] Write security test suite for workspace path validation: test path traversal (`../../../etc/passwd`), symlink escape, relative path resolution, null bytes in path, extremely long paths, Unicode normalization edge cases
- [X] T234 [P] [US1] Write security test: verify `--auto-workspace` without `--allowed-root` fails at startup
- [X] T235 [P] [US3] Write security test: verify HTTP server defaults to `127.0.0.1` bind, not `0.0.0.0`
- [X] T236 [US1] Implement workspace eviction: when `max_auto_workspaces` limit is reached, evict the LRU auto-discovered workspace (by `last_used_at`), clean up its index data, log the eviction
- [X] T237 [P] Add `--help` text for new CLI flags: `--transport`, `--port`, `--bind`, `--auto-workspace`, `--allowed-root` with usage examples
- [X] T238a Update `tools/list` response to include `workspace` parameter in all tool schemas and add contract test verifying all query/path tools expose `workspace`
- [X] T238b Ensure HTTP error mapping uses canonical machine-readable `error.code` values from `specs/meta/protocol-error-codes.md` (including `invalid_input`, `workspace_not_registered`, `workspace_not_allowed`, `index_incompatible`)
- [X] T238c Normalize metadata enums (`indexing_status`, `result_completeness`) in all updated tool response structs
- [X] T239 [P] Write E2E test: start server with `--auto-workspace --allowed-root /tmp`, auto-discover 3 workspaces, query each, verify `known_workspaces` table has 3 entries with correct `last_used_at` updates
- [X] T454 [US1] Implement warmset prewarm selection in `crates/cruxe-cli/src/commands/serve_mcp.rs` and `crates/cruxe-state/src/workspace.rs`: load most-recently-used workspaces up to configurable bound (default: 3) and prewarm only those indices; implement `--no-prewarm` CLI flag to skip warmset prewarming entirely
- [X] T455 [US2] Implement interrupted job reconciliation in `crates/cruxe-state/src/jobs.rs`: mark leftover running jobs as `interrupted` on startup and surface `interrupted_recovery_report` via `crates/cruxe-mcp/src/tools/index_status.rs` and `crates/cruxe-mcp/src/tools/health_check.rs`
- [X] T456 [P] [US2] Add integration tests for restart recovery + warmset behavior: verify interrupted report visibility and warmset-first latency improvements on recent workspaces
- [X] T457 [P] Write performance benchmark: verify `/health` p95 < 50ms (SC-303), workspace routing overhead < 5ms per request, and warmset-enabled startup first-query p95 < 400ms (SC-307)

**Checkpoint**: Security hardened, CLI documented, E2E scenarios pass, performance verified

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Core Types)**: No dependencies beyond 003-structure-nav -- can start immediately
- **Phase 2 (Workspace Router)**: Depends on Phase 1 (types and state layer)
- **Phase 3 (Progress Notifications)**: Depends on Phase 1 (schema migration for progress_token); independent of Phase 2
- **Phase 4 (HTTP Transport)**: Depends on Phase 2 (workspace routing) for full functionality; `/health` can be built independently
- **Phase 5 (Security & Polish)**: Depends on Phases 2, 3, 4

### Parallel Opportunities

- Phase 1: T197/T198 (types), T201/T202/T203 (unit tests) can run in parallel
- Phase 3 and Phase 4 can proceed in parallel after Phase 1 is complete
- Phase 3: T220/T221 (tests) can run in parallel
- Phase 4: T231/T232 (tests) can run in parallel
- Phase 5: T234/T235/T237/T239 can run in parallel

### User Story Dependencies

- **US1 (P1)**: Phase 1 + Phase 2 -- foundational, no dependencies on US2/US3
- **US2 (P2)**: Phase 1 (schema) + Phase 3 -- independent of workspace routing
- **US3 (P2)**: Phase 1 + Phase 2 (workspace param) + Phase 4 -- builds on routing

## Implementation Strategy

### MVP First (US1)

1. Complete Phase 1: Core types and state
2. Complete Phase 2: Workspace router
3. **STOP and VALIDATE**: `workspace` parameter works on all tools, security constraints enforced

### Incremental Delivery

1. Phase 1 -> Types and state ready
2. Phase 2 -> Multi-workspace routing works (core value)
3. Phase 3 -> Progress notifications during indexing
4. Phase 4 -> HTTP transport with /health
5. Phase 5 -> Security hardening and polish

## Notes

- [P] tasks = different files, no dependencies
- [USn] label maps task to specific user story
- Commit after each task or logical group
- Stop at any checkpoint to validate independently
- Total: 49 tasks, 5 phases
