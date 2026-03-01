## Why

AI coding agents need to navigate code structure (parent chains, related symbols, import graphs) without reading entire files. Phase 1.5a of the development plan calls for symbol edge population, hierarchy traversal, related symbol discovery, and token budget-aware context retrieval. Without import edges, structural navigation tools cannot traverse the import graph or provide cross-file context. Without token-budget controls, agents receive fixed-size results that waste their finite context windows.

## What Changes

1. Parse import/use/require statements via tree-sitter for Rust, TypeScript, Python, and Go and populate the `symbol_edges` table with `imports` edges (US1).
2. Implement `get_symbol_hierarchy` MCP tool for ancestor/descendant traversal via `parent_symbol_id` in `symbol_relations` (US2).
3. Implement `find_related_symbols` MCP tool for scope-based symbol discovery (file, module, package) using `symbol_relations` and `symbol_edges` (US3).
4. Implement `get_code_context` MCP tool with `max_tokens` budget and breadth/depth strategies for token-aware context retrieval (US4).

## Capabilities

### New Capabilities

- **FR-200**: System MUST parse import/use/require statements via tree-sitter for
  Rust, TypeScript, Python, and Go and populate the `symbol_edges` table with
  `edge_type='imports'` and `confidence='static'`.
- **FR-201**: Import edge extraction MUST resolve target symbols to
  `symbol_stable_id` using best-effort qualified name matching against
  `symbol_relations`.
- **FR-202**: Import edges MUST be replaced atomically per source file during
  re-indexing (delete all edges from that file's symbols, then insert new edges).
- **FR-203**: System MUST provide `get_symbol_hierarchy` MCP tool that traverses
  `parent_symbol_id` in `symbol_relations` and returns the ancestor or descendant
  chain for a given symbol.
- **FR-204**: `get_symbol_hierarchy` MUST accept `direction: "ancestors"` (leaf to
  root) and `direction: "descendants"` (root to leaves) parameters.
- **FR-205**: System MUST provide `find_related_symbols` MCP tool that returns
  symbols in the same scope (file, module, or package) using `symbol_relations`
  and `symbol_edges`.
- **FR-206**: `find_related_symbols` MUST prioritize results: same-file symbols
  first, then same-module symbols, then import-connected symbols.
- **FR-207**: System MUST provide `get_code_context` MCP tool with a `max_tokens`
  parameter that constrains the total estimated token count of the response.
- **FR-208**: `get_code_context` MUST support `strategy: "breadth"` (more symbols,
  signature-level detail) and `strategy: "depth"` (fewer symbols, includes body).
- **FR-209**: Token estimation MUST use whitespace-split word count multiplied by
  1.3 as a conservative approximation.
- **FR-210**: `get_code_context` MUST include `estimated_tokens`, `truncated`, and
  `metadata` fields in every response.
- **FR-211**: All new MCP tools MUST include Protocol v1 metadata in responses
  (`cruxe_protocol_version`, `freshness_status`, `indexing_status`,
  `result_completeness`, `ref`) and use canonical enums:
  `indexing_status` = `not_indexed | indexing | ready | failed`,
  `result_completeness` = `complete | partial | truncated`.
- **FR-212**: All new MCP tools MUST accept an optional `ref` parameter for
  ref-scoped queries, defaulting to current HEAD or `"live"`.
- **FR-213**: `get_code_context` default `max_tokens` MUST be 4000 when the
  parameter is not provided.
- **FR-214**: Graph traversal in `get_symbol_hierarchy` and `find_related_symbols`
  MUST implement cycle detection to handle circular references safely.
- **FR-215**: `get_symbol_hierarchy`, `find_related_symbols`, and `get_code_context`
  in `003` rely on token-budget controls rather than a dedicated `compact` flag;
  any `compact` extension for these tools is explicitly deferred.

### Modified Capabilities

- `001-core-mvp`: Populates existing `symbol_edges` table (schema-ready from 001-core-mvp) with `imports` edge data.

### Key Entities

- **SymbolEdge**: A directed relationship between two symbols (import, call, etc.)
  stored in the `symbol_edges` table with source/target `symbol_stable_id`,
  edge type, and confidence level.
- **SymbolHierarchy**: An ordered chain of symbols from leaf to root (ancestors)
  or root to leaves (descendants), derived from `parent_symbol_id` traversal.
- **CodeContext**: A token budget-fitted collection of code symbols and/or bodies,
  assembled according to a breadth or depth strategy, with estimated token count.

### Implementation Alignment (2026-02-25)

- `symbol_edges` schema now includes composite indexes for forward/reverse
  `(repo, ref, symbol_id, edge_type)` lookup paths.
- Query-shape regression tests verify index-backed plans for forward and reverse
  typed edge traversals.

## Impact

- **SC-200**: Import edges are correctly extracted for >= 90% of import statements
  in fixture repositories across all four v1 languages.
- **SC-201**: `get_symbol_hierarchy` returns the correct ancestor chain for >= 95%
  of symbols in fixture repositories.
- **SC-202**: `get_code_context` never exceeds the requested `max_tokens` budget
  (measured by the same estimation function used in the response).
- **SC-203**: All three new MCP tools respond within 200ms p95 on a warm index
  for single-symbol queries.
- **SC-204**: `find_related_symbols` returns at least one related symbol for >= 80%
  of non-isolated symbols in fixture repositories.

### Edge Cases

- Import resolution cannot find the target symbol in the index: the edge is stored with a `to_symbol_id` derived from the qualified name. The edge exists for graph traversal but the target may not resolve to a `symbol_relations` row (external dependency). `find_related_symbols` skips unresolved edges.
- Circular imports: edges are recorded in both directions. Graph traversal tools implement cycle detection (visited set) to avoid infinite loops.
- Very small `max_tokens` (e.g., 10) in `get_code_context`: if no single result fits within the budget, `context_items` is empty, `truncated: true`, and a `suggestion` field recommends increasing the budget.
- `get_symbol_hierarchy` called with `ref` that has no data: returns `symbol_not_found` error with `ref` in the error metadata.
- Stale edges after partial re-indexing: edges are replaced per-file during indexing (all edges with matching `from_symbol_id` for the re-indexed file are deleted and re-created).
- `003` tools do not expose `compact` in this phase: tools are token-budget-driven by design (`max_tokens` + strategy shaping), while `compact` remains focused on `search_code`/`locate_symbol` from `002`. Future phases may add `compact` if benchmark evidence shows agent benefit.

### Affected Crates

- `cruxe-indexer` (import extraction)
- `cruxe-query` (hierarchy + related + context)
- `cruxe-mcp` (new tool handlers)
- `cruxe-state` (edge CRUD)
- `cruxe-core` (token estimation)

### API / Performance Impact

- API impact: three new MCP tools (`get_symbol_hierarchy`, `find_related_symbols`, `get_code_context`).
- Performance targets: hierarchy/related p95 < 200ms warm, get_code_context p95 < 500ms warm.
