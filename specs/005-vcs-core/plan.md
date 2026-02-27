# Implementation Plan: VCS Core

**Branch**: `005-vcs-core` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/005-vcs-core/spec.md`
**Depends On**: 004-workspace-transport
**Version**: v0.9.0

## Summary

Implement the correctness-critical VCS foundation: incremental git-diff sync,
branch overlay index lifecycle, ancestry-break recovery, query-time base+overlay
merge, and VCS-mode core MCP routing for `search_code` / `locate_symbol`.

This spec intentionally excludes advanced VCS analysis tooling and state
portability commands; those are moved to `006-vcs-ga-tooling`.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: git2, tantivy, rusqlite, tokio, blake3, tree-sitter, serde
**Storage**: Tantivy (base + overlay) + SQLite (`branch_state`, `branch_tombstones`, `index_jobs`, `worktree_leases`)
**Testing**: cargo test + multi-branch fixture repos
**Constraints**: zero external service dependencies; overlay writes MUST NOT modify base index

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Overlay merge preserves concrete `file:line` location answers |
| II. Single Binary Distribution | PASS | No new service dependency introduced |
| III. Branch/Worktree Correctness | **PRIMARY** | This spec implements the correctness core before GA tooling |
| IV. Incremental by Design | PASS | Git-diff + per-file atomic updates + idempotent sync |
| V. Agent-Aware Response Design | PASS | `source_layer` metadata added to VCS-mode core queries |
| VI. Fail-Soft Operation | PASS | Staging + rollback preserves previous published state |
| VII. Explainable Ranking | PASS | Core merge path remains deterministic for downstream explainability |

## Project Structure

### Documentation (this feature)

```text
specs/005-vcs-core/
  plan.md
  spec.md
  data-model.md
  tasks.md
  contracts/
    mcp-tools.md
```

### Source Code Focus

```text
crates/
  cruxe-vcs/             # VcsAdapter + Git2 + worktree leases
  cruxe-indexer/         # overlay lifecycle, staging, sync_incremental
  cruxe-query/           # tombstones + overlay_merge + VCS routing
  cruxe-state/           # branch/tombstone/lease state tables
  cruxe-mcp/             # core routing updates for search/locate
```

## Pre-Analysis

- **Similar patterns**: ref-scoped query in 001, per-file updates in index writer, SQLite job-state lifecycle.
- **Dependencies**: git2 adapter, overlay manager, tombstone query path, MCP routing hooks.
- **Conventions**: per-crate typed errors, structured tracing spans, SQLite WAL mode.
- **Risk areas**: concurrent sync conflict, staging atomicity, stale overlay publication.

## Complexity Tracking

### Justified Complexity

1. **Two-phase staging** for crash-safe publish.
2. **Tombstone suppression** to prevent branch result leakage.
3. **Adapter boundary** (`VcsAdapter`) to isolate VCS operations from query/index concerns.

### Avoided Complexity

- No cross-VCS backend support beyond Git2 in this milestone.
- No automatic overlay eviction; handled later in tooling/maintenance track.
- No semantic retrieval changes in this VCS core milestone.
