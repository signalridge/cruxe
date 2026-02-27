# Feature Specification: VCS Core (Branch Overlay Correctness)

**Feature Branch**: `005-vcs-core`
**Created**: 2026-02-23
**Status**: Complete (merge candidate)
**Version**: v0.9.0 (pre-GA core gate)
**Depends On**: 004-workspace-transport
**Input**: Constitution Principle III (Branch and Worktree Correctness), `specs/meta/design.md` Section 5.1 and Section 9.1-9.7

## Closure Update (2026-02-25)

- All scoped FRs are implemented and all scoped SCs are covered by passing tests.
- 005 is closed as a merge-candidate spec.
- This feature cycle did not create new `openspec/changes/*` artifacts.

## Overview

This spec isolates the **correctness-critical VCS foundation** before adding the full
GA tool surface. It delivers branch overlay indexing, incremental git-diff sync,
rebase/force-push recovery, query-time base+overlay merge, and safe MCP routing of
core search tools (`search_code`, `locate_symbol`).

The goal is to establish deterministic, ref-correct behavior as a standalone
milestone before layering advanced VCS analysis tools.

## Readiness Baseline Update (2026-02-25)

- Added `VcsAdapter` trait boundary in `cruxe-core::vcs_adapter` with a
  default Git-backed adapter implementation for FR-410 direction.
- Added shared `OverlayMergeKey` domain type to avoid ad-hoc tuple/string merge-key drift.

## User Scenarios & Testing

### User Story 1 - Git-Diff Incremental Sync (Priority: P1)

A developer pushes a commit that changes 10 files in a 5,000-file repository and
runs `sync_repo`. The system computes `git diff --name-status` against the
merge-base, identifies only the changed files, and updates the overlay index
incrementally. Renames are treated as delete-old + add-new in overlay and
tombstone logic. Each file update is atomic: no partial file state is ever
published. The developer sees sync complete in seconds, not minutes.

**Why this priority**: Incremental sync is the performance foundation for VCS
mode. Without it, every branch sync would require a full re-index, making
branch-scoped search impractical for real-world repositories.

**Independent Test**: Index a fixture repository on `main`, create a branch with
10 changed files, run `sync_repo`, and verify only those 10 files are processed
and the sync completes in under 5 seconds.

**Acceptance Scenarios**:

1. **Given** a repository indexed on `main` with a feature branch that modifies
   10 files, **When** `sync_repo` is run for the feature branch, **Then** the
   system computes `git diff --name-status` against the merge-base and processes
   only the 10 changed files, completing in under 5 seconds (SC-403).
2. **Given** a feature branch where `auth.rs` was renamed to `auth_handler.rs`,
   **When** `sync_repo` processes the rename, **Then** a tombstone is created for
   `auth.rs` and `auth_handler.rs` is added to the overlay index (FR-401).
3. **Given** a sync operation where file 7 of 10 fails to parse, **When** the
   error occurs, **Then** files 1-6 are published atomically, file 7 is skipped
   with an error logged, and files 8-10 continue processing. No partial file
   state is published (FR-402).
4. **Given** a sync already in progress for `(project, ref)`, **When** a second
   `sync_repo` is triggered for the same `(project, ref)`, **Then** the second
   request fails fast with an explicit `sync_in_progress` error (FR-409).
5. **Given** a failed sync operation, **When** the failure is detected, **Then**
   the previous published overlay state is preserved and the `index_jobs` entry
   is marked `failed` with a rollback record.

---

### User Story 2 - Branch Overlay Indexing (Priority: P1)

A developer works on two feature branches simultaneously. Each branch has its own
isolated overlay index directory containing only the files that differ from the
base (`main`). The base index remains immutable and shared. When the developer
searches on `feat/auth`, results reflect `feat/auth` changes; when they search on
`feat/payments`, results reflect `feat/payments` changes. There is zero cross-ref
leakage between branches.

**Why this priority**: Branch isolation is the correctness guarantee that makes
VCS-mode search trustworthy. Without per-ref overlay separation, search results
would conflate changes from unrelated branches, producing misleading results for
agents and developers alike.

**Independent Test**: Index the same repository on two branches where each has
a unique file. Search for those files on the opposite branch and verify they do
not appear. Then verify each file appears only on its own branch.

**Acceptance Scenarios**:

1. **Given** a repository with branches `feat/auth` and `feat/payments`, **When**
   both branches are synced, **Then** separate overlay index directories exist for
   each ref under the project's index root (FR-403).
2. **Given** `feat/auth` adds `src/auth_guard.rs`, **When** `search_code` is
   called with `ref: "feat/payments"`, **Then** `auth_guard.rs` does not appear in
   results (SC-400).
3. **Given** `feat/payments` modifies `src/billing.rs`, **When** `search_code` is
   called with `ref: "feat/auth"`, **Then** the original `main` version of
   `billing.rs` is returned, not the `feat/payments` version.
4. **Given** any overlay sync operation, **When** the base index for the default
   branch is inspected, **Then** it remains unmodified — the base index is
   immutable (FR-404).
5. **Given** a bootstrap sync for a branch with 50 changed files, **When**
   `sync_repo` creates the overlay, **Then** the overlay is fully populated in
   under 15 seconds (SC-404).

---

### User Story 3 - Rebase and Force-Push Recovery (Priority: P1)

A developer rebases their feature branch onto the latest `main` and force-pushes.
The commit ancestry between the previously indexed state and the new branch tip is
broken. On the next `sync_repo`, the system detects the ancestry break, discards
the stale overlay, recomputes the merge-base against the new history, and rebuilds
the overlay from scratch. The base index is never rebuilt — only the overlay is
regenerated. Post-recovery search results are correct and consistent.

**Why this priority**: Rebase and force-push are routine operations in
trunk-based and PR-driven workflows. If the system cannot recover from ancestry
breaks, search results become silently incorrect — the worst possible failure
mode for a code intelligence tool.

**Independent Test**: Index a branch, rebase it (breaking ancestry), run
`sync_repo`, and verify the overlay is rebuilt correctly without touching the
base index. Compare search results before and after to confirm correctness.

**Acceptance Scenarios**:

1. **Given** a feature branch that was rebased (ancestry broken from previously
   indexed commit), **When** `sync_repo` is run, **Then** the system detects the
   ancestry break via commit lineage check (FR-406).
2. **Given** a detected ancestry break, **When** recovery begins, **Then** the
   stale overlay is discarded, a new merge-base is computed, and the overlay is
   rebuilt from the new diff (FR-406).
3. **Given** a force-push recovery, **When** the overlay is rebuilt, **Then** the
   base index remains untouched — recovery does not trigger a base rebuild
   (SC-401).
4. **Given** a recovered overlay after rebase, **When** `search_code` is called
   for the rebased branch, **Then** results reflect the post-rebase file contents
   accurately.
5. **Given** a rebase that removes a file previously in the overlay, **When**
   recovery completes, **Then** a tombstone is created for the removed file and
   it no longer appears in search results.

---

### User Story 4 - Query-Time Base+Overlay Merge (Priority: P1)

An AI agent calls `search_code` for a feature branch. The system queries both the
base index (shared `main`) and the branch overlay index, then merges results using
a canonical merge key with overlay-wins precedence. Files deleted on the branch
are suppressed via tombstones so they never appear in results. The agent receives
a unified, ref-correct result set as if the branch had its own full index.

**Why this priority**: The overlay architecture only delivers value if query-time
merge is correct. This is the mechanism that makes the per-ref illusion work —
without it, users would see duplicate or contradictory results from base and
overlay.

**Independent Test**: Create a branch that modifies `src/handler.rs` and deletes
`src/legacy.rs`. Search for symbols in both files. Verify `handler.rs` returns
the branch version (not base), and `legacy.rs` returns no results.

**Acceptance Scenarios**:

1. **Given** a file `src/handler.rs` that exists in both base and overlay,
   **When** `search_code` returns results, **Then** the overlay version wins and
   the base version is suppressed via merge key deduplication (FR-407).
2. **Given** a file `src/legacy.rs` deleted on the feature branch, **When**
   `search_code` is called for that branch, **Then** tombstone suppression
   prevents the base version from appearing in results (FR-405, SC-402).
3. **Given** a file `src/utils.rs` that exists only in base (unchanged on
   branch), **When** `search_code` matches it, **Then** the base version is
   returned normally since no overlay entry or tombstone exists.
4. **Given** a query that matches 3 base results and 2 overlay results with 1
   merge key collision, **When** results are merged, **Then** exactly 4 results
   are returned (2 base-only + 1 overlay-wins + 1 overlay-only).
5. **Given** a branch with a tombstone for `src/old.rs`, **When** `locate_symbol`
   is called for a symbol that only existed in `src/old.rs`, **Then** no results
   are returned for that ref.

---

### User Story 5 - VCS Adapter Abstraction (Priority: P2)

A maintainer needs to extend Cruxe with support for a non-Git VCS (or a
different Git backend). The system provides a `VcsAdapter` trait that isolates all
VCS operations (diff computation, merge-base detection, ancestry checks, ref
listing) behind a clean boundary. The Git2 implementation is the default. Index
and query logic never call VCS operations directly — they go through the adapter.

**Why this priority**: Decoupling VCS operations from index/query logic ensures
testability (adapters can be mocked), maintainability (Git internals are
contained), and future extensibility (alternative VCS backends or testing
strategies).

**Independent Test**: Run the full overlay sync and query test suite using a mock
`VcsAdapter` that returns canned diff and merge-base results. Verify all tests
pass without touching a real Git repository.

**Acceptance Scenarios**:

1. **Given** the `VcsAdapter` trait definition, **When** its interface is
   inspected, **Then** it exposes methods for diff computation, merge-base
   detection, ancestry validation, ref listing, and worktree management (FR-410).
2. **Given** the default `Git2VcsAdapter` implementation, **When** `diff_files`
   is called for a branch, **Then** it returns the same results as
   `git diff --name-status` against the merge-base.
3. **Given** a mock `VcsAdapter` returning a canned diff of 5 modified files,
   **When** `sync_repo` is run against the mock, **Then** overlay indexing
   processes exactly those 5 files without any Git dependency.
4. **Given** the index and query modules, **When** their source is inspected,
   **Then** no direct calls to `git2` or VCS shell commands exist — all VCS
   access goes through `VcsAdapter`.

---

### User Story 6 - Worktree Manager (Priority: P2)

A developer has two terminal sessions working on different branches. Each session
needs a checked-out worktree to access branch files for indexing. The system
manages Git worktrees through a lease-based model with persisted refcount state in
SQLite. When a sync operation needs a worktree, it acquires a lease; when done, it
releases it. Concurrent access to the same worktree is prevented. Orphaned
worktrees from crashed processes are detected and cleaned up.

**Why this priority**: Branch overlay indexing requires file-system access to
branch contents. Worktree management ensures concurrent branch operations are safe
and deterministic, preventing resource leaks and data races.

**Independent Test**: Acquire a worktree lease for `feat/auth`, verify the
worktree exists on disk, attempt to acquire a second lease for the same ref and
verify it fails, release the first lease, and verify re-acquisition succeeds.

**Acceptance Scenarios**:

1. **Given** a sync operation for `feat/auth`, **When** a worktree lease is
   requested, **Then** a Git worktree is created (or reused) and a
   `WorktreeLease` record is persisted in SQLite with refcount = 1 (FR-411).
2. **Given** an active worktree lease for `feat/auth`, **When** a second lease
   request arrives for the same ref, **Then** the request fails with an explicit
   `worktree_in_use` status.
3. **Given** a completed sync operation, **When** the worktree lease is released,
   **Then** the refcount is decremented and, if zero, the worktree is eligible
   for cleanup.
4. **Given** a process crash during sync, **When** the system restarts and
   detects an orphaned lease (stale PID), **Then** the orphaned lease is cleaned
   up and the worktree is released for reuse.
5. **Given** a branch name with filesystem-unsafe characters (e.g.,
   `feat/auth#2`), **When** a worktree is created, **Then** the worktree
   directory name is deterministically normalized to a safe path.

---

### User Story 7 - Core MCP Routing and Metadata (Priority: P2)

An AI agent calls `search_code` or `locate_symbol` via MCP on a VCS-mode project
with a specific `ref`. The system routes the query through the merged VCS query
path (base+overlay), and every result includes a `source_layer` field indicating
whether the result came from `"base"` or `"overlay"`. The agent uses this metadata
to understand whether a result reflects the shared mainline or branch-specific
changes.

**Why this priority**: MCP is the primary interface for AI agents. Ensuring that
the core search tools are VCS-aware and include provenance metadata enables agents
to make informed decisions about code context without additional queries.

**Independent Test**: Start the MCP server with a VCS-mode project, sync a
feature branch, call `search_code` and `locate_symbol` via MCP, and verify every
result includes `source_layer` metadata with correct values.

**Acceptance Scenarios**:

1. **Given** a VCS-mode project with a synced feature branch, **When**
   `search_code` is called with `ref: "feat/auth"`, **Then** results from the
   base index include `source_layer: "base"` and results from the overlay include
   `source_layer: "overlay"` (FR-408, SC-405).
2. **Given** a `locate_symbol` call for a symbol that exists in both base and
   overlay, **When** results are returned, **Then** only the overlay version is
   returned (overlay-wins) with `source_layer: "overlay"`.
3. **Given** a `locate_symbol` call for a symbol that exists only in base,
   **When** results are returned, **Then** the result includes
   `source_layer: "base"`.
4. **Given** a VCS-mode project, **When** `tools/list` is called, **Then**
   `search_code` and `locate_symbol` schemas remain unchanged — VCS routing is
   transparent to the agent (FR-412).
5. **Given** a single-version mode project (no Git), **When** `search_code` is
   called, **Then** results do not include `source_layer` metadata and the query
   path bypasses overlay merge entirely.

### Edge Cases

- Deleted files in feature branches must never be returned from base for that ref.
- Failed sync must preserve the previous published overlay state.
- Concurrent sync on same `(project, ref)` must fail fast with explicit status.
- Branch names with filesystem-unsafe characters must be normalized deterministically.

## Requirements

### Functional Requirements

- **FR-400**: System MUST detect changed files via git diff name-status against merge-base.
- **FR-401**: System MUST treat rename as delete-old + add-new in overlay/tombstone logic.
- **FR-402**: System MUST perform per-file atomic updates; no partial file state may publish.
- **FR-403**: System MUST maintain separate overlay index directories per ref.
- **FR-404**: System MUST preserve an immutable base index for default branch operations.
- **FR-405**: System MUST track per-ref tombstones and suppress matching base paths at query time.
- **FR-406**: System MUST detect ancestry breaks and rebuild overlay from new merge-base.
- **FR-407**: System MUST merge base+overlay results by canonical merge key with overlay-wins precedence.
- **FR-408**: System MUST add `source_layer` (`base` | `overlay`) to VCS-mode search/locate results.
- **FR-409**: System MUST enforce single-active-sync-per-ref through `index_jobs` coordination.
- **FR-410**: System MUST provide `VcsAdapter` and a Git2 implementation.
- **FR-411**: System MUST manage worktree leases with persisted refcount state.
- **FR-412**: System MUST keep core MCP routing (`search_code`, `locate_symbol`) on merged VCS query path.

### Key Entities

- **OverlayIndex**: ref-scoped Tantivy index set for branch-specific deltas.
- **TombstoneEntry**: path suppression record preventing base leakage for a ref.
- **MergeKey**: dedup key for base/overlay collision resolution.
- **WorktreeLease**: persistent refcount lease for safe worktree lifecycle.

## Success Criteria

### Measurable Outcomes

- **SC-400**: Same query on two refs returns ref-consistent results with zero cross-ref leakage.
- **SC-401**: Rebase/force-push recovery rebuilds overlay correctness without base rebuild.
- **SC-402**: Deleted files in feature branch are never returned for that ref.
- **SC-403**: Incremental sync (10 changed files) completes in under 5 seconds.
- **SC-404**: Overlay bootstrap (50 changed files) completes in under 15 seconds.
- **SC-405**: VCS-mode `search_code`/`locate_symbol` always return `source_layer` metadata.
