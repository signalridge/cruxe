## Context

Enhance the MCP protocol layer with agent-aware response design (`detail_level`, `compact`), file outline tool (`get_file_outline`), operational health reporting (`health_check`), Tantivy index prewarming, debug ranking explanations with `ranking_explain_level`, and stale-aware query behavior refinement. These changes are additive to the 001-core-mvp codebase and introduce no new storage schemas or external dependencies. The `detail_level` parameter and `ranking_reasons` field fulfill Constitution principles V and VII respectively.

**Language/Version**: Rust (latest stable, 2024 edition) -- same as 001-core-mvp
**Primary Dependencies**: No new crate dependencies; uses existing tantivy, rusqlite, serde, tokio, tracing
**Storage**: No schema changes. Uses existing Tantivy indices and SQLite `symbol_relations` table.
**Testing**: cargo test + fixture repos from 001-core-mvp
**Performance Goals**: `get_file_outline` p95 < 50ms, `health_check` p95 < 10ms, prewarm first-query p95 < 500ms
**Constraints**: All changes are backward-compatible with Protocol v1 response contract

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | N/A | No changes to core navigation |
| II. Single Binary Distribution | PASS | No new dependencies or external services |
| III. Branch/Worktree Correctness | PASS | `get_file_outline` is ref-scoped; freshness check is ref-aware |
| IV. Incremental by Design | PASS | Stale-aware query triggers async incremental sync |
| V. Agent-Aware Response Design | **FULFILLED** | `detail_level` parameter is the Phase 1.1 MUST -- location/signature/context |
| VI. Fail-Soft Operation | PASS | Prewarm failure does not block queries; best_effort policy always returns |
| VII. Explainable Ranking | **FULFILLED** | `ranking_reasons` debug field is the Phase 1.1 MUST |

## Goals / Non-Goals

**Goals:**
1. Provide agent-controlled response verbosity via `detail_level` and `compact` parameters (Constitution V).
2. Provide `get_file_outline` for symbol-tree navigation without full-file reads.
3. Provide `health_check` for operational readiness and startup compatibility inspection.
4. Reduce cold-start latency via Tantivy index prewarming on `serve-mcp` startup.
5. Provide transparent ranking explanations via `ranking_explain_level` (Constitution VII).
6. Provide configurable stale-aware query behavior via `freshness_policy`.

**Non-Goals:**
1. New storage schemas or external dependencies.
2. New crate boundaries -- all changes fit within existing crate structure.
3. Breaking changes to Protocol v1 response contract.

## Decisions

### D1. `detail_level` + `compact` as serialization filters

Query and ranking logic remains unchanged regardless of `detail_level`. The parameter controls only which fields are included in the serialized response. `compact` further strips large optional blocks while preserving identity/location/score/follow-up handles.

**Why:** serialization-only filtering reduces risk and keeps the ranking pipeline deterministic.

### D2. `get_file_outline` as pure SQLite query

The file outline tool queries the existing `symbol_relations` SQLite table using `parent_symbol_id` chains to build nested trees. No Tantivy involvement.

**Why:** avoids Tantivy complexity for a structural query; leverages existing relational data with trivial latency.

### D3. Prewarm as async-after-handshake

On `serve-mcp` startup, the MCP server loop starts first and accepts connections. Prewarm runs asynchronously in the background, touching segment metadata and running warmup queries. `--no-prewarm` flag provides opt-out.

**Why:** preserves MCP readiness while warming indices in background; handshake requests never block on prewarm.

### D4. Freshness policy as enum

Three clear levels (`strict`, `balanced`, `best_effort`) with deterministic behavior per level. Configurable in `config.toml` and overridable per-request.

**Why:** avoids ambiguous intermediate states; each level has well-defined query/sync behavior.

### D5. `ranking_reasons` gated by `ranking_explain_level`

Three levels: `off` (default, zero overhead), `basic` (compact normalized factors for agent routing), `full` (verbose debug breakdown for diagnostics).

**Why:** zero-overhead default preserves latency; tiered output serves both agent and human debugging use cases.

### D6. No new crates

All changes fit within existing crate boundaries. `freshness.rs` is the only new file in an existing crate; all other changes are additions to existing files.

### Project Structure

Documentation (this feature):

```text
openspec/changes/archive/2026-02-22-002-agent-protocol/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts for get_file_outline, health_check
└── tasks.md             # Actionable task list
```

Source code changes (relative to 001-core-mvp structure):

```text
crates/
├── cruxe-core/
│   └── src/
│       └── types.rs          # Add DetailLevel enum, FreshnessPolicy enum, RankingReasons struct
├── cruxe-query/
│   └── src/
│       ├── ranking.rs        # Add ranking_reasons collection to reranker
│       ├── freshness.rs      # NEW: pre-query freshness check + policy logic
│       ├── search.rs         # Add detail_level/compact/freshness_policy params
│       └── locate.rs         # Add detail_level/compact params
├── cruxe-state/
│   └── src/
│       ├── symbols.rs        # Add get_file_outline query (SQLite)
│       └── tantivy_index.rs  # Add prewarm logic
├── cruxe-mcp/
│   └── src/
│       ├── tools/
│       │   ├── mod.rs        # Register get_file_outline, health_check
│       │   ├── search_code.rs    # Wire detail_level, compact, freshness_policy, ranking_reasons
│       │   ├── locate_symbol.rs  # Wire detail_level, compact, ranking_reasons
│       │   ├── get_file_outline.rs  # NEW: file outline tool handler
│       │   └── health_check.rs      # NEW: health check tool handler
│       ├── protocol.rs       # Add RankingReasons to metadata, DetailLevel to input schemas
│       └── server.rs         # Add prewarm call before accepting connections
└── cruxe-cli/
    └── src/
        └── commands/
            └── serve_mcp.rs  # Add --no-prewarm flag

configs/
└── default.toml              # Add freshness policy and ranking explainability defaults
```

## Risks / Trade-offs

- **[Risk] Serialization-only detail filtering may surprise callers expecting query behavior changes** --> **Mitigation:** document that `detail_level` affects output shape only; ranking and retrieval are identical across levels.
- **[Risk] Prewarm on large indices may consume significant memory briefly** --> **Mitigation:** async non-blocking execution; `--no-prewarm` opt-out; health status visible during warmup.
- **[Risk] Freshness check adds latency to every query** --> **Mitigation:** lightweight signals only (HEAD commit comparison for VCS, manifest hash cursor for single-version); target < 5ms overhead.
- **[Risk] `ranking_explain_level: "full"` may expose internal scoring details** --> **Mitigation:** gated behind explicit opt-in; `off` is default with zero overhead.
