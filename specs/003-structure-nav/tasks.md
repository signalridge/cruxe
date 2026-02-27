# Tasks: Symbol Structure & Navigation

**Input**: Design documents from `/specs/003-structure-nav/`
**Prerequisites**: plan.md (required), spec.md (required), contracts/mcp-tools.md
**Depends On**: 002-agent-protocol (Phase 1.1) must be complete

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US4)
- Include exact file paths in descriptions

## Phase 1: Foundation (Edge Storage + Token Estimation)

**Purpose**: CRUD operations for `symbol_edges`, token estimation utility

- [x] T140 [US1] Implement `symbol_edges` CRUD in `crates/cruxe-state/src/edges.rs`: `insert_edges(repo, ref, edges: Vec<SymbolEdge>)`, `delete_edges_for_file(repo, ref, from_symbol_ids: Vec<&str>)`, `get_edges_from(repo, ref, from_symbol_id)`, `get_edges_to(repo, ref, to_symbol_id)`, `get_edges_by_type(repo, ref, edge_type)`
- [x] T141 [P] [US4] Implement token estimation utility in `crates/cruxe-core/src/tokens.rs`: `estimate_tokens(text: &str) -> usize` using whitespace-split word count * 1.3, with `ceil` rounding
- [x] T142 [P] [US1] Add `SymbolEdge` type to `crates/cruxe-core/src/types.rs`: `repo`, `ref_name`, `from_symbol_id`, `to_symbol_id`, `edge_type`, `confidence`
- [x] T143 [US1] Write unit tests for `edges.rs`: insert, delete per file, query by from/to/type, verify atomic replacement
- [x] T144 [P] [US4] Write unit tests for `tokens.rs`: verify estimation for empty string, single word, code snippet with identifiers, large text block

**Checkpoint**: Edge storage and token estimation utilities functional

---

## Phase 2: Import Extraction (Per-Language Parsers)

**Purpose**: Extract import/use/require statements from tree-sitter parse trees

- [x] T145 [US1] Implement import extraction dispatcher in `crates/cruxe-indexer/src/import_extract.rs`: dispatch to per-language extractor, return `Vec<RawImport>` where `RawImport` = `{ source_qualified_name, target_qualified_name, target_name, import_line }`
- [x] T146 [P] [US1] Implement Rust import extraction in `crates/cruxe-indexer/src/languages/rust.rs`: parse `use` items (simple, nested `use a::{b, c}`, glob `use a::*`, rename `use a as b`), extract target qualified names
- [x] T147 [P] [US1] Implement TypeScript import extraction in `crates/cruxe-indexer/src/languages/typescript.rs`: parse `import { Name } from './path'`, `import Name from './path'`, `import * as Name from './path'`, `require('./path')`, extract target qualified names with relative path resolution
- [x] T148 [P] [US1] Implement Python import extraction in `crates/cruxe-indexer/src/languages/python.rs`: parse `import module`, `from module import name`, `from module import name as alias`, `from . import name` (relative imports), extract target qualified names
- [x] T149 [P] [US1] Implement Go import extraction in `crates/cruxe-indexer/src/languages/go.rs`: parse `import "path"`, `import ( "path1"; "path2" )`, `import alias "path"`, extract package paths as target qualified names
- [x] T150 [US1] Implement cross-file resolution in `crates/cruxe-indexer/src/import_extract.rs`: `resolve_imports(raw_imports: Vec<RawImport>, repo, ref) -> Vec<SymbolEdge>` — query `symbol_relations` by qualified name to resolve `to_symbol_id`; for unresolved targets, derive synthetic `symbol_stable_id` from `blake3("unresolved:" + qualified_name)`
- [x] T151 [P] [US1] Write unit tests for Rust import extraction: verify `use crate::auth::Claims` -> qualified name `auth::Claims`, nested `use a::{b, c}` -> two imports, glob `use a::*` -> single wildcard import
- [x] T152 [P] [US1] Write unit tests for TypeScript import extraction: named import, default import, namespace import, require
- [x] T153 [P] [US1] Write unit tests for Python import extraction: absolute import, from import, relative import, alias import
- [x] T154 [P] [US1] Write unit tests for Go import extraction: single import, grouped import, aliased import

**Checkpoint**: All four languages extract import statements from tree-sitter trees

---

## Phase 3: Index Pipeline Integration

**Purpose**: Wire import extraction into the existing indexing pipeline

- [x] T155 [US1] Integrate import extraction into `crates/cruxe-indexer/src/writer.rs`: after writing symbol records for a file, run import extraction on the same parse tree, resolve imports, write edges to SQLite via `edges.rs`
- [x] T156 [US1] Implement per-file edge replacement in `crates/cruxe-indexer/src/writer.rs`: before inserting new edges for a file, delete all existing `imports` edges where `from_symbol_id` belongs to the current file's symbols
- [x] T157 [US1] Write integration test: index `testdata/fixtures/rust-sample/` (must have cross-file imports), verify `symbol_edges` table contains expected import edges with correct `from_symbol_id` and `to_symbol_id`
- [x] T158 [P] [US1] Write integration test: re-index after modifying imports in a fixture file, verify old edges are replaced with new edges (no stale edges remain)
- [x] T159 [P] [US1] Extend fixture repos with cross-file import patterns: add files to `testdata/fixtures/rust-sample/` with `use` statements referencing other fixture files, similarly for TS/Python/Go

**Checkpoint**: `cruxe index` populates `symbol_edges` with import relationships

---

## Phase 4: get_symbol_hierarchy Tool

**Purpose**: Parent chain traversal via `parent_symbol_id` in `symbol_relations`

- [x] T160 [US2] Implement `get_symbol_hierarchy` in `crates/cruxe-query/src/hierarchy.rs`: given `(repo, ref, symbol_name, path)`, find the symbol in `symbol_relations`, then traverse `parent_symbol_id` upward (ancestors) or query children downward (descendants), with cycle detection via visited set
- [x] T161 [US2] Implement ancestor traversal in `hierarchy.rs`: recursive query `SELECT * FROM symbol_relations WHERE symbol_id = ? AND repo = ? AND ref = ?` following `parent_symbol_id` until NULL
- [x] T162 [US2] Implement descendant traversal in `hierarchy.rs`: query `SELECT * FROM symbol_relations WHERE parent_symbol_id = ? AND repo = ? AND ref = ?` recursively
- [x] T163 [US2] Implement MCP handler in `crates/cruxe-mcp/src/tools/get_symbol_hierarchy.rs`: parse input, call hierarchy query, format response with Protocol v1 metadata and stable follow-up handles (`symbol_id`, `symbol_stable_id`)
- [x] T164 [US2] Register `get_symbol_hierarchy` in `crates/cruxe-mcp/src/tools/mod.rs`: add to `tools/list` response
- [x] T165 [US2] Write integration test: index fixture repo with nested symbols (method inside impl inside module), call `get_symbol_hierarchy` with `direction: "ancestors"` for the method, verify chain includes impl and module-level entries
- [x] T166 [P] [US2] Write integration test: call `get_symbol_hierarchy` with `direction: "descendants"` for a class/struct, verify all child methods are returned
- [x] T167 [P] [US2] Write unit test: verify cycle detection stops traversal when circular `parent_symbol_id` references exist (defensive test)

**Checkpoint**: `get_symbol_hierarchy` returns correct parent/child chains via MCP

---

## Phase 5: find_related_symbols Tool

**Purpose**: Scope-based symbol discovery using `symbol_relations` + `symbol_edges`

- [x] T168 [US3] Implement `find_related_symbols` in `crates/cruxe-query/src/related.rs`: given `(repo, ref, symbol_name, path, scope, limit)`, find related symbols by scope
- [x] T169 [US3] Implement `scope: "file"` in `related.rs`: query `SELECT * FROM symbol_relations WHERE repo = ? AND ref = ? AND path = ? AND symbol_id != ?` — return all symbols in the same file
- [x] T170 [US3] Implement `scope: "module"` in `related.rs`: derive module path from file path (language-specific: Rust uses `mod` tree, Python uses package directory, TS uses directory, Go uses package), query symbols in sibling files within the same module, union with import-connected symbols from `symbol_edges`
- [x] T171 [US3] Implement `scope: "package"` in `related.rs`: expand scope to parent directory/package, include all symbols under that package prefix
- [x] T172 [US3] Implement result prioritization in `related.rs`: same-file symbols ranked first, then same-module, then import-connected, apply `limit` after ranking
- [x] T173 [US3] Implement MCP handler in `crates/cruxe-mcp/src/tools/find_related_symbols.rs`: parse input, call related query, format response with Protocol v1 metadata and stable follow-up handles (`symbol_id`, `symbol_stable_id`)
- [x] T174 [US3] Register `find_related_symbols` in `crates/cruxe-mcp/src/tools/mod.rs`
- [x] T175 [US3] Write integration test: index fixture repo, call `find_related_symbols` with `scope: "file"`, verify all sibling symbols returned
- [x] T176 [P] [US3] Write integration test: call with `scope: "module"`, verify symbols from sibling files in same module are included
- [x] T177 [P] [US3] Write unit test: verify `limit` parameter correctly truncates results after prioritization

**Checkpoint**: `find_related_symbols` returns scope-appropriate related symbols via MCP

---

## Phase 6: get_code_context Tool

**Purpose**: Token budget-aware context retrieval with breadth/depth strategies

- [x] T178 [US4] Implement `get_code_context` in `crates/cruxe-query/src/context.rs`: given `(query, max_tokens, strategy, ref, language)`, search for relevant symbols, then fit results within token budget
- [x] T179 [US4] Implement breadth strategy in `context.rs`: retrieve top-k symbols via existing `search_code` or `locate_symbol`, serialize each at signature-level detail (name, kind, path, line_start, signature), accumulate estimated tokens, stop when budget exceeded
- [x] T180 [US4] Implement depth strategy in `context.rs`: retrieve top-k symbols, for each symbol read the body text from source file (using file path + line range), serialize with full body, accumulate estimated tokens, stop when budget exceeded
- [x] T181 [US4] Implement token accumulation loop in `context.rs`: for each candidate result, estimate tokens for its serialized form, add to running total, stop adding when next result would exceed `max_tokens`
- [x] T182 [US4] Implement response construction in `context.rs`: build `CodeContextResponse` with `context_items`, `estimated_tokens`, `truncated`, `metadata` including `total_candidates` and `suggestion` when truncated
- [x] T183 [US4] Implement MCP handler in `crates/cruxe-mcp/src/tools/get_code_context.rs`: parse input (default `max_tokens=4000`, default `strategy="breadth"`), call context query, format response with Protocol v1 metadata, stable handles, and deterministic `suggested_next_actions` when truncated/low-confidence
- [x] T184 [US4] Register `get_code_context` in `crates/cruxe-mcp/src/tools/mod.rs`
- [x] T185 [US4] Write integration test: index fixture repo, call `get_code_context` with `max_tokens: 500`, `strategy: "breadth"`, verify `estimated_tokens <= 500` and multiple results returned
- [x] T186 [P] [US4] Write integration test: call with `strategy: "depth"`, verify results include body text and `estimated_tokens <= max_tokens`
- [x] T187 [P] [US4] Write integration test: call with very small `max_tokens: 50`, verify `truncated: true` and graceful response
- [x] T188 [P] [US4] Write unit test: verify token estimation consistency — serialize a known string, estimate tokens, verify `ceil(word_count * 1.3)` matches

**Checkpoint**: `get_code_context` returns budget-fitted results via MCP

---

## Phase 7: Polish & Validation

**Purpose**: End-to-end validation, MCP tools/list update, cross-cutting concerns

- [x] T189 Verify `cruxe serve-mcp` `tools/list` includes all three new tools with correct input schemas
- [x] T190 [P] Write E2E test: start MCP server with indexed fixture repo, call all three new tools via JSON-RPC, verify responses match contract schemas in `contracts/mcp-tools.md`
- [x] T191 [P] Verify all new tool responses include Protocol v1 metadata fields and all tool errors map to `specs/meta/protocol-error-codes.md` with actionable `error.data` payload where applicable
- [x] T192 Run `cargo test --workspace` and fix any failures
- [x] T193 Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [x] T194 Run relevance validation: call `get_symbol_hierarchy` for 10 known symbols in fixture repos, verify >= 95% correct chains
- [x] T195 Run budget validation: call `get_code_context` with 10 different `max_tokens` values, verify `estimated_tokens` never exceeds budget

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1** (Foundation): No dependencies beyond 002-agent-protocol completion
- **Phase 2** (Import Extraction): Depends on Phase 1 (SymbolEdge type, edge CRUD)
- **Phase 3** (Pipeline Integration): Depends on Phase 2 (extractors ready)
- **Phase 4** (Hierarchy): Depends on Phase 1 only (uses symbol_relations, not edges)
- **Phase 5** (Related): Depends on Phase 3 (needs populated edges) + Phase 4 (shares query patterns)
- **Phase 6** (Context): Depends on Phase 1 (token estimation) + existing search from 001-core-mvp
- **Phase 7** (Polish): Depends on all previous phases

### Parallel Opportunities

- Phase 1: Token estimation (T141) and SymbolEdge type (T142) can run in parallel with edge CRUD (T140)
- Phase 2: All four language extractors (T146-T149) can run in parallel; all unit tests (T151-T154) can run in parallel
- Phase 3: T158, T159 can run in parallel
- Phase 4: T166, T167 can run in parallel; Phase 4 can start as soon as Phase 1 completes (does not need Phase 2/3)
- Phase 5: T176, T177 can run in parallel
- Phase 6: T186, T187, T188 can run in parallel; Phase 6 can start as soon as Phase 1 completes
- Phase 7: T190, T191 can run in parallel

### Critical Path

Phase 1 -> Phase 2 -> Phase 3 -> Phase 5 -> Phase 7

Phase 4 and Phase 6 are off the critical path and can proceed in parallel with Phase 2/3.

## Implementation Strategy

### Incremental Delivery

1. Phase 1: Edge storage + token utility ready
2. Phase 2: Import extractors for all 4 languages
3. Phase 3: Index pipeline produces edges (first demo: `SELECT * FROM symbol_edges`)
4. Phase 4: `get_symbol_hierarchy` works via MCP (independent of edges)
5. Phase 5: `find_related_symbols` uses both relations and edges
6. Phase 6: `get_code_context` with budget control (core agent value)
7. Phase 7: Full validation

### MVP First (Phase 4 + Phase 6)

If time-constrained, Phase 4 (`get_symbol_hierarchy`) and Phase 6 (`get_code_context`)
deliver the highest agent value independently of import edge population. They can
ship with `find_related_symbols` limited to file scope only.

## Notes

- [P] tasks = different files, no dependencies
- [USn] label maps task to specific user story
- Commit after each task or logical group
- Stop at any checkpoint to validate independently
- Total: 56 tasks, 7 phases
- `calls` edges deferred to Phase 2.5 (spec 007-call-graph)
- `heuristic` confidence not used in this phase
