# Tasks: Cruxe Core MVP

**Input**: Design documents from `/specs/001-core-mvp/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/mcp-tools.md

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US5)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Rust workspace skeleton, build configuration, project scaffolding

- [X] T001 Create workspace `Cargo.toml` with 6 member crates: `cruxe-cli`, `cruxe-core`, `cruxe-state`, `cruxe-indexer`, `cruxe-query`, `cruxe-mcp`
- [X] T002 [P] Create `crates/cruxe-core/Cargo.toml` with dependencies: `serde`, `serde_json`, `thiserror`, `tracing`, `blake3`
- [X] T003 [P] Create `crates/cruxe-cli/Cargo.toml` with dependencies: `clap`, `tokio`, `tracing-subscriber`, `anyhow`
- [X] T004 [P] Create `crates/cruxe-state/Cargo.toml` with dependencies: `rusqlite` (bundled), `tantivy`, `serde`
- [X] T005 [P] Create `crates/cruxe-indexer/Cargo.toml` with dependencies: `tree-sitter`, language grammars, `ignore`, `blake3`
- [X] T006 [P] Create `crates/cruxe-query/Cargo.toml` with dependencies: `tantivy`, `serde`
- [X] T007 [P] Create `crates/cruxe-mcp/Cargo.toml` with dependencies: `tokio`, `serde_json`
- [X] T008 Create `.gitignore` with Rust patterns: `target/`, `*.rs.bk`, `.idea/`, `.env*`, `*.log`
- [X] T009 [P] Create `configs/default.toml` with default configuration values
- [X] T010 [P] Create empty `testdata/fixtures/` directory structure with `rust-sample/`, `ts-sample/`, `python-sample/`, `go-sample/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types, error handling, config loading, storage engines — MUST be complete before any user story

- [X] T011 Implement error type hierarchy in `crates/cruxe-core/src/error.rs`: `ConfigError`, `StateError`, `IndexError`, `ParseError`, `QueryError`, `McpError`, `VcsError`
- [X] T012 [P] Implement shared types in `crates/cruxe-core/src/types.rs`: `Project`, `SymbolKind`, `SymbolRecord`, `SnippetRecord`, `FileRecord`, `DetailLevel`, `QueryIntent`, `RefScope`
- [X] T013 [P] Implement constants in `crates/cruxe-core/src/constants.rs`: `REF_LIVE`, `DEFAULT_LIMIT`, `SCHEMA_VERSION`, `PARSER_VERSION`, `MAX_FILE_SIZE`, `DEFAULT_DATA_DIR`
- [X] T014 [P] Implement config loader in `crates/cruxe-core/src/config.rs`: TOML parsing, three-layer precedence (CLI > project > global > defaults), `CRUXE_` env prefix
- [X] T015 Implement SQLite connection manager in `crates/cruxe-state/src/db.rs`: connection pool, WAL mode, NORMAL sync, 64MB cache, foreign keys, 5s busy timeout
- [X] T016 Implement SQLite schema creation in `crates/cruxe-state/src/schema.rs`: all 8 tables from data-model.md (`projects`, `file_manifest`, `branch_state`, `branch_tombstones`, `index_jobs`, `known_workspaces`, `symbol_relations`, `symbol_edges`)
- [X] T017 [P] Implement project_id generation: `blake3(realpath(repo_root))[:16]` in `crates/cruxe-core/src/types.rs`
- [X] T018 Implement Tantivy index creation in `crates/cruxe-state/src/tantivy_index.rs`: three index schemas (`symbols`, `snippets`, `files`) per data-model.md with `ref` field in all
- [X] T019 Implement custom Tantivy tokenizers in `crates/cruxe-state/src/tokenizers.rs`: `code_camel`, `code_snake`, `code_dotted`, `code_path` per data-model.md
- [X] T020 [P] Write unit tests for custom tokenizers in `crates/cruxe-state/src/tokenizers.rs`: verify `CamelCase` → `[camel, case]`, `snake_case` → `[snake, case]`, `pkg.module.Class` → `[pkg, module, class]`, `src/auth/handler.rs` → `[src, auth, handler, rs]`
- [X] T021 [P] Implement `symbol_stable_id` computation in `crates/cruxe-core/src/types.rs`: `blake3("stable_id:v1|" + language + "|" + kind + "|" + qualified_name + "|" + normalized_signature)`
- [X] T022 [P] Write unit test for `symbol_stable_id`: verify line movement does not change identity, verify signature change does
- [X] T023 Implement `crates/cruxe-core/src/lib.rs` to re-export all public types

**Checkpoint**: Storage engines (Tantivy + SQLite) functional, core types defined, config loading works

---

## Phase 3: User Story 1 - Initialize and Diagnose (Priority: P1) MVP

**Goal**: `cruxe init` and `cruxe doctor` work end-to-end

**Independent Test**: Run `cruxe init` in a sample repo, then `cruxe doctor` — both succeed

### Implementation for User Story 1

- [X] T024 [US1] Implement `projects` CRUD in `crates/cruxe-state/src/project.rs`: create, get_by_root, get_by_id, update
- [X] T025 [US1] Implement VCS detection in `crates/cruxe-cli/src/commands/init.rs`: check for `.git` directory, set `vcs_mode` flag
- [X] T026 [US1] Implement `init` command in `crates/cruxe-cli/src/commands/init.rs`: register project in SQLite, create Tantivy index directories under `~/.cruxe/data/<project_id>/base/{symbols,snippets,files}/`, detect VCS mode, handle re-init gracefully
- [X] T027 [US1] Implement `doctor` command in `crates/cruxe-cli/src/commands/doctor.rs`: verify Tantivy index opens without error, verify SQLite `PRAGMA integrity_check`, check tree-sitter grammar availability for Rust/TypeScript/Python/Go, report ignore rule summary
- [X] T028 [US1] Implement `.cruxeignore` parsing in `crates/cruxe-indexer/src/scanner.rs`: use `ignore` crate, layered loading (built-in defaults → `.gitignore` → `.cruxeignore`), support `!` negation patterns
- [X] T029 [US1] Implement built-in default ignore list in `crates/cruxe-indexer/src/scanner.rs`: binary extensions (`.exe`, `.dll`, `.so`, `.dylib`, `.o`, `.a`, `.wasm`, `.pyc`, `.class`, `.jar`), directories (`.git/`, `node_modules/`, `__pycache__/`, `.tox/`, `target/`, `build/`), patterns (`*.min.js`, `*.min.css`, `*.generated.*`, `*.pb.go`, `*_generated.rs`)
- [X] T030 [US1] Implement CLI `main.rs` in `crates/cruxe-cli/src/main.rs`: clap app with `init`, `doctor` subcommands, `--verbose` flag, tracing-subscriber setup
- [X] T031 [US1] Write integration test for `init` + `doctor` roundtrip: create temp directory, run init, verify SQLite tables exist, verify Tantivy indices open, run doctor and check output

**Checkpoint**: `cruxe init` creates project, `cruxe doctor` reports healthy

---

## Phase 4: User Story 2 - Index and Locate Symbols (Priority: P1) MVP

**Goal**: `cruxe index` populates indices, `locate_symbol` returns correct `file:line`

**Independent Test**: Index a fixture repo, locate a known symbol, verify correct file:line

### Implementation for User Story 2

- [X] T032 [US2] Implement file scanner in `crates/cruxe-indexer/src/scanner.rs`: walk directory tree with ignore chain, collect `(path, language)` pairs, detect language from extension
- [X] T033 [US2] Implement tree-sitter parser dispatcher in `crates/cruxe-indexer/src/parser.rs`: load grammar by language, parse source file, return syntax tree
- [X] T034 [P] [US2] Implement Rust symbol extraction in `crates/cruxe-indexer/src/languages/rust.rs`: extract fn, struct, enum, trait, impl, const, static, type alias with name, qualified_name, kind, signature, line range, parent_symbol_id
- [X] T035 [P] [US2] Implement TypeScript symbol extraction in `crates/cruxe-indexer/src/languages/typescript.rs`: extract function, class, interface, enum, const, type alias, method
- [X] T036 [P] [US2] Implement Python symbol extraction in `crates/cruxe-indexer/src/languages/python.rs`: extract def, class, async_def with decorators, module-level assignments
- [X] T037 [P] [US2] Implement Go symbol extraction in `crates/cruxe-indexer/src/languages/go.rs`: extract func, type (struct/interface), const, var, method (receiver-based)
- [X] T038 [US2] Implement symbol record builder in `crates/cruxe-indexer/src/symbol_extract.rs`: build `SymbolRecord` from tree-sitter nodes, compute `symbol_id` and `symbol_stable_id`, extract `parent_symbol_id` via scope nesting
- [X] T039 [US2] Implement snippet record builder in `crates/cruxe-indexer/src/snippet_extract.rs`: extract function bodies, class bodies, module-level blocks as `SnippetRecord` with imports context
- [X] T040 [US2] Implement batch writer in `crates/cruxe-indexer/src/writer.rs`: write `SymbolRecord` to Tantivy `symbols` index + SQLite `symbol_relations`, write `SnippetRecord` to Tantivy `snippets` index, write `FileRecord` to Tantivy `files` index + SQLite `file_manifest`, use blake3 content hashing
- [X] T041 [US2] Implement `index` command in `crates/cruxe-cli/src/commands/index.rs`: orchestrate scan → parse → extract → write pipeline, create `index_jobs` entry with state transitions (queued → running → published/failed), support `--force` flag for full re-index
- [X] T042 [US2] Implement `locate_symbol` in `crates/cruxe-query/src/locate.rs`: query Tantivy `symbols` index by `symbol_exact` field, apply definition-first ranking (definitions scored higher than references), filter by optional `kind`, `language`, `ref` parameters
- [X] T043 [US2] Write integration test: index `testdata/fixtures/rust-sample/`, call `locate_symbol("validate_token")`, verify returns correct `file:line`, `kind: "fn"`, and definition-first ordering
- [X] T044 [P] [US2] Create Rust fixture repo in `testdata/fixtures/rust-sample/`: 5-10 files with functions, structs, enums, traits, impl blocks, nested methods, imports
- [X] T045 [P] [US2] Create TypeScript fixture repo in `testdata/fixtures/ts-sample/`: 5-10 files with functions, classes, interfaces, enums, exports
- [X] T046 [P] [US2] Create Python fixture repo in `testdata/fixtures/python-sample/`: 5-10 files with functions, classes, decorators, module-level code
- [X] T047 [P] [US2] Create Go fixture repo in `testdata/fixtures/go-sample/`: 5-10 files with functions, structs, interfaces, methods, constants

**Checkpoint**: `cruxe index` populates all three indices, `locate_symbol` returns correct file:line

---

## Phase 5: User Story 3 - Search Code (Priority: P2)

**Goal**: `search_code` with query intent classification and multi-index search

**Independent Test**: Search for an error string in fixture repo, verify correct results

### Implementation for User Story 3

- [X] T048 [US3] Implement query intent classifier in `crates/cruxe-query/src/intent.rs`: classify into `symbol`, `path`, `error`, `natural_language` based on pattern matching (CamelCase/snake_case → symbol, contains `/` or file extension → path, contains quotes or stack trace patterns → error, default → natural_language)
- [X] T049 [US3] Implement query planner in `crates/cruxe-query/src/planner.rs`: select index priority and field weights based on intent, build Tantivy queries per index
- [X] T050 [US3] Implement `search_code` in `crates/cruxe-query/src/search.rs`: parallel query across `symbols`, `snippets`, `files` indices, merge results using RRF (Reciprocal Rank Fusion), apply per-intent field weights
- [X] T051 [US3] Implement rule-based reranker in `crates/cruxe-query/src/ranking.rs`: exact symbol match boost, qualified name boost, signature match boost, path affinity boost, definition-over-reference boost, language match boost
- [X] T052 [US3] Implement dual-index join in `crates/cruxe-query/src/search.rs`: for snippet matches, resolve `(path, line_range)` against `symbol_relations` table, enrich results with symbol metadata when match exists
- [X] T053 [US3] Implement `search` CLI command in `crates/cruxe-cli/src/commands/search.rs`: accept query string, optional `--ref`, `--lang`, `--limit` flags, format output as table with path:line, kind, name, score
- [X] T054 [US3] Write integration test: index fixture repo, search for `"connection refused"` (error intent), verify correct file and line returned
- [X] T055 [P] [US3] Write integration test: search for `"src/auth/handler.rs"` (path intent), verify file metadata returned
- [X] T056 [P] [US3] Write unit test for query intent classifier: verify classification for 10+ sample queries across all four intent types

**Checkpoint**: `search_code` returns ranked results with correct intent classification and file:line precision

---

## Phase 6: User Story 4 - MCP Server (Priority: P2)

**Goal**: `cruxe serve-mcp` exposes tools to AI agents via stdio

**Independent Test**: Start MCP server, send `tools/list`, verify tool schemas

### Implementation for User Story 4

- [X] T057 [US4] Implement MCP JSON-RPC server loop in `crates/cruxe-mcp/src/server.rs`: read JSON-RPC requests from stdin, dispatch to tool handlers, write responses to stdout, handle `initialize`, `tools/list`, `tools/call` methods
- [X] T058 [US4] Implement Protocol v1 response types in `crates/cruxe-mcp/src/protocol.rs`: `ProtocolMetadata` struct with `cruxe_protocol_version`, `freshness_status`, `indexing_status`, `result_completeness`, `ref`, `schema_status` fields
- [X] T059 [P] [US4] Implement `index_repo` tool handler in `crates/cruxe-mcp/src/tools/index_repo.rs`: delegate to indexer, return job_id and status
- [X] T060 [P] [US4] Implement `sync_repo` tool handler in `crates/cruxe-mcp/src/tools/sync_repo.rs`: trigger incremental sync, return changed file count
- [X] T061 [P] [US4] Implement `search_code` tool handler in `crates/cruxe-mcp/src/tools/search_code.rs`: delegate to query engine, return results with Protocol v1 metadata, stable handles (`result_id`, optional `symbol_id` / `symbol_stable_id`), and `suggested_next_actions`
- [X] T062 [P] [US4] Implement `locate_symbol` tool handler in `crates/cruxe-mcp/src/tools/locate_symbol.rs`: delegate to query engine, return results with Protocol v1 metadata and stable follow-up handles (`symbol_id`, `symbol_stable_id`)
- [X] T063 [P] [US4] Implement `index_status` tool handler in `crates/cruxe-mcp/src/tools/index_status.rs`: return project status, file/symbol counts, recent jobs, and startup compatibility fields (`schema_status`, `current_schema_version`, `required_schema_version`)
- [X] T064 [US4] Implement `serve-mcp` CLI command in `crates/cruxe-cli/src/commands/serve_mcp.rs`: start MCP server loop with workspace context
- [X] T065 [US4] Write integration test: start MCP server in-process, send `tools/list` request, verify all 5 tools listed with correct input schemas
- [X] T066 [US4] Write integration test: start MCP server with indexed fixture repo, call `locate_symbol` via JSON-RPC, verify response matches expected format with Protocol v1 metadata and includes stable handles (`symbol_id`, `symbol_stable_id`)

**Checkpoint**: MCP server responds to all tool calls, Protocol v1 metadata included in responses

---

## Phase 7: User Story 5 - Ref-Scoped Search Preview (Priority: P3)

**Goal**: Queries are scoped to a specific ref (branch name or `"live"`)

**Independent Test**: Index on two branches, query both, verify ref-consistent results

### Implementation for User Story 5

- [X] T067 [US5] Implement ref resolution in `crates/cruxe-query/src/planner.rs`: resolve `ref` parameter to effective ref (explicit > HEAD detection > `"live"` fallback), add `ref` filter to all Tantivy queries
- [X] T068 [US5] Implement HEAD detection via `git2` in `crates/cruxe-core/src/vcs.rs`: detect current branch name for VCS mode projects
- [X] T069 [US5] Update `index` command to accept `--ref` flag: index files under a specific ref scope, store ref in all Tantivy documents and SQLite records
- [X] T070 [US5] Update `branch_state` table operations in `crates/cruxe-state/src/branch_state.rs`: create/update `branch_state` entry on index, track `last_indexed_commit`
- [X] T071 [US5] Write E2E test: create a fixture repo with two branches (`main` and `feat/auth`), add a file only on `feat/auth`, index both, search with `ref: "feat/auth"` → new file found, search with `ref: "main"` → new file not found
- [X] T072 [US5] Write unit test: verify that single-version mode (no Git) defaults to `ref: "live"` for all operations

**Checkpoint**: Queries are ref-scoped, laying groundwork for Phase 2 VCS GA

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, cleanup, final validation

- [X] T073 [P] Update `README.md` with accurate project description, installation instructions (`cargo install cruxe`), quick start guide, MCP configuration example
- [X] T074 [P] Create `CLAUDE.md` project-level instructions for AI agents working on this codebase
- [X] T075 Verify `cargo build --release` produces a single statically-linked binary on macOS and Linux
- [X] T076 Run full test suite (`cargo test --workspace`) and fix any failures
- [X] T077 Run `cargo clippy --workspace -- -D warnings` and fix all lints
- [X] T078 Run `cargo fmt --check --all` and fix formatting
- [X] T079 [P] Add `--help` text for all CLI commands with usage examples
- [X] T080 Create `.cruxeignore` example file in `configs/` with documented patterns
- [X] T081 Run relevance benchmark: index `testdata/fixtures/rust-sample/`, execute 10 benchmark queries, verify top-1 precision >= 90% for symbol intent

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (workspace exists) — BLOCKS all user stories
- **US1 Init/Doctor (Phase 3)**: Depends on Phase 2 (storage engines ready)
- **US2 Index/Locate (Phase 4)**: Depends on Phase 3 (project can be initialized)
- **US3 Search (Phase 5)**: Depends on Phase 4 (indexed data exists to search)
- **US4 MCP Server (Phase 6)**: Depends on Phase 5 (search/locate logic available)
- **US5 Ref-Scoped (Phase 7)**: Depends on Phase 4 (indexer writes ref field)
- **Polish (Phase 8)**: Depends on all user stories

### User Story Dependencies

- **US1 (P1)**: After Foundational — No dependencies on other stories
- **US2 (P1)**: After US1 — Needs initialized project to index into
- **US3 (P2)**: After US2 — Needs indexed data for search
- **US4 (P2)**: After US3 — Wraps search/locate in MCP protocol
- **US5 (P3)**: After US2 — Adds ref filtering to existing index/query

### Parallel Opportunities

- Phase 1: All crate Cargo.toml files (T002–T007) can be created in parallel
- Phase 2: Types (T012), constants (T013), config (T014), tokenizer tests (T020), stable_id (T021–T022) can run in parallel
- Phase 4: All four language extractors (T034–T037) and fixture repos (T044–T047) can run in parallel
- Phase 5: Test cases T054/T055/T056 can run in parallel
- Phase 6: All tool handlers (T059–T063) can run in parallel
- Phase 8: Documentation tasks (T073, T074, T079, T080) can run in parallel

---

## Implementation Strategy

### MVP First (US1 + US2)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational
3. Complete Phase 3: US1 (init + doctor)
4. Complete Phase 4: US2 (index + locate)
5. **STOP and VALIDATE**: `cruxe init && cruxe index && cruxe search "AuthHandler"` works

### Incremental Delivery

1. Setup + Foundational → Foundation ready
2. US1 → `init` and `doctor` work (first demo)
3. US2 → Index and locate work (core value demo)
4. US3 → Search with intent classification (broader value)
5. US4 → MCP server (agent integration demo)
6. US5 → Ref-scoped preview (VCS groundwork)

## Notes

- [P] tasks = different files, no dependencies
- [USn] label maps task to specific user story
- Commit after each task or logical group
- Stop at any checkpoint to validate independently
- Total: 81 tasks, 8 phases
