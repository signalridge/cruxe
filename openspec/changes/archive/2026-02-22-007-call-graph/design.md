## Context

Add call edge extraction to the tree-sitter indexing pipeline, store caller-callee relationships in `symbol_edges`, and expose three new MCP tools: `get_call_graph`, `compare_symbol_between_commits`, and `suggest_followup_queries`. This enables AI agents to reason about code structure, change impact, and search strategy refinement.

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: tree-sitter (existing), rusqlite (existing), tantivy (existing), git2 (existing)
**Storage**: SQLite `symbol_edges` table (existing schema, new `edge_type = 'calls'` entries)
**Testing**: cargo test + fixture repos with known call graphs
**Performance Goals**: call graph query p95 < 500ms for depth <= 2, edge extraction adds < 20% to index time
**Constraints**: Must not degrade existing indexing performance by more than 20%
**Scale/Scope**: Up to 50,000 symbols, up to 200,000 call edges per repo

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | Call graph is a direct extension of navigation capability |
| II. Single Binary Distribution | PASS | No new external dependencies |
| III. Branch/Worktree Correctness | PASS | All edges are ref-scoped; compare tool works across refs |
| IV. Incremental by Design | PASS | Edge extraction is per-file; incremental sync updates edges for changed files only |
| V. Agent-Aware Response Design | PASS | `suggest_followup_queries` is explicitly agent-guidance |
| VI. Fail-Soft Operation | PASS | Unresolvable calls stored with NULL target; extraction errors skip file gracefully |
| VII. Explainable Ranking | N/A | Call graph is structural, not ranked |

## Goals / Non-Goals

**Goals:**
1. Extract call edges from source files using tree-sitter during indexing for Rust, TypeScript, Python, and Go.
2. Expose `get_call_graph` MCP tool for graph traversal with configurable direction, depth, and limit.
3. Expose `compare_symbol_between_commits` MCP tool for cross-ref symbol diffing.
4. Expose `suggest_followup_queries` MCP tool for agent search guidance.
5. Maintain indexing performance within 20% overhead budget.

**Non-Goals:**
1. Full type inference or dataflow analysis for call resolution (best-effort qualified name matching only).
2. Dynamic/runtime call tracing.
3. Cross-repository call graph linking.

## Decisions

### D1. Call extraction as a second-pass module

Call edge extraction is a new module (`call_extract.rs`) in `cruxe-indexer` rather than extending `symbol_extract.rs`, because call analysis requires a second pass over the AST after symbols are extracted (callee resolution depends on the symbol index).

**Why:** The `call_extract.rs` module runs after symbol extraction within the same indexing pipeline, keeping the two concerns separated while sharing the parsed AST.

### D2. Per-language call site patterns

Each supported language gets call site query patterns added to its existing language module (`rust.rs`, `typescript.rs`, `python.rs`, `go.rs`).

**Why:** Language-specific AST node types differ significantly (e.g., `call_expression` vs `method_call_expression` in Rust, `call` + `attribute` in Python), so per-language dispatching is necessary.

### D3. Confidence classification

Call edges are classified as `static` (direct, unambiguous calls) or `heuristic` (method calls with ambiguous receiver type, trait objects, function pointers, closures).

**Why:** Downstream consumers (agents, UI) need to know how trustworthy an edge is. Binary confidence is simple to reason about and avoids false precision.

### D4. Cross-file resolution by qualified name matching

Callee names are matched against qualified names in the symbol index for the same ref. Unresolved targets get `to_symbol_id = NULL` with `to_name` preserved.

**Why:** Best-effort resolution without full type inference. Preserves unresolved edges rather than dropping them, so agents can still see external dependencies.

### D5. Graph traversal with depth cap

BFS traversal capped at depth 5 with cycle detection via visited set. Depth > 5 is rejected with a warning in metadata.

**Why:** Prevents runaway traversal in large codebases while covering most practical use cases (most call chains of interest are within 2-3 levels).

### Project Structure

#### Documentation

```text
openspec/changes/archive/2026-02-22-007-call-graph/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts
└── tasks.md             # Actionable task list
```

#### Source Code Changes

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

## Risks / Trade-offs

- **[Risk] Heuristic confidence edges may create false positives in call graphs** → **Mitigation:** Confidence is exposed in responses so agents can filter; `static`-only mode can be added later.
- **[Risk] Cross-file resolution by name matching may produce incorrect edges when multiple symbols share a name** → **Mitigation:** Qualified name matching (not just simple name) reduces ambiguity; unresolved targets are preserved with `NULL` rather than guessing.
- **[Risk] Call extraction adds overhead to indexing** → **Mitigation:** Budgeted at < 20% overhead; second pass reuses already-parsed AST; benchmarked in Phase 6.
- **[Risk] Depth cap of 5 may be insufficient for some use cases** → **Mitigation:** Cap is configurable; most practical queries use depth 1-2.

## Resolved Questions

1. Call extraction is a well-bounded second pass over files during indexing. Graph traversal is bounded by depth cap (5) and result limit. No constitution violations identified.
