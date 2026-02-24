# Implementation Plan: Agent Protocol Enhancement

**Branch**: `002-agent-protocol` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/002-agent-protocol/spec.md`
**Depends On**: `001-core-mvp` — all 001 phases must be complete before implementation begins

## Summary

Enhance the MCP protocol layer with agent-aware response design (`detail_level`,
`compact`), file outline tool (`get_file_outline`), operational health reporting
(`health_check`), Tantivy index prewarming, debug ranking explanations with
`ranking_explain_level`, and stale-aware query behavior refinement. These
changes are additive to the
001-core-mvp codebase and introduce no new storage schemas or external
dependencies. The `detail_level` parameter and `ranking_reasons` field fulfill
Constitution principles V and VII respectively.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition) — same as 001-core-mvp
**Primary Dependencies**: No new crate dependencies; uses existing tantivy, rusqlite, serde, tokio, tracing
**Storage**: No schema changes. Uses existing Tantivy indices and SQLite `symbol_relations` table.
**Testing**: cargo test + fixture repos from 001-core-mvp
**Performance Goals**: `get_file_outline` p95 < 50ms, `health_check` p95 < 10ms, prewarm first-query p95 < 500ms
**Constraints**: All changes are backward-compatible with Protocol v1 response contract

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | N/A | No changes to core navigation |
| II. Single Binary Distribution | PASS | No new dependencies or external services |
| III. Branch/Worktree Correctness | PASS | `get_file_outline` is ref-scoped; freshness check is ref-aware |
| IV. Incremental by Design | PASS | Stale-aware query triggers async incremental sync |
| V. Agent-Aware Response Design | **FULFILLED** | `detail_level` parameter is the Phase 1.1 MUST — location/signature/context |
| VI. Fail-Soft Operation | PASS | Prewarm failure does not block queries; best_effort policy always returns |
| VII. Explainable Ranking | **FULFILLED** | `ranking_reasons` debug field is the Phase 1.1 MUST |

## Project Structure

### Documentation (this feature)

```text
specs/002-agent-protocol/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts for get_file_outline, health_check
└── tasks.md             # Actionable task list
```

No `research.md` or `data-model.md` — this spec adds no new storage schemas
or technology decisions. The 001-core-mvp data model is referenced as-is.

### Source Code Changes (relative to 001-core-mvp structure)

```text
crates/
├── codecompass-core/
│   └── src/
│       └── types.rs          # Add DetailLevel enum, FreshnessPolicy enum, RankingReasons struct
├── codecompass-query/
│   └── src/
│       ├── ranking.rs        # Add ranking_reasons collection to reranker
│       ├── freshness.rs      # NEW: pre-query freshness check + policy logic
│       ├── search.rs         # Add detail_level/compact/freshness_policy params
│       └── locate.rs         # Add detail_level/compact params
├── codecompass-state/
│   └── src/
│       ├── symbols.rs        # Add get_file_outline query (SQLite)
│       └── tantivy_index.rs  # Add prewarm logic
├── codecompass-mcp/
│   └── src/
│       ├── tools/
│       │   ├── mod.rs        # Register get_file_outline, health_check
│       │   ├── search_code.rs    # Wire detail_level, compact, freshness_policy, ranking_reasons
│       │   ├── locate_symbol.rs  # Wire detail_level, compact, ranking_reasons
│       │   ├── get_file_outline.rs  # NEW: file outline tool handler
│       │   └── health_check.rs      # NEW: health check tool handler
│       ├── protocol.rs       # Add RankingReasons to metadata, DetailLevel to input schemas
│       └── server.rs         # Add prewarm call before accepting connections
└── codecompass-cli/
    └── src/
        └── commands/
            └── serve_mcp.rs  # Add --no-prewarm flag

configs/
└── default.toml              # Add freshness policy and ranking explainability defaults
```

**Structure Decision**: No new crates. All changes fit within existing crate
boundaries. `freshness.rs` is the only new file in an existing crate; all
other changes are additions to existing files.

## Complexity Tracking

No constitution violations to justify. All changes are additive to existing
architecture.

| Decision | Rationale |
|----------|-----------|
| `detail_level` + `compact` as serialization filters | Keeps query/ranking logic unchanged; serialization-only reduces risk |
| `get_file_outline` as pure SQLite query | Avoids Tantivy involvement; leverages existing `symbol_relations` table |
| Prewarm as async-after-handshake | Preserves MCP readiness while warming indices in background; `--no-prewarm` for opt-out |
| Freshness policy as enum | Three clear levels avoid ambiguous intermediate states |
| `ranking_reasons` gated by `ranking_explain_level` | `off` keeps zero-overhead default, `basic`/`full` enable explainability as needed |
