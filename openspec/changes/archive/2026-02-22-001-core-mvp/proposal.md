## Why

Cruxe needs a foundational code navigation engine — a single Rust binary with zero external service dependencies that can index a repository, extract symbol definitions, and serve precise `file:line` results to AI coding agents via MCP protocol. Without project initialization, indexing, and search, no subsequent capability (agent protocol optimization, structure navigation, VCS correctness) can be built.

Symbol location is the core value proposition: accurate indexing and retrieval of function, struct, class, and method definitions. MCP integration is the primary distribution channel — without it, Cruxe is just a CLI tool; with it, every AI agent gains code navigation superpowers.

## What Changes

1. Add `cruxe init` command for project registration with VCS mode detection and `cruxe doctor` for health verification.
2. Add tree-sitter-based code indexing (`cruxe index`) for Rust, TypeScript, Python, and Go with three-layer ignore chain (built-in defaults, `.gitignore`, `.cruxeignore`).
3. Add `locate_symbol` with definition-first ranking policy returning `file:line` results with stable follow-up handles.
4. Add `search_code` with query intent classification (symbol, path, error, natural_language) and multi-index RRF merge.
5. Add MCP server (stdio transport) exposing `index_repo`, `sync_repo`, `search_code`, `locate_symbol`, and `index_status` tools with Protocol v1 metadata.
6. Add ref-scoped search preview — branch name as `ref` scope, laying groundwork for Phase 2 VCS GA.

## Capabilities

### New Capabilities

- `project-init-doctor`: Project registration, VCS mode detection, health verification.
  - FR-001: System MUST provide a `cruxe init` command that registers a project, creates index directories, and detects VCS vs single-version mode.
  - FR-002: System MUST provide a `cruxe doctor` command that verifies Tantivy index health, SQLite integrity, tree-sitter grammar availability, and active ignore rules with excluded file counts.

- `code-indexing`: File scanning, tree-sitter parsing, symbol extraction, incremental sync.
  - FR-003: System MUST scan source files respecting a three-layer ignore chain: built-in defaults, `.gitignore`, `.cruxeignore` (using gitignore-compatible glob syntax including `!` negation patterns).
  - FR-004: System MUST extract symbol definitions from source files using tree-sitter for Rust, TypeScript, Python, and Go as the v1 language set.
  - FR-005: System MUST populate three Tantivy indices (`symbols`, `snippets`, `files`) with ref-scoped records and custom code tokenizers.
  - FR-006: System MUST populate a SQLite `symbol_relations` table with `parent_symbol_id` extraction via tree-sitter scope nesting.
  - FR-007: System MUST compute `symbol_stable_id` using blake3 hash of `language + kind + qualified_name + normalized_signature`, excluding line numbers so that line movement does not change symbol identity.
  - FR-008: System MUST compute file content hashes using blake3 and store them in `file_manifest` for incremental diff capability.
  - FR-011: System MUST implement four custom Tantivy tokenizers: `code_camel` (CamelCase splitting), `code_snake` (snake_case splitting), `code_dotted` (dotted name splitting), `code_path` (file path component splitting).
  - FR-017: System MUST initialize SQLite with WAL mode, NORMAL synchronous, 64MB cache, foreign keys enabled, and 5s busy timeout.
  - FR-018: System MUST create all SQLite tables on initialization: `projects`, `file_manifest`, `branch_state`, `branch_tombstones`, `index_jobs`, `known_workspaces`, `symbol_relations`, `symbol_edges`.
  - FR-019: System MUST provide `sync_repo` that triggers incremental re-indexing using `file_manifest` content hashes, returning the count of changed files and a job ID for tracking.
  - FR-020: System MUST determine `freshness_status` as: `"fresh"` when last indexed commit matches HEAD (VCS mode) or manifest hash is current (single-version), `"stale"` when HEAD has advanced beyond last indexed commit, `"syncing"` when an index job is actively running.
  - FR-021: System MUST run startup index compatibility checks and classify index state as `compatible | not_indexed | reindex_required | corrupt_manifest`.
  - FR-022: When index state is `reindex_required` or `corrupt_manifest`, query tools MUST fail with actionable error code `index_incompatible` and remediation guidance (`cruxe index --force`).

- `symbol-location`: Definition-first symbol lookup with stable handles.
  - FR-009: System MUST provide `locate_symbol` with definition-first ranking policy returning `path`, `line_start`, `line_end`, `kind`, `name` at minimum, and MUST include a stable follow-up handle (`symbol_id` / `symbol_stable_id`).

- `code-search`: Intent-classified full-text search with multi-index merge.
  - FR-010: System MUST provide `search_code` with query intent classification into `symbol`, `path`, `error`, `natural_language` categories, achieving >= 85% correct classification on a benchmark query set of at least 20 queries.
  - FR-015: System MUST allow search queries during active indexing, returning partial results with appropriate status metadata.
  - FR-016: System MUST provide structured logging via `tracing` with configurable verbosity levels.

- `mcp-server`: MCP JSON-RPC server with Protocol v1 metadata.
  - FR-012: System MUST provide an MCP server (stdio transport) exposing `index_repo`, `sync_repo`, `search_code`, `locate_symbol`, and `index_status` tools.
  - FR-013: System MUST include Protocol v1 metadata in all search responses: `cruxe_protocol_version`, `freshness_status`, `indexing_status`, `result_completeness`, `ref`; canonical enums: `indexing_status`: `not_indexed | indexing | ready | failed`, `result_completeness`: `complete | partial | truncated`.

- `ref-scoped-search`: Branch-aware search scoping (VCS preview).
  - FR-014: System MUST support `ref` field in all index schemas, using branch name in VCS mode and `"live"` in single-version mode.

**Key Entities:**

- **Project**: A registered workspace with unique identity, absolute repo root path, VCS mode flag, schema/parser version tracking, and default ref for search scoping.
- **Symbol**: A code entity (function, struct, class, method, trait, interface, enum, constant) with name, qualified name, kind, language, file location (path + line range), optional signature, parent relationship, and stable identity hash.
- **Snippet**: A code block (function body, class body) indexed for full-text search, linked to symbols by file path and line range overlap for dual-index join.
- **File**: A source file record with path, language detection, content hash for incremental diff, and modification timestamp for fast pre-filtering.
- **IndexJob**: A state machine tracking indexing operations through queued, running, validating, published, failed, and rolled_back states.

**Post-implementation notes (2026-02-25):**
- Runtime protocol error handling now uses centralized typed codes from `cruxe-core::error::ProtocolErrorCode`.
- Config normalization now canonicalizes `freshness_policy` and `ranking_explain_level` during load.
- Core typed foundations now include `OverlayMergeKey` for later VCS overlay merge semantics.

## Impact

- Affected crates: `cruxe-cli`, `cruxe-core`, `cruxe-state`, `cruxe-indexer`, `cruxe-query`, `cruxe-mcp` (all 6 crates bootstrapped).
- API impact: establishes all MCP tool contracts and Protocol v1 response envelope.
- Performance targets: symbol lookup p95 < 300ms warm, search p95 < 500ms, full index < 60s for 5k files.
- Distribution: single binary via `cargo install cruxe`, zero external service dependencies.

**Success Criteria:**

- SC-001: Developers can install and initialize Cruxe in under 2 minutes on a clean machine with Rust toolchain available.
- SC-002: Symbol lookup returns the correct definition location as the top-1 result for at least 90% of benchmark queries on fixture repositories.
- SC-003: Full repository indexing completes in under 60 seconds for a 5,000-file Rust or Go repository.
- SC-004: The MCP server responds to tool calls within 500ms p95 on a warm index.
- SC-005: The single binary has zero external service dependencies — no database server, no search engine, no container runtime required.
- SC-006: AI coding agents can discover and use Cruxe tools via standard MCP protocol without custom integration code.
- SC-007: For repeated queries against unchanged source snapshots, `symbol_id` and `symbol_stable_id` remain stable across responses in >= 99% of sampled results.

**Edge Cases:**

- Tree-sitter grammar unavailable for a language: file indexed at file level only (no symbol extraction), warning in `doctor` output.
- File exceeds max size limit (default 1MB): skipped with warning, excluded from search results.
- SQLite locked during concurrent read/write: WAL mode with 5s busy_timeout handles transparently; retry with exponential backoff on timeout.
- Tantivy index corrupted: `doctor` detects corruption and recommends `cruxe index --force` for full rebuild.
- `.cruxeignore` syntax errors: invalid patterns logged as warnings and skipped; valid patterns still apply.
- `cruxe index` interrupted mid-operation: next run detects incomplete state via `file_manifest` content hashes and re-indexes only uncommitted files.
