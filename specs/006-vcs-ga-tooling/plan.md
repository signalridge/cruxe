# Implementation Plan: VCS GA Tooling & Portability

**Branch**: `006-vcs-ga-tooling` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/006-vcs-ga-tooling/spec.md`
**Depends On**: 005-vcs-core
**Version**: v1.0.0

## Summary

Implement the advanced VCS GA surface on top of core overlay correctness:
`diff_context`, `find_references`, `explain_ranking`, `list_refs`, `switch_ref`,
state export/import, and overlay maintenance CLI.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: git2, tantivy, rusqlite, tokio, serde
**Storage**: Uses VCS core tables/indexes from 005; adds portability/ops flows
**Testing**: fixture-branch integration + MCP contract verification + portability roundtrip

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Tools expose symbol-level and relation-level results |
| II. Single Binary Distribution | PASS | No external service introduced |
| III. Branch/Worktree Correctness | PASS | Tooling consumes ref-correct core state from 005 |
| IV. Incremental by Design | PASS | Import of stale state is recoverable via incremental sync |
| V. Agent-Aware Response Design | PASS | Adds workflow tools for agent branch reasoning |
| VI. Fail-Soft Operation | PASS | Tool failures isolated; core search path remains available |
| VII. Explainable Ranking | PASS | `explain_ranking` is explicit deliverable |

## Project Structure

### Documentation (this feature)

```text
specs/006-vcs-ga-tooling/
  plan.md
  spec.md
  tasks.md
  contracts/
    mcp-tools.md
```

### Source Code Focus

```text
crates/
  cruxe-query/      # diff_context, find_references, explain_ranking
  cruxe-mcp/        # tool handlers + tools/list registration
  cruxe-state/      # export/import flows
  cruxe-cli/        # state export/import + prune-overlays
```

## Pre-Analysis

- **Similar patterns**: MCP handler patterns in 001-004, relation-graph reads in 003, VCS merge baseline from 005.
- **Dependencies**: Requires published overlay correctness from 005 before activation.
- **Conventions**: deterministic responses, action-oriented error hints, structured tracing.
- **Risk areas**: tool registration drift, portability bundle compatibility, stale import behavior.

## Complexity Tracking

### Justified Complexity

1. Separate tooling layer prevents core correctness and analysis tooling from coupling too early.
2. Portability commands improve CI and ephemeral environment operability.
3. Dedicated tool contracts reduce ambiguity for MCP clients.

### Avoided Complexity

- No new ranking model complexity beyond explainability surface.
- No non-Git VCS backend in this milestone.
- No automated archival policy beyond explicit prune command.
