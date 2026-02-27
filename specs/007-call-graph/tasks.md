# Tasks: Call Graph Analysis

**Input**: Design documents from `/specs/007-call-graph/`
**Prerequisites**: plan.md (required), spec.md (required), contracts/mcp-tools.md (required)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US4)
- Include exact file paths in descriptions

## Phase 1: Data Layer (Call Edge Storage)

**Purpose**: CRUD operations for call edges in `symbol_edges` table

- [x] T325 [US1] Implement call edge CRUD in `crates/cruxe-state/src/edges.rs`: `insert_call_edges(edges: &[CallEdge])`, `get_callers(symbol_id, ref)`, `get_callees(symbol_id, ref)`, `delete_edges_for_file(file_path, ref)` for incremental updates
- [x] T326 [P] [US1] Add `CallEdge` type to `crates/cruxe-core/src/types.rs`: `from_symbol_id`, `to_symbol_id`, `to_name` (for unresolved), `edge_type`, `confidence` (static/heuristic), `source_file`, `source_line`
- [x] T327 [US1] Write unit tests for edge CRUD in `crates/cruxe-state/src/edges.rs`: insert, query by caller, query by callee, delete by file, handle NULL `to_symbol_id`

**Checkpoint**: Call edges can be stored, queried, and deleted in `symbol_edges`

---

## Phase 2: Call Edge Extraction (Tree-Sitter)

**Purpose**: Extract call sites from source files during indexing

- [x] T328 [US1] Implement call site extraction framework in `crates/cruxe-indexer/src/call_extract.rs`: accept parsed AST + symbol index, produce `Vec<CallEdge>`, dispatch to per-language extractors
- [x] T329 [P] [US1] Implement Rust call site patterns in `crates/cruxe-indexer/src/languages/rust.rs`: match `call_expression`, `method_call_expression` nodes, extract callee name, determine confidence based on receiver type availability
- [x] T330 [P] [US1] Implement TypeScript call site patterns in `crates/cruxe-indexer/src/languages/typescript.rs`: match `call_expression`, `new_expression`, `method_call` nodes
- [x] T331 [P] [US1] Implement Python call site patterns in `crates/cruxe-indexer/src/languages/python.rs`: match `call` nodes, `attribute` + `call` for method calls
- [x] T332 [P] [US1] Implement Go call site patterns in `crates/cruxe-indexer/src/languages/go.rs`: match `call_expression`, `selector_expression` + `call_expression` for method calls
- [x] T333 [US1] Implement cross-file callee resolution in `crates/cruxe-indexer/src/call_extract.rs`: match callee name against qualified names in the symbol index for the same ref, set `to_symbol_id` when matched, `NULL` when unresolved
- [x] T334 [US1] Integrate call extraction into indexing pipeline in `crates/cruxe-indexer/src/writer.rs`: after symbol extraction pass, run call extraction, batch-write call edges to `symbol_edges`
- [x] T335 [US1] Update incremental sync in `crates/cruxe-indexer/src/writer.rs`: when a file changes, delete old call edges for that file before re-extracting
- [x] T336 [P] [US1] Create call graph fixture repo in `testdata/fixtures/call-graph-sample/`: 8-10 files across Rust, TypeScript, Python, Go with known direct calls, method calls, cross-file calls, recursive calls, and unresolvable external calls
- [x] T337 [US1] Write integration test: index `testdata/fixtures/call-graph-sample/`, query `symbol_edges` for `edge_type = 'calls'`, verify all expected edges exist with correct confidence levels

**Checkpoint**: Indexing produces call edges for all four languages, cross-file resolution works

---

## Phase 3: Call Graph Query (US2)

**Purpose**: Graph traversal logic for `get_call_graph` tool

- [x] T338 [US2] Implement call graph traversal in `crates/cruxe-query/src/call_graph.rs`: BFS from a given symbol, configurable direction (callers/callees/both), depth (1-5, cap enforced), limit, ref-scoped
- [x] T339 [US2] Implement cycle detection in graph traversal in `crates/cruxe-query/src/call_graph.rs`: track visited symbol IDs to handle recursive calls without infinite loops
- [x] T340 [US2] Implement `get_call_graph` MCP tool handler in `crates/cruxe-mcp/src/tools/get_call_graph.rs`: parse input, delegate to `call_graph.rs`, format response with Protocol v1 metadata and stable symbol handles
- [x] T341 [US2] Register `get_call_graph` in MCP tool list in `crates/cruxe-mcp/src/server.rs`
- [x] T342 [US2] Write integration test: index fixture repo, call `get_call_graph` at depth 1, verify callers and callees match expected values
- [x] T343 [P] [US2] Write integration test: call `get_call_graph` at depth 2, verify transitive edges are included
- [x] T344 [P] [US2] Write unit test: verify depth cap at 5, verify cycle detection with recursive function

**Checkpoint**: `get_call_graph` returns correct caller/callee graphs with depth control

---

## Phase 4: Symbol Comparison (US3)

**Purpose**: Cross-ref symbol comparison tool

- [x] T345 [US3] Implement symbol comparison logic in `crates/cruxe-query/src/symbol_compare.rs`: load symbol from both refs via Tantivy `symbols` index, compare signature, line range, compute diff summary
- [x] T346 [US3] Implement body diff retrieval in `crates/cruxe-query/src/symbol_compare.rs`: if symbol body is available in snippets index, compute a line-level diff summary (added/removed/changed line counts)
- [x] T347 [US3] Implement `compare_symbol_between_commits` MCP tool handler in `crates/cruxe-mcp/src/tools/compare_symbol.rs`: parse input, delegate to `symbol_compare.rs`, format response with Protocol v1 metadata, stable handles, and compatibility metadata
- [x] T348 [US3] Register `compare_symbol_between_commits` in MCP tool list in `crates/cruxe-mcp/src/server.rs`
- [x] T349 [US3] Write integration test: index fixture repo at two different refs with a changed function, call `compare_symbol_between_commits`, verify diff summary is accurate
- [x] T350 [P] [US3] Write unit test: verify handling of added symbol (base=null), deleted symbol (head=null), unchanged symbol

**Checkpoint**: `compare_symbol_between_commits` shows accurate diffs across refs

---

## Phase 5: Follow-up Query Suggestions (US4)

**Purpose**: Agent guidance tool for low-confidence results

- [x] T351 [US4] Implement follow-up suggestion engine in `crates/cruxe-query/src/followup.rs`: analyze previous query type and results, extract identifiers from natural language queries, generate tool call suggestions with parameters
- [x] T352 [US4] Implement suggestion rules in `crates/cruxe-query/src/followup.rs`: low-confidence `search_code` -> suggest `locate_symbol`; zero-result `locate_symbol` -> suggest `search_code` or `get_call_graph`; `natural_language` intent -> suggest `symbol` intent reformulation
- [x] T353 [US4] Implement `suggest_followup_queries` MCP tool handler in `crates/cruxe-mcp/src/tools/suggest_followup.rs`: parse input, delegate to `followup.rs`, format response with Protocol v1 metadata and canonical error envelope
- [x] T354 [US4] Register `suggest_followup_queries` in MCP tool list in `crates/cruxe-mcp/src/server.rs`
- [x] T355 [US4] Write unit test: verify suggestion rules for each scenario (low confidence, zero results, above threshold)
- [x] T356 [US4] Write integration test: index fixture repo, perform a low-confidence search, call `suggest_followup_queries`, verify suggestions are actionable

**Checkpoint**: `suggest_followup_queries` provides actionable guidance for agents

---

## Phase 6: Polish & Validation

**Purpose**: End-to-end validation, performance benchmarking, documentation

- [x] T357 [P] Update MCP `tools/list` response to include all three new tools with correct schemas, and validate errors for all three tools against `specs/meta/protocol-error-codes.md` with machine-stable `error.code` values
- [x] T358 Run full test suite (`cargo test --workspace`) and fix any failures
- [x] T359 [P] Benchmark call edge extraction overhead: index a 5,000-file repo with and without call extraction, verify overhead < 20%
- [x] T360 [P] Benchmark `get_call_graph` query latency: measure p95 for depth 1 and depth 2 on a 10,000-symbol index
- [x] T361 [P] Create golden output files in `testdata/golden/` for call graph queries at depth 1 and depth 2
- [x] T362 Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [x] T363 Run `cargo fmt --check --all` and fix formatting

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1** (Data Layer): No new dependencies - uses existing `symbol_edges` table schema
- **Phase 2** (Extraction): Depends on Phase 1 (edge CRUD available)
- **Phase 3** (Call Graph Query): Depends on Phase 2 (edges exist to traverse)
- **Phase 4** (Symbol Comparison): Independent of Phases 2-3 (uses existing symbol index)
- **Phase 5** (Follow-up Suggestions): Depends on Phase 3 (references call graph in suggestions)
- **Phase 6** (Polish): Depends on all phases

### Parallel Opportunities

- Phase 1: T325 and T326 can run in parallel
- Phase 2: All four language extractors (T329-T332) and fixture repo (T336) can run in parallel
- Phase 3: T343 and T344 can run in parallel
- Phase 4: Can run in parallel with Phases 2-3 (independent data path)
- Phase 5: T355 and T356 can run in parallel after T351-T354
- Phase 6: T357, T359, T360, T361 can run in parallel

## Implementation Strategy

### Incremental Delivery

1. Phase 1 -> Edge storage works
2. Phase 2 -> Indexing produces call edges (core data pipeline)
3. Phase 3 -> `get_call_graph` works (primary user value)
4. Phase 4 -> `compare_symbol_between_commits` works (can proceed in parallel with Phase 3)
5. Phase 5 -> `suggest_followup_queries` works (agent guidance)
6. Phase 6 -> Validation and performance verification

## Notes

- Total: 39 tasks, 6 phases
- Phase 4 (Symbol Comparison) can be developed in parallel with Phases 2-3
- Call extraction is a second pass over the AST, running after symbol extraction
- Depth cap of 5 prevents runaway graph traversal in large codebases
