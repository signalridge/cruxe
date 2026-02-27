# Tasks: VCS GA Tooling

**Input**: Design documents from `/specs/006-vcs-ga-tooling/`
**Prerequisites**: plan.md (required), spec.md (required), contracts/mcp-tools.md
**Depends On**: 005-vcs-core

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US5)
- Include exact file paths in descriptions


## Phase 1: VCS Analysis Tools (`diff_context`, `find_references`, `explain_ranking`)

- [x] T296 [US1] Implement `diff_context` computation in `crates/cruxe-query/src/diff_context.rs`: compute merge-base, get changed files, compare symbol records between base and overlay using `symbol_stable_id`, classify as added/modified/deleted, include before/after signatures for modified
- [x] T297 [US1] Implement `diff_context` MCP tool handler in `crates/cruxe-mcp/src/tools/diff_context.rs`: parse input, delegate to query engine, format response per contract including stable handles (`symbol_id`, `symbol_stable_id`) and compatibility metadata
- [x] T298 [US2] Implement `find_references` in `crates/cruxe-query/src/find_references.rs`: query `symbol_edges` table for `from_symbol_id` or `to_symbol_id` matching the given symbol, join with `symbol_relations` for location data, filter by optional `kind` and `ref`
- [x] T299 [US2] Implement `find_references` MCP tool handler in `crates/cruxe-mcp/src/tools/find_references.rs`: parse input, delegate to query engine, format response per contract including stable handles for target/source symbols
- [x] T300 [US3] Implement `explain_ranking` in `crates/cruxe-query/src/explain_ranking.rs`: re-execute query, find the specific result, break down scoring into components (bm25, exact_match, qualified_name, path_affinity, definition_boost, kind_match, total)
- [x] T301 [US3] Implement `explain_ranking` MCP tool handler in `crates/cruxe-mcp/src/tools/explain_ranking.rs`: parse input, delegate to query engine, format response per contract and canonical error envelope
- [x] T302 [US1] Write integration test: create branch with added/modified/deleted symbols, call `diff_context`, verify all three change types are correctly classified
- [x] T303 [P] [US1] Write integration test: verify `path_filter` on `diff_context` limits results to matching paths
- [x] T304 [P] [US2] Write integration test: index repo with symbol_edges populated, call `find_references`, verify all edges returned with correct metadata
- [x] T305 [P] [US3] Write integration test: run search query, call `explain_ranking` on a result, verify scoring breakdown sums to total
- [x] T306 [US3] Write unit test: verify `explain_ranking` is deterministic — same query + same index state = same breakdown

## Phase 2: Ref Operations Tools (`list_refs`, `switch_ref`)

- [x] T307 [US4] Implement `list_refs` MCP tool handler in `crates/cruxe-mcp/src/tools/list_refs.rs`: query `branch_state` table for all refs for the project, return with metadata
- [x] T308 [US4] Implement `switch_ref` MCP tool handler in `crates/cruxe-mcp/src/tools/switch_ref.rs`: validate ref exists in `branch_state`, update session default ref, verify worktree availability
- [x] T309 [US4] Write integration test: index `main` and `feat/auth`, call `list_refs`, verify both returned with correct metadata
- [x] T310 [P] [US4] Write integration test: call `switch_ref` with valid ref, verify subsequent queries default to that ref
- [x] T311 [P] [US4] Write integration test: call `switch_ref` with unindexed ref, verify error response with guidance

## Phase 3: State Portability (`state export/import`)

- [x] T312 [US5] Implement state export in `crates/cruxe-state/src/export.rs`: gather SQLite DB + Tantivy indices, create `metadata.json`, compress to `.tar.zst` archive
- [x] T313 [US5] Implement state import in `crates/cruxe-state/src/import.rs`: validate format/schema version, extract to data directory, update `projects` table with local repo_root, mark branches as stale
- [x] T314 [US5] Implement `state export` CLI command in `crates/cruxe-cli/src/commands/state_export.rs`
- [x] T315 [US5] Implement `state import` CLI command in `crates/cruxe-cli/src/commands/state_import.rs`
- [x] T316 [US5] Write integration test: full export + import roundtrip, verify search returns same results after import
- [x] T317 [P] [US5] Write integration test: import stale state, run sync, verify only delta is re-indexed

## Phase 4: Tool Registry and Discovery

- [x] T318 Register all 5 new tools in `crates/cruxe-mcp/src/server.rs`: `diff_context`, `find_references`, `explain_ranking`, `list_refs`, `switch_ref` in `tools/list` response
- [x] T319 Write integration test: start MCP server, call `tools/list`, verify all new tools are listed with correct schemas, and validate new tool errors map to `specs/meta/protocol-error-codes.md` with stable `error.code` across stdio and HTTP transports

## Phase 5: Overlay Maintenance CLI

- [x] T320 Implement `prune-overlays` CLI command in `crates/cruxe-cli/src/commands/prune_overlays.rs`: list overlays by `last_accessed_at`, delete overlays older than TTL, update `branch_state` to `evicted`
- [x] T321 [P] Write integration test: create overlays, mark as stale, run prune, verify overlays are removed

## Phase 6: Tooling Polish and Verification

- [x] T322 Run full test suite (`cargo test --workspace`) and fix any failures
- [x] T323 Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [x] T324 Run `cargo fmt --check --all` and fix formatting

## Phase 7: Descriptor Stability and Test Throughput

- [x] T325 [US5] Implement bounded runtime SQLite connection cache in `crates/cruxe-mcp/src/server.rs` with configurable cap (`CRUXE_MAX_OPEN_CONNECTIONS`) and idle LRU eviction to prevent unbounded FD growth
- [x] T326 [US5] Add connection manager regression tests in `crates/cruxe-mcp/src/server/tests.rs` to validate idle eviction behavior and safe handling when all cached entries are in use
- [x] T327 [US5] Replace fully serial fixture-index gating with configurable bounded parallelism in `crates/cruxe-mcp/src/server/tests.rs` (`CRUXE_TEST_FIXTURE_PARALLELISM`) to preserve stability without forcing `RUST_TEST_THREADS=1`
- [x] T328 [US5] Add project-scoped cross-process maintenance lock helper in `crates/cruxe-state/src/maintenance_lock.rs` with a parent-scoped stable lock path (`locks/state-maintenance-<path-hash>.lock`) safe across import rename-swap
- [x] T329 [US5] Apply maintenance lock to `state import` command lifecycle in `crates/cruxe-cli/src/commands/state_import.rs`, plus `prune-overlays` in `crates/cruxe-cli/src/commands/prune_overlays.rs`, and overlay publish path in `crates/cruxe-indexer/src/sync_incremental.rs`
- [x] T330 [US5] Map maintenance-lock contention to retryable MCP sync semantics in `crates/cruxe-mcp/src/server/tool_calls/shared.rs` and add lock helper unit tests
- [x] T331 [P] [US5] Add cross-process lock-contention integration coverage in `crates/cruxe-cli/tests/integration_test.rs` to verify `state import` fails fast while a maintenance lock is held

## Dependencies & Execution Order

### Phase Dependencies

- Phase 1 depends on 005 core merge correctness.
- Phase 2 and Phase 3 can start after Phase 1 baseline interfaces exist.
- Phase 4 depends on Phases 1-3 (register all tools once handlers exist).
- Phase 5 can run in parallel with Phase 2/3.
- Phase 6 validates end-to-end tooling readiness for v1.0.0 GA.

### Critical Path

Phase 1 -> Phase 2 -> Phase 4 -> Phase 6

## Implementation Strategy

1. Deliver analysis tools (`diff_context`, `find_references`, `explain_ranking`).
2. Add ref operations and state portability commands.
3. Finalize registration, maintenance command, and tooling polish.

## Notes

- This spec completes the VCS GA surface together with `005-vcs-core`.
- Keep failures fail-soft: tool-specific errors must not break base search flows.
