## Context

Build the foundational Rust workspace, embedded storage engines (Tantivy + SQLite), tree-sitter-based code parser, symbol indexer, and MCP server (stdio) with `locate_symbol` and `search_code` tools. The deliverable is a single binary that can be installed via `cargo install`, indexes a code repository, and serves precise `file:line` symbol location results to AI coding agents via MCP protocol.

**Technical stack:**

| Aspect | Choice |
|--------|--------|
| Language | Rust (latest stable, 2024 edition) |
| Dependencies | tantivy, rusqlite, tree-sitter, tokio, clap, serde, tracing, git2, blake3, ignore |
| Storage | Tantivy (embedded full-text search) + SQLite (embedded structured data, WAL mode) |
| Testing | cargo test + fixture repos for integration/E2E |
| Target platforms | macOS (arm64, x86_64), Linux (x86_64, aarch64), Windows (x86_64) |
| Performance goals | symbol lookup p95 < 300ms warm, search p95 < 500ms, full index < 60s for 5k files |
| Constraints | zero external service dependencies, single binary distribution, < 100MB idle memory |
| Scale | single repo up to 50k files, v1 languages: Rust, TypeScript, Python, Go |

### Constitution Alignment

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | `locate_symbol` with definition-first policy is the primary tool |
| II. Single Binary Distribution | PASS | Tantivy + SQLite embedded, `cargo install` distribution |
| III. Branch/Worktree Correctness | PASS (preview) | `ref` field in all schemas from day one; full VCS GA deferred to Phase 2 |
| IV. Incremental by Design | PASS | blake3 content hashing in `file_manifest`, idempotent operations |
| V. Agent-Aware Response Design | PASS | Protocol v1 metadata in all responses; detail_level deferred to Phase 1.1 |
| VI. Fail-Soft Operation | PASS | Search during indexing with partial results; no external providers in MVP |
| VII. Explainable Ranking | PASS (partial) | Basic ranking in MVP; debug ranking_reasons deferred to Phase 1.1 |

## Goals / Non-Goals

**Goals:**

1. Bootstrap a 6-crate Rust workspace with clear responsibility separation.
2. Implement embedded Tantivy + SQLite storage with three index schemas (symbols, snippets, files).
3. Build tree-sitter extraction for Rust, TypeScript, Python, and Go.
4. Deliver `locate_symbol` with definition-first ranking and `search_code` with intent classification.
5. Ship an MCP server (stdio) with Protocol v1 metadata in all responses.
6. Establish ref-scoping foundation for future VCS GA.

**Non-Goals:**

1. Full VCS branch overlay merge — deferred to Phase 2 (005-vcs-core).
2. `detail_level` and token budget optimization — deferred to Phase 1.1 (002-agent-protocol).
3. Debug `ranking_reasons` output — deferred to Phase 1.1.
4. HTTP transport — deferred to Phase 1.5b (004-workspace-transport).
5. Semantic/hybrid search — deferred to Phase 3 (008-semantic-hybrid).

## Decisions

### D1. Rust workspace with 6 crates

```text
crates/
├── cruxe-cli/          # Binary entry point (clap commands)
├── cruxe-core/         # Shared types, errors, config, constants
├── cruxe-state/        # SQLite state store + Tantivy index management
├── cruxe-indexer/      # Scanner, parser, index writer
├── cruxe-query/        # Search, locate, query planner
└── cruxe-mcp/          # MCP server (stdio transport)
```

`cruxe-core` contains shared types and config (no separate config crate — config logic is lightweight). Crates are added incrementally: Phase 0 starts with `cli`, `core`, `state`; Phase 1 adds `indexer`, `query`, `mcp`.

**Why:** Clean responsibility boundaries, parallel compilation, testability per layer.

### D2. Dual embedded storage (Tantivy + SQLite)

Tantivy for full-text search (symbols, snippets, files indices), SQLite for structured data (projects, file_manifest, symbol_relations, index_jobs). Both are embedded — no external server required.

**Why:** Tantivy provides high-performance inverted index with custom tokenizers; SQLite provides ACID transactions for state management. Embedding both achieves the single-binary distribution constraint.

### D3. Four custom code tokenizers

`code_camel` (CamelCase splitting), `code_snake` (snake_case splitting), `code_dotted` (dotted name splitting), `code_path` (file path component splitting).

**Why:** Standard tokenizers don't understand code naming conventions. Custom tokenizers ensure `CamelCase` → `[camel, case]` and `snake_case` → `[snake, case]` for accurate code search.

### D4. Definition-first ranking policy

`locate_symbol` ranks definitions above references. Combined with exact symbol match boost, qualified name boost, and path affinity boost in rule-based reranking.

**Why:** AI agents typically want "where is X defined?" not "where is X called?" — definition-first matches the primary use case.

### D5. Query intent classification

Classify queries into `symbol`, `path`, `error`, `natural_language` categories using pattern matching (CamelCase/snake_case → symbol, contains `/` or file extension → path, contains quotes or stack trace patterns → error, default → natural_language). Target >= 85% correct classification.

**Why:** Different query types benefit from different index priorities and field weights. Intent-aware routing improves result relevance.

### D6. Source code layout

```text
Cargo.toml                    # Workspace root
crates/
├── cruxe-cli/src/
│   ├── main.rs
│   └── commands/{init,doctor,index,search,serve_mcp}.rs
├── cruxe-core/src/
│   ├── {lib,error,config,types,constants}.rs
├── cruxe-state/src/
│   ├── {lib,db,schema,project,manifest,symbols,jobs,tantivy_index,tokenizers}.rs
├── cruxe-indexer/src/
│   ├── {lib,scanner,parser,symbol_extract,snippet_extract,writer}.rs
│   └── languages/{mod,rust,typescript,python,go}.rs
├── cruxe-query/src/
│   ├── {lib,intent,planner,search,locate,ranking}.rs
└── cruxe-mcp/src/
    ├── {lib,server,protocol}.rs
    └── tools/{mod,index_repo,sync_repo,search_code,locate_symbol,index_status}.rs

configs/default.toml
testdata/fixtures/{rust-sample,ts-sample,python-sample,go-sample}/
testdata/golden/
```

**Why:** Follows Rust workspace conventions with clear module-per-file organization.

## Risks / Trade-offs

- **[Risk] Tree-sitter grammar quality varies across languages** → Mitigation: Focus on 4 well-supported languages (Rust, TS, Python, Go); fall back to file-level indexing when grammar unavailable.
- **[Risk] Tantivy index corruption on crash** → Mitigation: `doctor` detects corruption; `index --force` provides full rebuild path.
- **[Risk] SQLite contention under concurrent access** → Mitigation: WAL mode with 5s busy_timeout; retry with exponential backoff.
- **[Risk] Intent classification accuracy** → Mitigation: Pattern-based heuristics with >= 85% target; benchmark query set for regression testing.
