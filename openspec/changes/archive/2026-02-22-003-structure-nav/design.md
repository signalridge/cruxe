## Context

Add structural navigation capabilities to Cruxe: populate `symbol_edges` with import relationships extracted via tree-sitter, implement `get_symbol_hierarchy` for parent chain traversal, `find_related_symbols` for scope-based discovery, and `get_code_context` with token budget-aware response construction. These tools enable AI coding agents to navigate code structure efficiently without reading entire files.

**Language/Version**: Rust (latest stable, 2024 edition)
**Builds On**: All crates from 001-core-mvp, Protocol v1 from 002-agent-protocol
**Modified Crates**: `cruxe-indexer` (import extraction), `cruxe-query` (hierarchy + related + context), `cruxe-mcp` (new tool handlers), `cruxe-state` (edge CRUD), `cruxe-core` (token estimation)
**New Dependencies**: None (uses existing tree-sitter, rusqlite, tantivy, serde)
**Storage Changes**: No new tables. Populates existing `symbol_edges` table (schema-ready from 001-core-mvp).
**Performance Goals**: hierarchy/related p95 < 200ms warm, get_code_context p95 < 500ms warm
**Constraints**: Token estimation must be conservative (never exceed budget), import resolution is best-effort

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | `get_symbol_hierarchy` and `find_related_symbols` provide symbol-level structural navigation |
| II. Single Binary Distribution | PASS | No new external dependencies |
| III. Branch/Worktree Correctness | PASS | All queries are ref-scoped via existing `repo + ref` filtering |
| IV. Incremental by Design | PASS | Import edges replaced per-file atomically during re-indexing |
| V. Agent-Aware Response Design | **PASS (MUST)** | `get_code_context` with `max_tokens` fulfills the Phase 1.5 MUST requirement for token budget management |
| VI. Fail-Soft Operation | PASS | Unresolved import targets stored but skipped in navigation; empty results returned gracefully |
| VII. Explainable Ranking | PASS | Not directly applicable; context strategy is explicit (breadth/depth) |

## Goals / Non-Goals

**Goals:**
1. Populate `symbol_edges` with import relationships extracted via tree-sitter for Rust, TypeScript, Python, and Go.
2. Implement `get_symbol_hierarchy` for ancestor/descendant chain traversal.
3. Implement `find_related_symbols` for scope-based (file, module, package) symbol discovery.
4. Implement `get_code_context` with token budget-aware breadth/depth strategies.
5. All new tools include Protocol v1 metadata and ref-scoped queries.

**Non-Goals:**
1. `calls` edge type — explicitly deferred to Phase 2.5 (spec 007-call-graph).
2. `heuristic` confidence level — not used in this phase (all edges are `static`).
3. `compact` flag for `003` tools — deferred; these tools use token-budget controls instead.

## Decisions

### D1. Three separate query modules

Three new query modules (`hierarchy.rs`, `related.rs`, `context.rs`) rather than a single `navigation.rs`.

**Why:** Each has distinct data access patterns: hierarchy is recursive parent traversal, related combines file scope + edge graph, context involves token budgeting + search. Keeping them separate avoids a monolithic module.

### D2. Import edge extraction pipeline

1. During `cruxe index`, after symbol extraction per file, run import extraction on the same tree-sitter parse tree.
2. For each import/use/require node, extract the qualified name of the imported symbol.
3. Resolve the qualified name to a `symbol_stable_id` by querying `symbol_relations` (best-effort: if not found, store unresolved edge with `to_symbol_id = NULL` and `to_name = target_name`).
4. Insert edges into `symbol_edges` with `edge_type='imports'`, `confidence='static'`.
5. Before inserting, delete all existing `imports` edges where `from_symbol_id` belongs to the current file (atomic per-file replacement).

**Why:** Reuses the existing tree-sitter parse tree, avoids a separate pass, and ensures idempotent per-file replacement.

### D3. Cross-file resolution strategy

Import resolution is best-effort within the indexed codebase:

- **Rust**: `use crate::module::Symbol` -> look up `Symbol` in `module` path via `qualified_name` matching in `symbol_relations`.
- **Python**: `from package.module import name` -> match `package.module.name` as qualified name.
- **TypeScript**: `import { Name } from './path'` -> resolve relative path to absolute, then match by name in that file's symbols.
- **Go**: `import "package/path"` -> match package-level symbols by import path.

External dependencies (not in indexed codebase) produce unresolved edges with `to_symbol_id = NULL` and populated `to_name`. These are stored for completeness and can be counted/reported as unresolved references by navigation tools.

**Why:** Best-effort resolution avoids blocking indexing on external dependency resolution while still capturing the import graph structure.

### D4. Token estimation

Conservative approximation: `estimated_tokens = ceil(word_count * 1.3)` where `word_count` is the number of whitespace-split tokens in the serialized response.

**Why:** The multiplier 1.3 accounts for subword tokenization in LLM tokenizers where code identifiers like `validateUserToken` count as multiple tokens. Intentional overestimation ensures the budget is never exceeded.

### Project Structure

#### Documentation

```text
openspec/changes/archive/2026-02-22-003-structure-nav/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts
└── tasks.md             # Actionable task list
```

#### Source Code Changes

```text
crates/
├── cruxe-core/
│   └── src/
│       └── tokens.rs            # NEW: Token estimation utility
├── cruxe-indexer/
│   └── src/
│       ├── import_extract.rs    # NEW: Import statement parser dispatcher
│       └── languages/
│           ├── rust.rs           # MODIFY: Add import extraction functions
│           ├── typescript.rs     # MODIFY: Add import extraction functions
│           ├── python.rs         # MODIFY: Add import extraction functions
│           └── go.rs             # MODIFY: Add import extraction functions
├── cruxe-state/
│   └── src/
│       └── edges.rs             # NEW: symbol_edges CRUD operations
├── cruxe-query/
│   └── src/
│       ├── hierarchy.rs         # NEW: get_symbol_hierarchy implementation
│       ├── related.rs           # NEW: find_related_symbols implementation
│       └── context.rs           # NEW: get_code_context implementation
└── cruxe-mcp/
    └── src/
        └── tools/
            ├── mod.rs            # MODIFY: Register new tools
            ├── get_symbol_hierarchy.rs   # NEW: MCP handler
            ├── find_related_symbols.rs   # NEW: MCP handler
            └── get_code_context.rs       # NEW: MCP handler
```

## Risks / Trade-offs

- **[Risk] Import resolution may not find all target symbols (external dependencies)** → **Mitigation:** Store unresolved edges with `to_symbol_id = NULL` and `to_name`; navigation tools skip unresolved edges gracefully.
- **[Risk] Circular imports could cause infinite traversal** → **Mitigation:** All graph traversal implements cycle detection via visited set.
- **[Risk] Token estimation may be inaccurate for non-English or highly symbolic code** → **Mitigation:** Conservative 1.3x multiplier intentionally overestimates; budget is never exceeded.
- **[Risk] `calls` edge type deferred increases future integration scope** → **Mitigation:** Keeping this phase focused on import edges only reduces complexity; `calls` edges are explicitly deferred to Phase 2.5 (spec 007-call-graph).

## Migration Plan

### Incremental Delivery

1. Phase 1: Edge storage + token utility ready
2. Phase 2: Import extractors for all 4 languages
3. Phase 3: Index pipeline produces edges (first demo: `SELECT * FROM symbol_edges`)
4. Phase 4: `get_symbol_hierarchy` works via MCP (independent of edges)
5. Phase 5: `find_related_symbols` uses both relations and edges
6. Phase 6: `get_code_context` with budget control (core agent value)
7. Phase 7: Full validation

### MVP First (Phase 4 + Phase 6)

If time-constrained, Phase 4 (`get_symbol_hierarchy`) and Phase 6 (`get_code_context`) deliver the highest agent value independently of import edge population. They can ship with `find_related_symbols` limited to file scope only.

### Rollback

All new tools are additive; removing them from `tools/list` registration is sufficient to roll back.
