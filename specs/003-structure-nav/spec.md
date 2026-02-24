# Feature Specification: Symbol Structure & Navigation

**Feature Branch**: `003-structure-nav`
**Created**: 2026-02-23
**Status**: Draft
**Version**: v0.3.0-rc
**Depends On**: 002-agent-protocol
**Input**: Phase 1.5a items from development plan: symbol edge population, hierarchy traversal, related symbol discovery, and token budget-aware context retrieval.

## User Scenarios & Testing

### User Story 1 - Populate Import Edges from Source Code (Priority: P1)

A developer (or the indexer pipeline) runs `codecompass index` on a repository.
In addition to extracting symbol definitions (already implemented in 001-core-mvp),
the system now parses import/use/require statements via tree-sitter and populates
the `symbol_edges` table with `imports` edges. Each edge links the importing
symbol (or file-level module) to the imported symbol via `symbol_stable_id`.
Edge confidence is tagged `static` (parser-confirmed only in this phase).

**Why this priority**: Import edges are the foundation for structural navigation.
Without them, `find_related_symbols` cannot traverse the import graph, and
`get_symbol_hierarchy` lacks cross-file context.

**Independent Test**: Index a fixture repository with known import relationships,
then query `symbol_edges` for a specific file and verify import edges match the
actual import statements in source.

**Acceptance Scenarios**:

1. **Given** a Rust file with `use crate::auth::Claims;`, **When** indexing completes,
   **Then** a row exists in `symbol_edges` with `edge_type='imports'`,
   `from_symbol_id` matching the file/module symbol, `to_symbol_id` matching
   the `Claims` symbol's `symbol_stable_id`, and `confidence='static'`.
2. **Given** a Python file with `from auth.jwt import validate_token`, **When**
   indexing completes, **Then** an import edge links the module to `validate_token`
   with `confidence='static'`.
3. **Given** a TypeScript file with `import { Router } from 'express'`, **When**
   indexing completes, **Then** the edge is created with best-effort resolution.
   If the target symbol is not in the indexed codebase (external dependency),
   the edge is stored with `to_symbol_id` derived from the qualified name but
   may not resolve to a `symbol_relations` row.
4. **Given** a Go file with `import "github.com/org/pkg/auth"`, **When** indexing
   completes, **Then** an import edge is created for the package-level import.
5. **Given** a file with no import statements, **When** indexing completes,
   **Then** no `imports` edges are created for that file.
6. **Given** a re-index (`codecompass index`) after file changes, **When** a file's
   imports change, **Then** old import edges for that file are replaced with the
   new set (idempotent per-file replacement).

---

### User Story 2 - Navigate Symbol Hierarchy (Priority: P1)

An AI coding agent calls `get_symbol_hierarchy` to understand the structural
context of a symbol. Given a method name, the tool returns the parent chain:
method -> class/impl -> module -> package. Given a class name with
`direction: "descendants"`, it returns child methods and nested types.

**Why this priority**: Agents need structural context to understand where a
symbol lives in the codebase architecture. This is more precise than grep and
cheaper (in tokens) than reading entire files.

**Independent Test**: Index a fixture repository with nested symbols (class
containing methods, module containing classes). Call `get_symbol_hierarchy` for
a deeply nested method and verify the returned chain matches the actual nesting.

**Acceptance Scenarios**:

1. **Given** a method `validate` inside `impl AuthHandler` in module `auth::handler`,
   **When** `get_symbol_hierarchy` is called with `symbol_name: "validate"`,
   `path: "src/auth/handler.rs"`, `direction: "ancestors"`, **Then** the response
   contains the chain: `validate` (fn) -> `AuthHandler` (impl) -> file root,
   with each node including `kind`, `name`, `line_start`, and `line_end`.
2. **Given** a class `UserService` with three methods, **When**
   `get_symbol_hierarchy` is called with `direction: "descendants"`, **Then**
   the response lists all three child methods.
3. **Given** a symbol name that exists in multiple files, **When** `path` is
   provided, **Then** the result is scoped to the specified file.
4. **Given** a symbol name that does not exist, **When** the tool is called,
   **Then** an error response with code `symbol_not_found` is returned.
5. **Given** a top-level function (no parent), **When** `direction: "ancestors"`
   is used, **Then** the hierarchy contains only the function itself (chain
   length 1).

---

### User Story 3 - Find Related Symbols (Priority: P2)

An AI coding agent calls `find_related_symbols` to discover symbols in the same
scope as a given symbol. This helps agents understand co-located functionality
without reading entire files or modules.

**Why this priority**: Related symbol discovery reduces the number of tool calls
an agent needs to build a mental model of a code area.

**Independent Test**: Index a fixture repository, call `find_related_symbols` for
a known function with `scope: "file"`, verify the response includes sibling
symbols from the same file.

**Acceptance Scenarios**:

1. **Given** a file `src/auth/handler.rs` with 5 symbols, **When**
   `find_related_symbols` is called with `scope: "file"` for any symbol in that
   file, **Then** the response includes all other symbols in the file.
2. **Given** `scope: "module"`, **When** the tool is called for a symbol in
   `auth::handler`, **Then** the response includes symbols from sibling files
   in the `auth` module (e.g., `auth::jwt`, `auth::error`).
3. **Given** `scope: "module"` and the symbol has import edges, **When** the tool
   is called, **Then** imported symbols from within the same module are included
   in the results.
4. **Given** `limit: 5` with more than 5 related symbols, **When** the tool is
   called, **Then** at most 5 results are returned, prioritized by relevance
   (same-file > same-module > imported).
5. **Given** a symbol with no related symbols (isolated file, no imports), **When**
   the tool is called, **Then** an empty `related` array is returned with
   `scope_used: "file"`.

---

### User Story 4 - Get Token Budget-Aware Code Context (Priority: P1)

An AI coding agent calls `get_code_context` with a query and a `max_tokens`
budget. The tool retrieves relevant code context and fits it within the token
budget, using either a `breadth` strategy (more symbols, less detail) or a
`depth` strategy (fewer symbols, full bodies).

**Why this priority**: This is the Phase 1.5 MUST for Constitution Principle V
(Agent-Aware Response Design). Agents have finite context windows, and this tool
actively helps them manage it rather than dumping fixed-size results.

**Independent Test**: Index a fixture repository, call `get_code_context` with
`max_tokens: 500` and `strategy: "breadth"`, verify that `estimated_tokens` in
the response does not exceed 500.

**Acceptance Scenarios**:

1. **Given** `max_tokens: 500` and `strategy: "breadth"`, **When**
   `get_code_context` is called with a query matching 20 symbols, **Then** the
   response contains multiple symbols with signature-level detail, and
   `estimated_tokens <= 500`.
2. **Given** `max_tokens: 2000` and `strategy: "depth"`, **When**
   `get_code_context` is called, **Then** fewer symbols are returned but each
   includes its full body text, and `estimated_tokens <= 2000`.
3. **Given** `max_tokens: 100` with large results, **When** the tool is called,
   **Then** `truncated: true` is set in metadata and as many results as possible
   are included within the budget.
4. **Given** a query that matches no symbols, **When** the tool is called,
   **Then** `context_items` is empty and `estimated_tokens: 0`.
5. **Given** no `max_tokens` parameter, **When** the tool is called, **Then**
   the default budget of 4000 tokens is used.
6. **Given** the `breadth` strategy, **When** results are serialized, **Then**
   each item includes `name`, `kind`, `path`, `line_start`, `signature` but
   NOT the full body.
7. **Given** the `depth` strategy, **When** results are serialized, **Then**
   each item includes the full `body` text in addition to metadata fields.
8. **Given** the response metadata, **Then** `estimated_tokens` is computed as
   `ceil(whitespace_split_word_count * 1.3)`.

---

### Edge Cases

- What happens when import resolution cannot find the target symbol in the index?
  The edge is stored with a `to_symbol_id` derived from the qualified name. The
  edge exists for graph traversal but the target may not resolve to a
  `symbol_relations` row (external dependency). `find_related_symbols` skips
  unresolved edges.
- What happens when a file has circular imports?
  Edges are recorded in both directions. Graph traversal tools must implement
  cycle detection (visited set) to avoid infinite loops.
- What happens when `get_code_context` has a very small `max_tokens` (e.g., 10)?
  If no single result fits within the budget, `context_items` is empty,
  `truncated: true`, and a `suggestion` field recommends increasing the budget.
- What happens when `get_symbol_hierarchy` is called with `ref` that has no data?
  Returns `symbol_not_found` error with `ref` in the error metadata.
- What happens when `symbol_edges` has stale edges after partial re-indexing?
  Edges are replaced per-file during indexing (all edges with matching
  `from_symbol_id` for the re-indexed file are deleted and re-created).
- Why do these tools not expose `compact` in this phase?
  `003` tools are token-budget-driven by design (`max_tokens` + strategy shaping),
  while `compact` remains focused on `search_code`/`locate_symbol` from `002`.
  Future phases may add `compact` here if benchmark evidence shows agent benefit.

## Requirements

### Functional Requirements

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
  (`codecompass_protocol_version`, `freshness_status`, `indexing_status`,
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

### Key Entities

- **SymbolEdge**: A directed relationship between two symbols (import, call, etc.)
  stored in the `symbol_edges` table with source/target `symbol_stable_id`,
  edge type, and confidence level.
- **SymbolHierarchy**: An ordered chain of symbols from leaf to root (ancestors)
  or root to leaves (descendants), derived from `parent_symbol_id` traversal.
- **CodeContext**: A token budget-fitted collection of code symbols and/or bodies,
  assembled according to a breadth or depth strategy, with estimated token count.

## Success Criteria

### Measurable Outcomes

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
