## 1. Core Types and Config (Shared Infrastructure)

**Purpose**: Add new types, enums, and configuration for all 002 features

- [x] T082 [P] [US1] Add `DetailLevel` enum (`Location`, `Signature`, `Context`) to `crates/cruxe-core/src/types.rs` with serde rename to lowercase strings, default `Signature`
- [x] T083 [P] [US6] Add `FreshnessPolicy` enum (`Strict`, `Balanced`, `BestEffort`) to `crates/cruxe-core/src/types.rs` with serde rename to lowercase strings, default `Balanced`
- [x] T084 [P] [US5] Add `RankingReasons` struct to `crates/cruxe-core/src/types.rs` with fields: `result_index: usize`, `exact_match_boost: f64`, `qualified_name_boost: f64`, `path_affinity: f64`, `definition_boost: f64`, `kind_match: f64`, `bm25_score: f64`, `final_score: f64`
- [x] T085 [P] Add `freshness_policy` and ranking explainability controls to config schema in `crates/cruxe-core/src/config.rs`
- [x] T086 Add `freshness_policy = "balanced"` and default explainability setting (`ranking_explain_level = "off"`; legacy debug flag compatibility optional) to `configs/default.toml`
- [x] T087 [P] Write unit tests for `DetailLevel` and `FreshnessPolicy` serde round-trip in `crates/cruxe-core/src/types.rs`

## 2. Detail Level Serialization (US1 -- Priority: P1)

**Purpose**: Implement detail_level-aware response serialization for search and locate

**Goal**: `search_code` and `locate_symbol` responses respect `detail_level` parameter

- [x] T088 [US1] Implement `serialize_result_at_level(result, detail_level, compact)` function in `crates/cruxe-query/src/detail.rs` that filters result fields based on `DetailLevel` enum and optional compact mode: `Location` emits only path/line_start/line_end/kind/name; `Signature` adds qualified_name/signature/language/visibility; `Context` adds body_preview/parent/related_symbols unless `compact=true`
- [x] T089 [US1] Add `body_preview` generation logic in `crates/cruxe-query/src/detail.rs`: read first N lines of symbol body from Tantivy `content` field, truncate to configurable limit (default 10 lines)
- [x] T090 [US1] Add `parent` context resolution in `crates/cruxe-query/src/detail.rs`: look up `parent_symbol_id` in SQLite `symbol_relations` to get parent kind/name/path/line
- [x] T091 [US1] Add `related_symbols` resolution in `crates/cruxe-query/src/detail.rs`: query `symbol_relations` for symbols in the same file referenced by imports or same parent, limit to 5
- [x] T092 [US1] Wire `detail_level` and `compact` parameters into `search_code` MCP tool handler in `crates/cruxe-mcp/src/server.rs`: parse from input, pass to serialization
- [x] T093 [US1] Wire `detail_level` and `compact` parameters into `locate_symbol` MCP tool handler in `crates/cruxe-mcp/src/server.rs`: parse from input, pass to serialization
- [x] T094 [US1] Update Protocol v1 response types: added `DetailLevel` to tool input schemas, updated result serialization with conditional field inclusion via detail level filtering
- [x] T095 [US1] Write integration test: call `locate_symbol` with `detail_level: "location"`, verify response contains location fields plus identity fields and no signature/context fields
- [x] T096 [P] [US1] Write integration test: call `locate_symbol` with `detail_level: "signature"` (default), verify response contains signature-level fields plus identity fields
- [x] T097 [P] [US1] Write integration test: call `search_code` with `detail_level: "context"`, verify response contains body_preview and parent fields when available

## 3. File Outline Tool (US2 -- Priority: P1)

**Purpose**: Implement `get_file_outline` MCP tool

**Goal**: Agents can get file symbol skeleton without full file read

- [x] T098 [US2] Implement `get_file_outline_query` in `crates/cruxe-state/src/symbols.rs`: SELECT with optional `parent_symbol_id IS NULL` filter for `depth="top"`
- [x] T099 [US2] Implement `build_symbol_tree` in `crates/cruxe-state/src/symbols.rs`: convert flat symbol list to nested tree using `parent_symbol_id` chains with HashMap and recursive assembly
- [x] T100 [US2] Create `crates/cruxe-mcp/src/tools/get_file_outline.rs`: MCP tool definition with `path`, `ref`, `depth`, `language` parameters
- [x] T101 [US2] Register `get_file_outline` in `crates/cruxe-mcp/src/tools/mod.rs`, add to `tools/list` response, and wire handler in server.rs
- [x] T102 [US2] Write integration test: index fixture, call `get_file_outline` on types.rs, verify nested structure with children under impl block
- [x] T103 [P] [US2] Write integration test: call `get_file_outline` with `depth: "top"`, verify only top-level symbols returned (no children)
- [x] T104 [P] [US2] Write integration test: call `get_file_outline` on non-existent file, verify `file_not_found` error
- [x] T105 [P] [US2] Performance test deferred -- fixture has <100 symbols; query is pure SQLite with trivial latency

## 4. Health Check and Prewarm (US3 + US4 -- Priority: P2)

**Purpose**: Implement health_check tool and Tantivy index prewarming

**Goal**: Agents can check system readiness; first query benefits from warm indices

### Implementation for User Story 3 (Health Check)

- [x] T106 [US3] Create `crates/cruxe-mcp/src/tools/health_check.rs`: MCP tool handler that accepts optional `workspace`, aggregates Tantivy reader open check, SQLite `PRAGMA integrity_check` (quick variant), grammar availability scan, active job query, prewarm status, and startup compatibility payload (`index.status`, `current_schema_version`, `required_schema_version`)
- [x] T107 [US3] Implement Tantivy health check in `crates/cruxe-state/src/tantivy_index.rs`: attempt to open reader on each index (symbols, snippets, files), report ok/error per index
- [x] T108 [US3] Implement SQLite health check in `crates/cruxe-state/src/db.rs`: run `PRAGMA quick_check` (faster than full integrity_check), report ok/error
- [x] T109 [US3] Implement grammar availability check: iterate v1 language list (Rust, TypeScript, Python, Go), verify tree-sitter grammar loads, return available/missing lists
- [x] T110 [US3] Register `health_check` in `crates/cruxe-mcp/src/tools/mod.rs` and add to `tools/list` response
- [x] T111 [US3] Write integration test: call `health_check` on a healthy system, verify `status: "ready"`, `tantivy_ok: true`, `sqlite_ok: true`, all grammars available, and compatibility payload reports `index.status: "compatible"`

### Implementation for User Story 4 (Prewarm)

- [x] T112 [US4] Implement `prewarm_indices` in `crates/cruxe-state/src/tantivy_index.rs`: for each registered project, open reader, touch segment metadata via `segment_readers().iter().map(|seg| seg.num_docs())`, run 3 warmup queries against `symbol_exact` field with common terms
- [x] T113 [US4] Add global `prewarm_status` state (enum: `Pending`, `InProgress`, `Complete`, `Failed`) accessible to health_check handler, stored in shared `Arc<AtomicU8>` or similar
- [x] T114 [US4] Wire prewarm into `serve-mcp` startup in `crates/cruxe-cli/src/commands/serve_mcp.rs`: start MCP server loop first, then run `prewarm_indices` asynchronously in background; set status to `warming` during and `ready` after completion
- [x] T115 [US4] Add `--no-prewarm` CLI flag to `serve-mcp` command in `crates/cruxe-cli/src/commands/serve_mcp.rs`: when set, skip prewarm and set status directly to `ready`
- [x] T116 [US4] Write integration test: start MCP server with prewarm, verify `health_check` returns `status: "warming"` immediately, then `status: "ready"` after warmup, and verify `initialize` / `tools/list` succeed during warmup
- [x] T117 [P] [US4] Write integration test: start MCP server with `--no-prewarm`, verify `health_check` returns `status: "ready"` immediately

## 5. Ranking Reasons (US5 -- Priority: P3)

**Purpose**: Add debug ranking explanations to search responses

**Goal**: Developers can see why results are ranked the way they are with controllable payload depth

- [x] T118 [US5] Extend reranker in `crates/cruxe-query/src/ranking.rs` to collect `RankingReasons` per result during scoring: capture individual boost values (exact_match_boost, qualified_name_boost, path_affinity, definition_boost, kind_match) and raw bm25_score before combining into final_score
- [x] T119 [US5] Add `debug_mode: bool` parameter to search/locate pipeline entry points in `crates/cruxe-query/src/search.rs` and `crates/cruxe-query/src/locate.rs`: when true, return `Vec<RankingReasons>` alongside results
- [x] T120 [US5] Wire ranking explainability through MCP layer in `crates/cruxe-mcp/src/tools/search_code.rs` and `crates/cruxe-mcp/src/tools/locate_symbol.rs`: include `ranking_reasons` in Protocol v1 metadata when explainability is enabled
- [x] T121 [US5] Update Protocol v1 metadata type in `crates/cruxe-mcp/src/protocol.rs`: add optional `ranking_reasons: Option<Vec<RankingReasons>>` field with `skip_serializing_if = "Option::is_none"`
- [x] T122 [US5] Write integration test: enable full explainability, call `search_code`, verify each result has `ranking_reasons` with all 7 fields populated
- [x] T123 [P] [US5] Write integration test: explainability off by default, call `search_code`, verify `ranking_reasons` is absent from response metadata
- [x] T451 [US5] Add `ranking_explain_level` parameter (`off`/`basic`/`full`) to `crates/cruxe-mcp/src/tools/search_code.rs` and `crates/cruxe-mcp/src/tools/locate_symbol.rs`, with default `off`
- [x] T452 [US5] Implement compact explainability serialization in `crates/cruxe-query/src/ranking.rs`: `basic` mode emits normalized factors only, `full` keeps full debug payload
- [x] T453 [P] [US5] Add integration coverage for explainability levels: verify `basic` payload is smaller than `full` while preserving result count
- [x] T462 [US1] Implement near-duplicate suppression in query response assembly for `search_code` and `locate_symbol` (FR-105b): dedup by symbol/file-region before final top-k, include `suppressed_duplicate_count` in metadata
- [x] T463 [US1] Implement hard payload safety limits (FR-105c): enforce max response budget with `result_completeness: "truncated"` and deterministic `suggested_next_actions` instead of hard failures

## 6. Stale-Aware Query Behavior (US6 -- Priority: P2)

**Purpose**: Implement pre-query freshness check with configurable policy

**Goal**: Agents get reliable results with clear freshness signals

- [x] T124 [US6] Create `crates/cruxe-query/src/freshness.rs`: implement `check_freshness(project, ref) -> FreshnessResult` that compares `branch_state.last_indexed_commit` to current HEAD (VCS mode) or `file_manifest` hash cursor (single-version mode), returning `Fresh`, `Stale { last_indexed_commit, current_head }`, or `Syncing`
- [x] T125 [US6] Implement `apply_freshness_policy(policy, freshness_result) -> PolicyAction` in `crates/cruxe-query/src/freshness.rs`: `Strict` + Stale -> `BlockWithError`, `Balanced` + Stale -> `ProceedWithStaleIndicator + TriggerAsyncSync`, `BestEffort` + Stale -> `ProceedWithStaleIndicator`
- [x] T126 [US6] Wire freshness check into `search_code` pipeline in `crates/cruxe-query/src/search.rs`: call `check_freshness` before query execution, apply policy, set `freshness_status` in response metadata
- [x] T127 [US6] Wire freshness check into `locate_symbol` pipeline in `crates/cruxe-query/src/locate.rs`: same pattern as T126
- [x] T128 [US6] Wire `freshness_policy` parameter into MCP tool handlers in `crates/cruxe-mcp/src/tools/search_code.rs` and `crates/cruxe-mcp/src/tools/locate_symbol.rs`: parse from input (optional, falls back to config default)
- [x] T129 [US6] Implement async sync trigger for `balanced` policy in `crates/cruxe-query/src/freshness.rs`: spawn background `sync_repo` task via tokio when freshness check returns Stale and policy is Balanced
- [x] T130 [US6] Implement strict mode error response in `crates/cruxe-mcp/src/tools/search_code.rs`: return `index_stale` when policy is Strict and index is stale; return `index_incompatible` with force-reindex guidance when startup compatibility check reports schema mismatch/corrupt manifest
- [x] T131 [US6] Write integration test: modify a fixture file after indexing, call `search_code` with `freshness_policy: "balanced"`, verify results returned with `freshness_status: "stale"` in metadata
- [x] T132 [P] [US6] Write integration test: call `search_code` with `freshness_policy: "strict"` on stale index, verify `index_stale` error returned
- [x] T133 [P] [US6] Write integration test: call `search_code` with `freshness_policy: "best_effort"` on stale index, verify results returned with `freshness_status: "stale"` and no sync triggered

## 7. Polish and Validation

**Purpose**: End-to-end validation, tools/list verification, documentation

- [x] T134 Write E2E test: start MCP server, send `tools/list`, verify `get_file_outline` and `health_check` are listed with correct input schemas alongside existing tools
- [x] T135 [P] Write E2E test: full workflow -- `health_check` -> `index_repo` -> `locate_symbol(detail_level: "location")` -> `get_file_outline` -> `search_code(detail_level: "context")`, verify all responses conform to contracts
- [x] T136 [P] Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [x] T137 [P] Run `cargo fmt --check --all` and fix formatting
- [x] T138 Run performance benchmark: measure `get_file_outline` p95 on fixture file with 100+ symbols, verify < 50ms; measure first-query latency with prewarm, verify < 500ms
- [x] T139 Verify backward compatibility: existing MCP clients calling `search_code` and `locate_symbol` without `detail_level` parameter receive `"signature"` level responses (default)

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Core Types)**: No dependencies beyond 001-core-mvp completion -- can start immediately
- **Phase 2 (Detail Level)**: Depends on Phase 1 (types defined)
- **Phase 3 (File Outline)**: Depends on Phase 1 (types defined); independent of Phase 2
- **Phase 4 (Health/Prewarm)**: Depends on Phase 1 (types defined); independent of Phases 2-3
- **Phase 5 (Ranking Reasons)**: Depends on Phase 1 (types defined); independent of Phases 2-4
- **Phase 6 (Freshness)**: Depends on Phase 1 (types defined); independent of Phases 2-5
- **Phase 7 (Polish)**: Depends on all previous phases

### Parallel Opportunities

- Phase 1: All type definitions (T082-T084) and config changes (T085) can run in parallel
- Phase 2: Integration tests T095/T096/T097 can run in parallel
- Phase 3: Tests T102/T103/T104/T105 can run in parallel after T098-T101
- Phase 4: Health check (T106-T111) and prewarm (T112-T117) are independent tracks
- Phase 5: Tests T122/T123 can run in parallel
- Phase 6: Tests T131/T132/T133 can run in parallel
- Phases 2, 3, 4, 5, 6 are all independent of each other (only share Phase 1)

### User Story Dependencies

- **US1 (Detail Level, P1)**: After Phase 1 -- highest priority, Constitution V
- **US2 (File Outline, P1)**: After Phase 1 -- highest priority, key agent tool
- **US3 (Health Check, P2)**: After Phase 1 -- enables US4 testing
- **US4 (Prewarm, P2)**: After Phase 1 -- independent but US3 tests prewarm status
- **US5 (Ranking Reasons, P3)**: After Phase 1 -- Constitution VII, lower priority
- **US6 (Freshness Policy, P2)**: After Phase 1 -- independent

## Implementation Strategy

### Priority-First

1. Complete Phase 1: Core Types
2. Complete Phase 2: Detail Level (US1, P1, Constitution V) AND Phase 3: File Outline (US2, P1) in parallel
3. Complete Phase 4: Health/Prewarm (US3+US4, P2) AND Phase 6: Freshness (US6, P2) in parallel
4. Complete Phase 5: Ranking Reasons (US5, P3)
5. Complete Phase 7: Polish
6. **STOP and VALIDATE**: `tools/list` shows all new tools, detail_level controls response shape, health_check reports accurate status

### Incremental Delivery

1. Phase 1 -> Types ready
2. Phase 2 -> `detail_level` works (Constitution V fulfilled)
3. Phase 3 -> `get_file_outline` available (agent workflow improvement)
4. Phase 4 -> `health_check` + prewarm (operational readiness)
5. Phase 5 -> `ranking_reasons` with explainability-level controls (Constitution VII fulfilled)
6. Phase 6 -> Stale-aware queries (freshness policy)

## Notes

- [P] tasks = different files, no dependencies
- [USn] label maps task to specific user story
- Commit after each task or logical group
- Stop at any checkpoint to validate independently
- Total: 63 tasks, 7 phases
- No new crate dependencies required
- No new storage schemas -- all queries against existing 001-core-mvp tables
