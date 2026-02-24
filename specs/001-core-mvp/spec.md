# Feature Specification: CodeCompass Core MVP

**Feature Branch**: `001-core-mvp`
**Created**: 2026-02-23
**Status**: Implemented
**Version**: v0.1.0
**Input**: User description: "Rust workspace bootstrap, Tantivy/SQLite storage engines, tree-sitter parser, symbol indexer, MCP server with locate_symbol and search_code tools"

## User Scenarios & Testing

### User Story 1 - Initialize a Code Project for Indexing (Priority: P1)

A developer installs CodeCompass via `cargo install codecompass` and runs
`codecompass init` in their repository root. The system detects whether the
directory is a Git repo (VCS mode) or a plain directory (single-version mode),
creates the necessary index directories and SQLite state store, and confirms
readiness. The developer then runs `codecompass doctor` to verify all components
are healthy.

**Why this priority**: Without project initialization, no other feature can work.
This is the foundation that all subsequent indexing and search depends on.

**Independent Test**: Run `cargo install codecompass` from a clean environment,
then `codecompass init` in a sample repo, then `codecompass doctor` — all three
must succeed with zero external dependencies.

**Acceptance Scenarios**:

1. **Given** a directory with a `.git` folder, **When** `codecompass init` is run,
   **Then** a project entry is created in SQLite with `vcs_mode=1`, Tantivy index
   directories are created, and the command exits with success status.
2. **Given** a directory without `.git`, **When** `codecompass init` is run,
   **Then** a project entry is created with `vcs_mode=0` and `ref='live'`.
3. **Given** a freshly initialized project, **When** `codecompass doctor` is run,
   **Then** it reports Tantivy index health OK, SQLite integrity OK, and
   tree-sitter grammar availability for configured languages.
4. **Given** `codecompass init` has already been run in the same directory,
   **When** `codecompass init` is run again, **Then** it detects the existing
   project and exits gracefully without corrupting state.

---

### User Story 2 - Index a Codebase and Locate Symbols (Priority: P1)

A developer runs `codecompass index` to scan their repository. The system
discovers source files (respecting `.gitignore` and `.codecompassignore`),
parses them with tree-sitter, extracts symbol definitions (functions, structs,
classes, methods, traits, interfaces, enums, constants), and populates both
the Tantivy full-text indices and the SQLite symbol relations table. The
developer can then use the CLI or MCP tools to locate specific symbols by name
and get precise `file:line` results.

**Why this priority**: Symbol location is the core value proposition. Without
accurate indexing and retrieval, the product delivers no value.

**Independent Test**: Index a known fixture repository, then run
`codecompass search "validate_token"` and verify it returns the correct file path
and line number for the function definition.

**Acceptance Scenarios**:

1. **Given** a Rust repository with 50 source files, **When** `codecompass index`
   is run, **Then** all non-ignored files are scanned, symbols are extracted, and
   the Tantivy `symbols`, `snippets`, and `files` indices are populated.
2. **Given** an indexed repository, **When** `locate_symbol` is called with
   `"AuthHandler"`, **Then** the result includes `path`, `line_start`, `line_end`,
   `kind: "struct"`, and `name: "AuthHandler"` for the definition.
3. **Given** a repository with both a function definition and call sites,
   **When** `locate_symbol` is called, **Then** definitions are ranked before
   references (definition-first policy).
4. **Given** a `.codecompassignore` file that excludes `testdata/fixtures/large/`,
   **When** indexing runs, **Then** files in that directory are not indexed.
5. **Given** a file with `CamelCase` identifiers, **When** searching for `"camel"`,
   **Then** the custom `code_camel` tokenizer splits the identifier and returns
   matching results.

---

### User Story 3 - Search Code via Natural Language and Error Strings (Priority: P2)

A developer (or AI agent) uses `search_code` with a natural language query like
"where is rate limiting implemented" or an error string like "connection refused".
The system classifies the query intent, searches across the symbols, snippets, and
files indices, merges results using RRF (Reciprocal Rank Fusion), and returns
ranked results with file:line precision.

**Why this priority**: Extends beyond exact symbol lookup to support broader code
discovery workflows that AI agents commonly need.

**Independent Test**: Search for an error message string that appears in a known
fixture file and verify the result points to the correct location.

**Acceptance Scenarios**:

1. **Given** a query `"connection refused"`, **When** `search_code` is called,
   **Then** results include files containing that error string with correct line
   numbers.
2. **Given** a query `"src/auth/handler.rs"`, **When** `search_code` is called,
   **Then** the query is classified as `path` intent and the file metadata is
   returned.
3. **Given** a query `"how does authentication work"`, **When** `search_code` is
   called, **Then** the query is classified as `natural_language` intent and
   relevant code snippets are returned with symbol metadata where available.

---

### User Story 4 - Serve MCP Tools to AI Coding Agents (Priority: P2)

A developer configures CodeCompass as an MCP server (stdio transport) in their
AI coding agent (Claude Code, Cursor, etc.). The agent can call `index_repo`,
`sync_repo`, `search_code`, `locate_symbol`, and `index_status` tools. Responses
follow Protocol v1 with `freshness_status`, `indexing_status`, and
`result_completeness` metadata fields.

**Why this priority**: MCP integration is the primary distribution channel.
Without it, CodeCompass is just a CLI tool. With it, every AI agent gains
code navigation superpowers.

**Independent Test**: Start `codecompass serve-mcp`, send a JSON-RPC
`tools/list` request via stdin, verify all expected tools are listed with
correct schemas. Then call `locate_symbol` and verify a valid response.

**Acceptance Scenarios**:

1. **Given** `codecompass serve-mcp` is running, **When** a `tools/list` request
   is sent, **Then** the response includes `index_repo`, `sync_repo`,
   `search_code`, `locate_symbol`, and `index_status`.
2. **Given** an indexed project, **When** `locate_symbol` is called via MCP with
   `{"name": "validate_token"}`, **Then** the response contains `path`,
   `line_start`, `line_end`, `kind`, `name` and Protocol v1 metadata.
3. **Given** an MCP request during active indexing, **When** `search_code` is
   called, **Then** the response includes `indexing_status: "indexing"` and
   `result_completeness: "partial"`.

---

### User Story 5 - Ref-Scoped Search Preview (Priority: P3)

A developer working on a feature branch runs a search. The system uses the
branch name as the `ref` scope. Results reflect the state of files on that
branch. This is a preview of VCS mode — full branch overlay with base+overlay
merge is deferred to Phase 2.

**Why this priority**: Establishes the ref-scoping foundation that Phase 2 VCS GA
builds upon. Without this groundwork, branch isolation cannot be added later.

**Independent Test**: Index the same repo on two different branches, query both,
verify results reflect branch-specific file contents.

**Acceptance Scenarios**:

1. **Given** a repository indexed on `main` branch, **When** `search_code` is
   called with `ref: "main"`, **Then** results are scoped to the `main` index.
2. **Given** a repository indexed on `feat/auth` branch with a new file,
   **When** `locate_symbol` is called with `ref: "feat/auth"`, **Then** the new
   file's symbols are included in results.
3. **Given** single-version mode (no Git), **When** any search is performed,
   **Then** `ref` defaults to `"live"` transparently.

### Edge Cases

- What happens when tree-sitter grammar is not available for a file's language?
  The file is indexed at file level only (no symbol extraction), with a warning
  in `doctor` output.
- What happens when a file exceeds the max size limit (default 1MB)?
  The file is skipped during indexing with a warning logged, and excluded from
  search results.
- What happens when SQLite is locked during concurrent read/write?
  WAL mode with 5s busy_timeout handles this transparently. If timeout exceeds,
  the operation retries with exponential backoff.
- What happens when Tantivy index is corrupted?
  `doctor` detects corruption and recommends `codecompass index --force` for
  full rebuild.
- What happens when `.codecompassignore` has syntax errors?
  Invalid patterns are logged as warnings and skipped; valid patterns still apply.
- What happens when `codecompass index` is interrupted mid-operation?
  Next run detects incomplete state via `file_manifest` content hashes and
  re-indexes only the files that were not fully committed.

## Requirements

### Functional Requirements

- **FR-001**: System MUST provide a `codecompass init` command that registers a project,
  creates index directories, and detects VCS vs single-version mode.
- **FR-002**: System MUST provide a `codecompass doctor` command that verifies Tantivy
  index health, SQLite integrity, tree-sitter grammar availability, and active ignore
  rules with excluded file counts.
- **FR-003**: System MUST scan source files respecting a three-layer ignore chain:
  built-in defaults, `.gitignore`, `.codecompassignore` (using gitignore-compatible
  glob syntax including `!` negation patterns).
- **FR-004**: System MUST extract symbol definitions from source files using tree-sitter
  for Rust, TypeScript, Python, and Go as the v1 language set.
- **FR-005**: System MUST populate three Tantivy indices (`symbols`, `snippets`, `files`)
  with ref-scoped records and custom code tokenizers.
- **FR-006**: System MUST populate a SQLite `symbol_relations` table with
  `parent_symbol_id` extraction via tree-sitter scope nesting.
- **FR-007**: System MUST compute `symbol_stable_id` using blake3 hash of
  `language + kind + qualified_name + normalized_signature`, excluding line numbers
  so that line movement does not change symbol identity.
- **FR-008**: System MUST compute file content hashes using blake3 and store them in
  `file_manifest` for incremental diff capability.
- **FR-009**: System MUST provide `locate_symbol` with definition-first ranking policy
  returning `path`, `line_start`, `line_end`, `kind`, `name` at minimum, and
  MUST include a stable follow-up handle (`symbol_id` / `symbol_stable_id`).
- **FR-010**: System MUST provide `search_code` with query intent classification
  into `symbol`, `path`, `error`, `natural_language` categories, achieving >= 85%
  correct classification on a benchmark query set of at least 20 queries.
- **FR-011**: System MUST implement four custom Tantivy tokenizers: `code_camel`
  (CamelCase splitting), `code_snake` (snake_case splitting), `code_dotted`
  (dotted name splitting), `code_path` (file path component splitting).
- **FR-012**: System MUST provide an MCP server (stdio transport) exposing `index_repo`,
  `sync_repo`, `search_code`, `locate_symbol`, and `index_status` tools.
- **FR-013**: System MUST include Protocol v1 metadata in all search responses:
  `codecompass_protocol_version`, `freshness_status`, `indexing_status`,
  `result_completeness`, `ref`; canonical enums:
  - `indexing_status`: `not_indexed | indexing | ready | failed`
  - `result_completeness`: `complete | partial | truncated`
- **FR-014**: System MUST support `ref` field in all index schemas, using branch name
  in VCS mode and `"live"` in single-version mode.
- **FR-015**: System MUST allow search queries during active indexing, returning partial
  results with appropriate status metadata.
- **FR-016**: System MUST provide structured logging via `tracing` with configurable
  verbosity levels.
- **FR-017**: System MUST initialize SQLite with WAL mode, NORMAL synchronous, 64MB
  cache, foreign keys enabled, and 5s busy timeout.
- **FR-018**: System MUST create all SQLite tables on initialization: `projects`,
  `file_manifest`, `branch_state`, `branch_tombstones`, `index_jobs`,
  `known_workspaces`, `symbol_relations`, `symbol_edges`.
- **FR-019**: System MUST provide `sync_repo` that triggers incremental re-indexing
  using `file_manifest` content hashes, returning the count of changed files and
  a job ID for tracking.
- **FR-020**: System MUST determine `freshness_status` as: `"fresh"` when last
  indexed commit matches HEAD (VCS mode) or manifest hash is current (single-version),
  `"stale"` when HEAD has advanced beyond last indexed commit, `"syncing"` when an
  index job is actively running.
- **FR-021**: System MUST run startup index compatibility checks and classify
  index state as `compatible | not_indexed | reindex_required | corrupt_manifest`.
- **FR-022**: When index state is `reindex_required` or `corrupt_manifest`, query
  tools MUST fail with actionable error code `index_incompatible` and remediation
  guidance (`codecompass index --force`).

### Key Entities

- **Project**: A registered workspace with unique identity, absolute repo root path,
  VCS mode flag, schema/parser version tracking, and default ref for search scoping.
- **Symbol**: A code entity (function, struct, class, method, trait, interface, enum,
  constant) with name, qualified name, kind, language, file location (path + line
  range), optional signature, parent relationship, and stable identity hash.
- **Snippet**: A code block (function body, class body) indexed for full-text search,
  linked to symbols by file path and line range overlap for dual-index join.
- **File**: A source file record with path, language detection, content hash for
  incremental diff, and modification timestamp for fast pre-filtering.
- **IndexJob**: A state machine tracking indexing operations through queued, running,
  validating, published, failed, and rolled_back states.

## Success Criteria

### Measurable Outcomes

- **SC-001**: Developers can install and initialize CodeCompass in under 2 minutes on a
  clean machine with Rust toolchain available.
- **SC-002**: Symbol lookup returns the correct definition location as the top-1 result
  for at least 90% of benchmark queries on fixture repositories.
- **SC-003**: Full repository indexing completes in under 60 seconds for a 5,000-file
  Rust or Go repository.
- **SC-004**: The MCP server responds to tool calls within 500ms p95 on a warm index.
- **SC-005**: The single binary has zero external service dependencies — no database
  server, no search engine, no container runtime required.
- **SC-006**: AI coding agents can discover and use CodeCompass tools via standard MCP
  protocol without custom integration code.
- **SC-007**: For repeated queries against unchanged source snapshots, `symbol_id` and
  `symbol_stable_id` remain stable across responses in >= 99% of sampled results.
