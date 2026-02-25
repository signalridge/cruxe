# Tasks: VCS Core

**Input**: Design documents from `/specs/005-vcs-core/`
**Prerequisites**: plan.md (required), spec.md (required), data-model.md, contracts/mcp-tools.md
**Depends On**: 004-workspace-transport

> Status note (2026-02-25): All listed tasks are complete and 005 is closed as a
> merge-candidate spec.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US7)
- Include exact file paths in descriptions


## Phase 1: VCS Adapter and Infrastructure

- [x] T240 Create `crates/codecompass-vcs/Cargo.toml` with dependencies: `git2`, `blake3`, `thiserror`, `tracing`, `serde`
- [x] T241 [US5] Define `VcsAdapter` trait in `crates/codecompass-vcs/src/adapter.rs`: `DetectRepo`, `ResolveHEAD`, `ListRefs`, `MergeBase`, `DiffNameStatus`, `IsAncestor`, `EnsureWorktree` with associated types for `FileChange` and `DiffEntry`
- [x] T242 [US5] Implement `DiffEntry` and `FileChangeKind` types in `crates/codecompass-vcs/src/diff.rs`: `Added`, `Modified`, `Deleted`, `Renamed { old_path, new_path }`
- [x] T243 [US5] Implement Git2 adapter in `crates/codecompass-vcs/src/git2_adapter.rs`: all 7 trait methods using the `git2` crate
- [x] T244 [P] [US5] Write unit tests for Git2 adapter: test each trait method against a temp Git repo created programmatically
- [x] T245 [US6] Implement worktree manager in `crates/codecompass-vcs/src/worktree.rs`: `ensure_worktree(ref)`, `release_lease(ref)`, `cleanup_stale()`, refcount increment/decrement
- [x] T246 [P] [US6] Write unit tests for worktree manager: lease acquisition, release, refcount enforcement, stale detection
- [x] T247 Add `worktree_leases` table to `crates/codecompass-state/src/schema.rs`
- [x] T248 [US6] Implement `worktree_leases` CRUD in `crates/codecompass-state/src/worktree_leases.rs`: create, get, update_refcount, update_status, list_stale
- [x] T249 Extend `branch_state` table in `crates/codecompass-state/src/schema.rs`: add `symbol_count`, `is_default_branch`, `status`, `eviction_eligible_at` columns
- [x] T250 [P] Extend `branch_tombstones` table in `crates/codecompass-state/src/schema.rs`: add `tombstone_type`, `created_at` columns
- [x] T251 Implement `branch_tombstones` CRUD in `crates/codecompass-state/src/tombstones.rs`: create, delete_for_ref, list_paths_for_ref, bulk_upsert
- [x] T252 Update `crates/codecompass-state/src/branch_state.rs`: support new `branch_state` fields, add `set_status`, `get_by_status`, `mark_eviction_eligible` operations
- [x] T253 [P] Create `crates/codecompass-vcs/src/lib.rs` to re-export all public types and traits
- [x] T254 Add `codecompass-vcs` to workspace `Cargo.toml` as member crate

## Phase 2: Overlay Index Infrastructure

- [x] T255 [US2] Implement overlay directory manager in `crates/codecompass-indexer/src/overlay.rs`: create overlay dir, normalize branch names for filesystem, list active overlays, delete overlay dir
- [x] T256 [US1] Implement two-phase staging in `crates/codecompass-indexer/src/staging.rs`: create staging dir with `sync_id`, write to staging, atomic commit (rename), rollback (delete staging), cleanup stale staging dirs
- [x] T257 [US2] Implement overlay Tantivy index creation in `crates/codecompass-indexer/src/overlay.rs`: create `symbols`, `snippets`, `files` indices in overlay directory with same schemas as base
- [x] T258 [US1] Update `crates/codecompass-indexer/src/writer.rs`: add `WriteTarget` enum (`Base`, `Overlay { branch }`, `Staging { sync_id }`), route writes to correct directory
- [x] T259 [P] [US1] Write unit tests for two-phase staging: successful commit, rollback on failure, cleanup of stale staging
- [x] T260 [P] [US2] Write unit tests for overlay directory manager: create, list, delete, branch name normalization

## Phase 3: Git-Diff Incremental Sync

- [x] T261 [US1] Implement git-diff sync orchestrator in `crates/codecompass-indexer/src/sync_incremental.rs`: compute merge-base, run `DiffNameStatus`, classify file changes (A/M/D/R), dispatch per-file processing
- [x] T262 [US1] Implement per-file atomic overlay write in `crates/codecompass-indexer/src/sync_incremental.rs`: for each changed file, parse with tree-sitter, extract symbols/snippets/file record, write ALL records for that file atomically to staging
- [x] T263 [US1] Implement rename handling in `crates/codecompass-indexer/src/sync_incremental.rs`: renamed files processed as delete old path (tombstone) + add new path (new records)
- [x] T264 [US1] Implement tombstone updates in `crates/codecompass-indexer/src/sync_incremental.rs`: after successful sync, bulk update `branch_tombstones` with deleted and replaced paths
- [x] T265 [US3] Implement ancestry check in `crates/codecompass-indexer/src/sync_incremental.rs`: before incremental sync, call `is_ancestor(last_indexed_commit, HEAD)`. If false, trigger overlay rebuild
- [x] T266 [US3] Implement overlay rebuild in `crates/codecompass-indexer/src/sync_incremental.rs`: delete existing overlay, recompute merge-base, diff from new merge-base, build full overlay from scratch
- [x] T267 [US1] Update `branch_state` after successful sync: set `last_indexed_commit`, `merge_base_commit`, `file_count`, `symbol_count`
- [x] T268 [US1] Update `index_jobs` during sync lifecycle: create job with `sync_id` on start, update to `published` on commit, update to `rolled_back` on failure
- [x] T269 [P] Create multi-branch fixture repo in `testdata/fixtures/vcs-sample/setup.sh`: script that creates a Git repo with `main` branch (base), `feat/add-file` (adds a new file), `feat/modify-sig` (modifies a function signature), `feat/delete-file` (deletes a file), `feat/rename-file` (renames a file), `feat/rebase-target` (for rebase testing)
- [x] T270 [US1] Write integration test: index base (`main`), create overlay for `feat/add-file`, verify only the new file is in overlay, verify base is unchanged
- [x] T271 [P] [US1] Write integration test: index overlay for `feat/delete-file`, verify tombstone exists for deleted file, verify base still has the file
- [x] T272 [P] [US1] Write integration test: index overlay for `feat/rename-file`, verify old path is tombstoned and new path is indexed
- [x] T273 [US3] Write integration test: index overlay, simulate rebase (rewrite history), run sync, verify ancestry break triggers overlay rebuild
- [x] T274 [US1] Write integration test: verify incremental sync (10 files) completes in < 5 seconds on fixture repo

## Phase 4: Query-Time Base+Overlay Merge

- [x] T275 [US4] Implement tombstone loader in `crates/codecompass-query/src/tombstone.rs`: load tombstone set from SQLite for a given `(repo, ref)`, cache in memory for query duration
- [x] T276 [US4] Implement overlay merge in `crates/codecompass-query/src/overlay_merge.rs`: `merged_search(query, ref)` function per algorithm in data-model.md — parallel search, tag source_layer, suppress tombstoned paths, merge by key (overlay wins), sort by score
- [x] T277 [US4] Implement merge key extraction in `crates/codecompass-query/src/overlay_merge.rs`: extract merge keys for symbols, snippets, and files per 001-core-mvp data-model.md
- [x] T278 [US4] Modify `crates/codecompass-query/src/search.rs`: detect VCS mode, route through `merged_search` for VCS queries, pass through directly for single-version mode
- [x] T279 [US4] Modify `crates/codecompass-query/src/locate.rs`: same VCS routing as search
- [x] T280 [US4] Add `source_layer` field to search/locate result types in `crates/codecompass-core/src/types.rs`
- [x] T281 [US4] Write E2E test: modify function signature on feature branch, query on both refs, verify old signature on `main` and new signature on feature branch (SC-400)
- [x] T282 [P] [US4] Write E2E test: delete file on feature branch, query on feature branch, verify deleted file not returned; query on `main`, verify file IS returned (SC-402)
- [x] T283 [P] [US4] Write E2E test: verify `source_layer` field is correctly set to `"base"` or `"overlay"` in results
- [x] T284 [US4] Write E2E test: verify dedup by merge key — overlay result replaces base result on collision

## Phase 5: Core MCP Routing and Sync Safety

- [x] T285 [US7] Modify `crates/codecompass-mcp/src/tools/search_code.rs`: route through overlay merge for VCS-mode queries, include `source_layer` in results and stable handles (`symbol_id`, `symbol_stable_id`) when available
- [x] T286 [US7] Modify `crates/codecompass-mcp/src/tools/locate_symbol.rs`: same overlay routing, include stable handles (`symbol_id`, `symbol_stable_id`)
- [x] T287 [US7] Update `crates/codecompass-mcp/src/protocol.rs`: add `source_layer` and `schema_status` to Protocol v1 metadata for VCS-mode responses
- [x] T288 [US7] Write integration tests: call `search_code` via MCP with `ref` parameter, verify overlay merge results include `source_layer`, and verify VCS-mode search/locate responses include stable handles (`symbol_id`, `symbol_stable_id`) plus compatibility metadata (`schema_status`)
- [x] T289 [US1] Implement concurrent sync prevention in `crates/codecompass-indexer/src/sync_incremental.rs`: check `index_jobs` for active job on same `(project_id, ref)` before starting sync, reject with `sync_in_progress` error

## Phase 6: Core GA Validation

- [x] T290 [US4] Write GA acceptance E2E test: same query on two different refs returns ref-consistent results (SC-400)
- [x] T291 [US6] [P] Write GA acceptance E2E test: switching worktree does not reuse stale overlay from previous ref (SC-401)
- [x] T292 [US4] [P] Write GA acceptance E2E test: deleted file in feature branch is not returned from base for that ref (SC-402)
- [x] T293 [US3] Write GA acceptance E2E test: rebase after indexing produces correct refreshed results without full base rebuild (SC-403)
- [x] T294 [US1] Performance benchmark: incremental sync (10 files) < 5s, overlay bootstrap (50 files) < 15s
- [x] T295 [US4] Verify branch result correctness: 100% on all fixture scenarios

## Dependencies & Execution Order

### Phase Dependencies

- Phase 1 -> Phase 2 -> Phase 3 -> Phase 4 is the core correctness chain.
- Phase 5 depends on Phase 4 (routing through merged query path).
- Phase 6 depends on Phases 3-5 and is the v0.9.0 core gate.

### Critical Path

Phase 1 -> Phase 2 -> Phase 3 -> Phase 4 -> Phase 5 -> Phase 6

## Implementation Strategy

1. Finish overlay correctness first (Phases 1-4).
2. Wire MCP core routing and job-safety second (Phase 5).
3. Run core GA acceptance suite before enabling advanced VCS tooling (Phase 6).

## Notes

- This spec intentionally excludes advanced VCS tools and portability commands.
- Those capabilities are defined in `006-vcs-ga-tooling`.
