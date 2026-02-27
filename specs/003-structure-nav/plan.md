# Implementation Plan: Symbol Structure & Navigation

**Branch**: `003-structure-nav` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Version**: v0.3.0-rc
**Depends On**: 002-agent-protocol (Phase 1.1)
**Input**: Feature specification from `/specs/003-structure-nav/spec.md`

## Summary

Add structural navigation capabilities to Cruxe: populate `symbol_edges`
with import relationships extracted via tree-sitter, implement `get_symbol_hierarchy`
for parent chain traversal, `find_related_symbols` for scope-based discovery, and
`get_code_context` with token budget-aware response construction. These tools
enable AI coding agents to navigate code structure efficiently without reading
entire files.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Builds On**: All crates from 001-core-mvp, Protocol v1 from 002-agent-protocol
**Modified Crates**: `cruxe-indexer` (import extraction), `cruxe-query` (hierarchy + related + context), `cruxe-mcp` (new tool handlers), `cruxe-state` (edge CRUD), `cruxe-core` (token estimation)
**New Dependencies**: None (uses existing tree-sitter, rusqlite, tantivy, serde)
**Storage Changes**: No new tables. Populates existing `symbol_edges` table (schema-ready from 001-core-mvp).
**Performance Goals**: hierarchy/related p95 < 200ms warm, get_code_context p95 < 500ms warm
**Constraints**: Token estimation must be conservative (never exceed budget), import resolution is best-effort

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | `get_symbol_hierarchy` and `find_related_symbols` provide symbol-level structural navigation |
| II. Single Binary Distribution | PASS | No new external dependencies |
| III. Branch/Worktree Correctness | PASS | All queries are ref-scoped via existing `repo + ref` filtering |
| IV. Incremental by Design | PASS | Import edges replaced per-file atomically during re-indexing |
| V. Agent-Aware Response Design | **PASS (MUST)** | `get_code_context` with `max_tokens` fulfills the Phase 1.5 MUST requirement for token budget management |
| VI. Fail-Soft Operation | PASS | Unresolved import targets stored but skipped in navigation; empty results returned gracefully |
| VII. Explainable Ranking | PASS | Not directly applicable; context strategy is explicit (breadth/depth) |

## Project Structure

### Documentation (this feature)

```text
specs/003-structure-nav/
├── plan.md              # This file
├── spec.md              # Feature specification
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts
└── tasks.md             # Actionable task list
```

### Source Code Changes (repository root)

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

**Structure Decision**: Three new query modules (`hierarchy.rs`, `related.rs`,
`context.rs`) rather than a single `navigation.rs`, because each has distinct
data access patterns: hierarchy is recursive parent traversal, related combines
file scope + edge graph, context involves token budgeting + search. Keeping them
separate avoids a monolithic module.

## Implementation Approach

### Import Edge Extraction Pipeline

1. During `cruxe index`, after symbol extraction per file, run import
   extraction on the same tree-sitter parse tree.
2. For each import/use/require node, extract the qualified name of the imported
   symbol.
3. Resolve the qualified name to a `symbol_stable_id` by querying
   `symbol_relations` (best-effort: if not found, derive a synthetic stable ID
   from the qualified name for graph storage).
4. Insert edges into `symbol_edges` with `edge_type='imports'`,
   `confidence='static'`.
5. Before inserting, delete all existing `imports` edges where `from_symbol_id`
   belongs to the current file (atomic per-file replacement).

### Cross-File Resolution Strategy

Import resolution is best-effort within the indexed codebase:

- **Rust**: `use crate::module::Symbol` -> look up `Symbol` in `module` path
  via `qualified_name` matching in `symbol_relations`.
- **Python**: `from package.module import name` -> match `package.module.name`
  as qualified name.
- **TypeScript**: `import { Name } from './path'` -> resolve relative path to
  absolute, then match by name in that file's symbols.
- **Go**: `import "package/path"` -> match package-level symbols by import path.

External dependencies (not in indexed codebase) produce edges with synthetic
`to_symbol_id` that do not resolve to `symbol_relations` rows. These are stored
for completeness but skipped by navigation tools.

### Token Estimation

Conservative approximation: `estimated_tokens = ceil(word_count * 1.3)` where
`word_count` is the number of whitespace-split tokens in the serialized response.
This intentionally overestimates to ensure the budget is never exceeded.

The multiplier 1.3 accounts for subword tokenization in LLM tokenizers where
code identifiers like `validateUserToken` count as multiple tokens.

## Complexity Tracking

No constitution violations. The `calls` edge type is explicitly deferred to
Phase 2.5 to keep this phase focused on import edges only. The `heuristic`
confidence level is not used in this phase (all edges are `static`).
