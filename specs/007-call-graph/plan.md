# Implementation Plan: Call Graph Analysis

**Branch**: `007-call-graph` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md) | **Depends On**: 006-vcs-ga-tooling
**Input**: Feature specification from `/specs/007-call-graph/spec.md`

## Summary

Add call edge extraction to the tree-sitter indexing pipeline, store caller-callee
relationships in `symbol_edges`, and expose three new MCP tools: `get_call_graph`,
`compare_symbol_between_commits`, and `suggest_followup_queries`. This enables AI agents
to reason about code structure, change impact, and search strategy refinement.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: tree-sitter (existing), rusqlite (existing), tantivy (existing), git2 (existing)
**Storage**: SQLite `symbol_edges` table (existing schema, new `edge_type = 'calls'` entries)
**Testing**: cargo test + fixture repos with known call graphs
**Performance Goals**: call graph query p95 < 500ms for depth <= 2, edge extraction adds < 20% to index time
**Constraints**: Must not degrade existing indexing performance by more than 20%
**Scale/Scope**: Up to 50,000 symbols, up to 200,000 call edges per repo

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Call graph is a direct extension of navigation capability |
| II. Single Binary Distribution | PASS | No new external dependencies |
| III. Branch/Worktree Correctness | PASS | All edges are ref-scoped; compare tool works across refs |
| IV. Incremental by Design | PASS | Edge extraction is per-file; incremental sync updates edges for changed files only |
| V. Agent-Aware Response Design | PASS | `suggest_followup_queries` is explicitly agent-guidance |
| VI. Fail-Soft Operation | PASS | Unresolvable calls stored with NULL target; extraction errors skip file gracefully |
| VII. Explainable Ranking | N/A | Call graph is structural, not ranked |

## Project Structure

### Documentation (this feature)

```text
specs/007-call-graph/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts
└── tasks.md             # Actionable task list
```

### Source Code Changes

```text
crates/
├── cruxe-indexer/
│   └── src/
│       ├── call_extract.rs          # NEW: Call edge extraction from tree-sitter AST
│       ├── languages/
│       │   ├── rust.rs              # ADD: call site query patterns
│       │   ├── typescript.rs        # ADD: call site query patterns
│       │   ├── python.rs            # ADD: call site query patterns
│       │   └── go.rs                # ADD: call site query patterns
│       └── writer.rs                # UPDATE: write call edges to symbol_edges
├── cruxe-state/
│   └── src/
│       └── edges.rs                 # NEW: symbol_edges CRUD for call edges
├── cruxe-query/
│   └── src/
│       ├── call_graph.rs            # NEW: call graph traversal (BFS with depth limit)
│       ├── symbol_compare.rs        # NEW: cross-ref symbol comparison
│       └── followup.rs              # NEW: follow-up query suggestion engine
├── cruxe-mcp/
│   └── src/
│       └── tools/
│           ├── get_call_graph.rs    # NEW: MCP tool handler
│           ├── compare_symbol.rs    # NEW: MCP tool handler
│           └── suggest_followup.rs  # NEW: MCP tool handler

testdata/
├── fixtures/
│   └── call-graph-sample/           # NEW: fixture repo with known call chains
└── golden/
    ├── call-graph-depth1.json       # Expected output for depth-1 query
    └── call-graph-depth2.json       # Expected output for depth-2 query
```

**Structure Decision**: Call edge extraction is a new module in `cruxe-indexer`
rather than extending `symbol_extract.rs`, because call analysis requires a second pass
over the AST after symbols are extracted (callee resolution depends on the symbol index).
The `call_extract.rs` module runs after symbol extraction within the same indexing pipeline.

## Complexity Tracking

No constitution violations to justify. Call edge extraction adds a well-bounded second
pass over files during indexing. Graph traversal is bounded by depth cap (5) and result
limit.
