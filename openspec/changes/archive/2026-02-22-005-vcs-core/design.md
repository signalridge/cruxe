## Context

Implement the correctness-critical VCS foundation: incremental git-diff sync, branch overlay index lifecycle, ancestry-break recovery, query-time base+overlay merge, and VCS-mode core MCP routing for `search_code` / `locate_symbol`. This spec intentionally excludes advanced VCS analysis tooling and state portability commands; those are moved to `006-vcs-ga-tooling`.

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: git2, tantivy, rusqlite, tokio, blake3, tree-sitter, serde
**Storage**: Tantivy (base + overlay) + SQLite (`branch_state`, `branch_tombstones`, `index_jobs`, `worktree_leases`)
**Testing**: cargo test + multi-branch fixture repos
**Constraints**: zero external service dependencies; overlay writes MUST NOT modify base index

### Pre-Analysis

- **Similar patterns**: ref-scoped query in 001, per-file updates in index writer, SQLite job-state lifecycle.
- **Dependencies**: git2 adapter, overlay manager, tombstone query path, MCP routing hooks.
- **Conventions**: per-crate typed errors, structured tracing spans, SQLite WAL mode.
- **Risk areas**: concurrent sync conflict, staging atomicity, stale overlay publication.

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Overlay merge preserves concrete `file:line` location answers |
| II. Single Binary Distribution | PASS | No new service dependency introduced |
| III. Branch/Worktree Correctness | **PRIMARY** | This spec implements the correctness core before GA tooling |
| IV. Incremental by Design | PASS | Git-diff + per-file atomic updates + idempotent sync |
| V. Agent-Aware Response Design | PASS | `source_layer` metadata added to VCS-mode core queries |
| VI. Fail-Soft Operation | PASS | Staging + rollback preserves previous published state |
| VII. Explainable Ranking | PASS | Core merge path remains deterministic for downstream explainability |

## Goals / Non-Goals

**Goals:**
1. Deliver incremental git-diff sync with per-file atomic updates and rename handling.
2. Implement branch overlay index lifecycle with immutable shared base.
3. Implement ancestry-break detection and overlay rebuild for rebase/force-push recovery.
4. Implement query-time base+overlay merge with tombstone suppression and overlay-wins precedence.
5. Wire VCS-mode core MCP routing for `search_code` / `locate_symbol` with `source_layer` metadata.
6. Provide `VcsAdapter` trait boundary with Git2 default implementation.
7. Implement worktree lease manager with persisted refcount state.

**Non-Goals:**
1. No cross-VCS backend support beyond Git2 in this milestone.
2. No automatic overlay eviction; handled later in tooling/maintenance track.
3. No semantic retrieval changes in this VCS core milestone.
4. No advanced VCS analysis tools or state portability commands (deferred to `006-vcs-ga-tooling`).

## Decisions

### D1. Two-phase staging for crash-safe publish

Overlay writes go through a staging directory with a unique `sync_id`. On success, staging is atomically committed (renamed) to the overlay directory. On failure, staging is deleted and the previous published state is preserved.

**Why:** crash-safe publish prevents partial overlay state from being visible to queries. Rollback is a simple directory delete.

### D2. Tombstone suppression for branch result isolation

Per-ref tombstones are stored in SQLite (`branch_tombstones`) and loaded at query time. Base results matching tombstoned paths are suppressed before merge, ensuring deleted files on a branch never leak into results.

**Why:** without tombstone suppression, deleted files would appear in branch search results from the immutable base index, violating ref-correctness.

### D3. VcsAdapter trait boundary

All VCS operations (diff computation, merge-base detection, ancestry checks, ref listing, worktree management) are isolated behind a `VcsAdapter` trait. Index and query logic never call VCS operations directly.

**Why:** isolates Git internals for testability (adapters can be mocked), maintainability (Git2 details contained), and future extensibility (alternative VCS backends).

### D4. Overlay merge with canonical merge key

Query-time merge searches base and overlay in parallel, tags results with `source_layer`, suppresses tombstoned paths, deduplicates by canonical merge key with overlay-wins precedence, and sorts by score.

**Why:** the overlay architecture only delivers value if query-time merge is correct. Canonical merge keys ensure deterministic dedup across index types (symbols, snippets, files).

### D5. Single-active-sync-per-ref enforcement

The `index_jobs` table tracks active sync operations per `(project_id, ref)`. Before starting a sync, the system checks for an existing active job and rejects with `sync_in_progress` if one exists.

**Why:** concurrent sync on the same ref would produce race conditions in overlay writes and tombstone state.

### D6. Project structure

```text
crates/
  cruxe-vcs/             # VcsAdapter + Git2 + worktree leases
  cruxe-indexer/         # overlay lifecycle, staging, sync_incremental
  cruxe-query/           # tombstones + overlay_merge + VCS routing
  cruxe-state/           # branch/tombstone/lease state tables
  cruxe-mcp/             # core routing updates for search/locate
```

## Risks / Trade-offs

- **[Risk] Concurrent sync conflict on same (project, ref)** → **Mitigation:** single-active-sync-per-ref enforcement via `index_jobs` table check before sync start.
- **[Risk] Staging atomicity failure (crash mid-commit)** → **Mitigation:** two-phase staging with atomic directory rename; rollback is directory delete preserving previous published state.
- **[Risk] Stale overlay publication after rebase/force-push** → **Mitigation:** ancestry check before incremental sync; ancestry break triggers full overlay rebuild from new merge-base.
- **[Risk] Base index corruption from overlay writes** → **Mitigation:** overlay writes are strictly directory-isolated; base index is immutable by design.

## Migration Plan

1. Finish overlay correctness first (VCS adapter, overlay infrastructure, incremental sync, query-time merge).
2. Wire MCP core routing and job-safety second.
3. Run core GA acceptance suite before enabling advanced VCS tooling.

Rollback: overlay directories can be deleted and rebuilt from scratch; base index is never modified by overlay operations.

## Resolved Questions

1. Cross-VCS backend support is deferred; only Git2 is implemented in this milestone.
2. Automatic overlay eviction is deferred to the tooling/maintenance track.
3. Advanced VCS analysis tools and state portability commands are defined in `006-vcs-ga-tooling`.
