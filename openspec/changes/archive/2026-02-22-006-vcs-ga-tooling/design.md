## Context

Implement the advanced VCS GA surface on top of core overlay correctness: `diff_context`, `find_references`, `explain_ranking`, `list_refs`, `switch_ref`, state export/import, and overlay maintenance CLI. This completes the VCS GA surface together with `005-vcs-core`.

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: git2, tantivy, rusqlite, tokio, serde
**Storage**: Uses VCS core tables/indexes from 005; adds portability/ops flows
**Testing**: fixture-branch integration + MCP contract verification + portability roundtrip

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Tools expose symbol-level and relation-level results |
| II. Single Binary Distribution | PASS | No external service introduced |
| III. Branch/Worktree Correctness | PASS | Tooling consumes ref-correct core state from 005 |
| IV. Incremental by Design | PASS | Import of stale state is recoverable via incremental sync |
| V. Agent-Aware Response Design | PASS | Adds workflow tools for agent branch reasoning |
| VI. Fail-Soft Operation | PASS | Tool failures isolated; core search path remains available |
| VII. Explainable Ranking | PASS | `explain_ranking` is explicit deliverable |

### Pre-Analysis

- **Similar patterns**: MCP handler patterns in 001-004, relation-graph reads in 003, VCS merge baseline from 005.
- **Dependencies**: Requires published overlay correctness from 005 before activation.
- **Conventions**: deterministic responses, action-oriented error hints, structured tracing.
- **Risk areas**: tool registration drift, portability bundle compatibility, stale import behavior.

## Goals / Non-Goals

**Goals:**
1. Deliver analysis tools (`diff_context`, `find_references`, `explain_ranking`) with symbol-level precision.
2. Add ref lifecycle helpers (`list_refs`, `switch_ref`) for predictable multi-branch workflows.
3. Provide portable state export/import CLI commands for ephemeral CI environments.
4. Register all VCS GA tooling in MCP `tools/list` and finalize v1.0.0 GA readiness.
5. Bound runtime resource consumption (SQLite connections, fixture test parallelism, cross-process maintenance locking).

**Non-Goals:**
1. No new ranking model complexity beyond explainability surface.
2. No non-Git VCS backend in this milestone.
3. No automated archival policy beyond explicit prune command.

## Decisions

### D1. Separate tooling layer from core correctness

Analysis tooling (`diff_context`, `find_references`, `explain_ranking`) is implemented in `cruxe-query` as distinct modules, with MCP handlers in `cruxe-mcp/src/tools/`. This separation prevents core correctness and analysis tooling from coupling too early.

**Why:** Tool surface can evolve independently from indexing and overlay correctness logic established in 005.

### D2. Fail-soft tool isolation

Tool-specific failures must not block base search workflows (`search_code`, `locate_symbol`). Each VCS GA tool handler returns errors through the canonical error envelope without degrading other tool availability.

**Why:** Agents rely on core search availability; new tools must not introduce regression risk to existing capabilities.

### D3. Portable state bundle format

State export produces a `.tar.zst` archive containing SQLite database, Tantivy index directories, and a `metadata.json` manifest with schema version and export timestamp. Import validates schema version before extraction and marks branches as stale to enable delta recovery via `sync_repo`.

**Why:** Ephemeral CI environments need instant code intelligence without re-indexing; schema version safety prevents silent corruption.

### D4. Bounded SQLite connection cache

MCP runtime uses an LRU eviction strategy with configurable cap (`CRUXE_MAX_OPEN_CONNECTIONS`) to prevent unbounded file-descriptor growth in long-lived multi-workspace servers.

**Why:** Multi-workspace MCP servers must remain stable under sustained request concurrency.

### D5. Cross-process maintenance lock

State-mutating operations (`state import`, `prune-overlays`, overlay publish during `sync_repo`) acquire a per-project lock file at a parent-scoped stable path (`locks/state-maintenance-<path-hash>.lock`). Lock location is stable across import rename-swap operations.

**Why:** Concurrent destructive mutations on the same project data directory can corrupt state; lock-and-fail-fast with retryable guidance is safer than silent races.

### D6. Configurable fixture test parallelism

MCP fixture tests use configurable bounded parallelism (`CRUXE_TEST_FIXTURE_PARALLELISM`) instead of global serial execution (`RUST_TEST_THREADS=1`).

**Why:** Preserves test stability without sacrificing throughput on multi-core CI runners.

### Project Structure

#### Documentation

```text
openspec/changes/archive/2026-02-22-006-vcs-ga-tooling/
  plan.md
  spec.md
  tasks.md
  contracts/
    mcp-tools.md
```

#### Source Code Focus

```text
crates/
  cruxe-query/      # diff_context, find_references, explain_ranking
  cruxe-mcp/        # tool handlers + tools/list registration
  cruxe-state/      # export/import flows
  cruxe-cli/        # state export/import + prune-overlays
```

## Risks / Trade-offs

- **[Risk] Tool registration drift** -> **Mitigation:** Integration test verifies `tools/list` includes all new tools with correct schemas and error code mappings.
- **[Risk] Portability bundle compatibility across versions** -> **Mitigation:** Schema/version safety checks during import; clear version mismatch errors without corrupting local state.
- **[Risk] Stale import behavior** -> **Mitigation:** Imported state marks branches as stale; delta recovery via next `sync_repo` invocation without full re-index.
- **[Risk] Unbounded FD growth in multi-workspace servers** -> **Mitigation:** Bounded SQLite connection cache with idle LRU eviction.
- **[Risk] Concurrent state mutation races** -> **Mitigation:** Per-project cross-process maintenance lock with fail-fast and retryable guidance.

## Migration Plan

1. Deliver analysis tools on top of 005 core merge correctness (Phase 1).
2. Add ref operations and state portability commands (Phases 2-3).
3. Finalize tool registration, maintenance command, and tooling polish (Phases 4-6).
4. Add descriptor stability and test throughput improvements (Phase 7).

Rollback: Individual tools can be unregistered from `tools/list` without affecting core search. State export/import commands are additive CLI surface.
