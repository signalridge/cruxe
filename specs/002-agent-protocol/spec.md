# Feature Specification: Agent Protocol Enhancement

**Feature Branch**: `002-agent-protocol`
**Created**: 2026-02-23
**Status**: Draft
**Version**: v0.2.0
**Depends On**: `001-core-mvp` (Phase 0+1)
**Input**: `specs/meta/design.md` sections 10.2, 10.3, 10.6, 10.11; Constitution principles V (Agent-Aware Response Design), VII (Explainable Ranking)

## User Scenarios & Testing

### User Story 1 - Agent Controls Response Verbosity via Detail Level (Priority: P1)

An AI coding agent calls `search_code` or `locate_symbol` and specifies
`detail_level: "location"` to get minimal token-cost results for existence
checks, or `detail_level: "context"` when it needs implementation details.
The default `"signature"` level gives the agent API shape information without
reading function bodies. With `compact: true`, large optional blocks are omitted
while preserving follow-up handles and ranking order. This reduces agent token
consumption by 5-10x on
location-only queries compared to always returning full context.

**Why this priority**: Constitution principle V (Agent-Aware Response Design)
identifies `detail_level` as the MUST from Phase 1.1. This is the primary
mechanism for agents to control context window consumption.

**Independent Test**: Call `locate_symbol` with each of the three detail levels
on the same symbol and verify that response token sizes differ by the expected
ratios (~50 / ~100 / ~300-500 tokens per result).

**Acceptance Scenarios**:

1. **Given** an indexed repository, **When** `locate_symbol` is called with
   `detail_level: "location"`, **Then** each result contains only `path`,
   `line_start`, `line_end`, `kind`, `name` (~50 tokens per result).
2. **Given** an indexed repository, **When** `locate_symbol` is called with
   `detail_level: "signature"` (or omitted, since it is the default), **Then**
   each result additionally contains `qualified_name`, `signature`, `language`,
   `visibility` (~100 tokens per result).
3. **Given** an indexed repository, **When** `search_code` is called with
   `detail_level: "context"`, **Then** each result additionally contains
   `body_preview`, `parent` context, and `related_symbols` (~300-500 tokens
   per result).
4. **Given** a `search_code` call with `detail_level: "location"`, **When**
   results are returned, **Then** the response JSON does not include
   `qualified_name`, `signature`, `body_preview`, or `related_symbols` fields
   (fields are omitted, not null).

---

### User Story 2 - Agent Gets File Symbol Outline Without Full Read (Priority: P1)

An AI agent knows a file path (from git diff, error output, or a previous
search result) but does not want to read the entire file. It calls
`get_file_outline` to retrieve a nested symbol tree showing the file's
structure (classes, functions, methods, constants) with signatures and line
numbers. This costs ~100-200 tokens vs ~2000+ for a full file read.

**Why this priority**: File outline is one of the highest-value agent workflow
tools. It eliminates wasteful full-file reads and lets agents pick exactly which
symbol to inspect in detail.

**Independent Test**: Index a fixture repo, call `get_file_outline` on a known
file with nested symbols, verify the returned tree matches the file's actual
structure.

**Acceptance Scenarios**:

1. **Given** an indexed Rust file with a struct and an impl block containing
   methods, **When** `get_file_outline` is called with `depth: "all"`, **Then**
   the response contains a nested symbol tree where methods appear as children
   of the impl block.
2. **Given** the same file, **When** `get_file_outline` is called with
   `depth: "top"`, **Then** only top-level symbols (where `parent_symbol_id IS
   NULL`) are returned, without children.
3. **Given** a file path that does not exist in the index, **When**
   `get_file_outline` is called, **Then** a `file_not_found` error is returned.
4. **Given** a VCS-mode project, **When** `get_file_outline` is called with
   `ref: "feat/auth"`, **Then** results reflect the file's state on that ref.
5. **Given** any valid request, **When** `get_file_outline` responds, **Then**
   response latency is p95 < 50ms (pure SQLite query, no Tantivy).

---

### User Story 3 - Agent Checks System Health Before Querying (Priority: P2)

An AI agent or developer calls `health_check` to verify that CodeCompass is
operational before issuing search queries. The tool reports Tantivy index
health, SQLite integrity, grammar availability, active indexing job status,
and prewarm readiness. This lets agents decide whether to proceed with queries
or wait for the system to become ready.

**Why this priority**: Health awareness prevents agents from making queries
against a system that will return degraded results, and helps debug
configuration issues.

**Independent Test**: Start `serve-mcp`, call `health_check` immediately
(during prewarm), then again after warmup completes, and verify status
transitions from `"warming"` to `"ready"`.

**Acceptance Scenarios**:

1. **Given** a healthy system with warm indices, **When** `health_check` is
   called, **Then** `status` is `"ready"`, `tantivy_ok` is `true`,
   `sqlite_ok` is `true`.
2. **Given** an active indexing job, **When** `health_check` is called, **Then**
   `active_job` contains job details and `status` is `"indexing"`.
3. **Given** the system is prewarming indices, **When** `health_check` is
   called, **Then** `status` is `"warming"` and `prewarm_status` indicates
   progress.
4. **Given** a missing tree-sitter grammar for a configured language, **When**
   `health_check` is called, **Then** the `grammars` field lists which
   languages are available and which are missing.
5. **Given** index schema is incompatible with the running binary, **When**
   `health_check` is called, **Then** `startup_checks.index.status` is
   `reindex_required` and includes actionable remediation guidance.

---

### User Story 4 - Fast First Query via Index Prewarming (Priority: P2)

A developer starts `codecompass serve-mcp` and immediately issues a search
query. Because Tantivy uses mmap, the first query on a cold index would pay
page fault latency (potentially > 2000ms). With prewarming enabled (default),
the server forces mmap pages into memory on startup, so the first real query
achieves warm-index latency (p95 < 300ms for symbol lookup).

**Why this priority**: Cold-start latency is a poor first experience, especially
for AI agents that issue queries immediately after server start.

**Independent Test**: Start `serve-mcp`, time the first `locate_symbol` call,
verify it completes within the warm p95 target (< 300ms) rather than the cold
target (< 2000ms).

**Acceptance Scenarios**:

1. **Given** `serve-mcp` started with prewarming (default), **When** the first
   `locate_symbol` is called after startup, **Then** latency is p95 < 500ms
   (within warm range, not cold range).
2. **Given** `serve-mcp` started with `--no-prewarm`, **When** the first
   query is issued, **Then** it still succeeds (no blocking) but may have
   higher latency.
3. **Given** prewarming is in progress, **When** `health_check` is called,
   **Then** `status` is `"warming"` and queries are accepted but may be slower.
4. **Given** prewarming completes, **When** `health_check` is called, **Then**
   `prewarm_status` is `"complete"` and `status` transitions to `"ready"`.
5. **Given** prewarming is running in the background, **When** MCP client sends
   `initialize` or `tools/list`, **Then** requests succeed immediately (server
   handshake MUST NOT block on prewarm completion).

---

### User Story 5 - Developer Inspects Ranking Explanations (Priority: P3)

A developer or agent troubleshooting search quality sets
`ranking_explain_level` to `basic` or `full`. Search responses include a
`ranking_reasons` field in metadata that shows per-result scoring breakdown:
exact match boost, qualified name boost, path affinity, definition boost,
kind match, and BM25 score.

**Why this priority**: Constitution principle VII (Explainable Ranking) requires
that ranking decisions are transparent. This is the Phase 1.1 MUST for
explainability.

**Independent Test**: Set `ranking_explain_level: "full"`, run a search, verify each result
includes a `ranking_reasons` object with the expected scoring factors.

**Acceptance Scenarios**:

1. **Given** `ranking_explain_level: "full"`, **When** `search_code` is called, **Then**
   each result includes `ranking_reasons` with fields: `exact_match_boost`,
   `qualified_name_boost`, `path_affinity`, `definition_boost`, `kind_match`,
   `bm25_score`.
2. **Given** `ranking_explain_level: "off"` (default), **When** `search_code` is
   called, **Then** the `ranking_reasons` field is absent from the response.
3. **Given** a search where one result is an exact symbol match, **When**
   ranking reasons are inspected, **Then** `exact_match_boost` is nonzero
   for that result and zero for non-exact matches.
4. **Given** `ranking_explain_level: "basic"`, **When** `search_code` is called,
   **Then** only normalized compact factors are returned (no verbose per-stage
   internals). **Given** `ranking_explain_level: "full"`, **Then** full debug
   factors are returned.

---

### User Story 6 - Agent Gets Reliable Results Despite Stale Index (Priority: P2)

An AI agent queries CodeCompass, but the index is stale (developer has made
changes since last sync). Under the default `balanced` policy, the agent
receives results with a stale indicator in metadata, and an async sync is
triggered in the background. Under `strict` policy, the query is blocked
with an error and guidance. Under `best_effort`, results always return
immediately with no sync triggered.

**Why this priority**: Stale-aware behavior ensures agents can make informed
decisions about result reliability, and the configurable policy levels let
teams tune the tradeoff between freshness and latency.

**Independent Test**: Modify a file after indexing, then query with each
freshness policy level and verify the expected behavior.

**Acceptance Scenarios**:

1. **Given** a stale index and `freshness_policy: "balanced"` (default),
   **When** `search_code` is called, **Then** results are returned with
   `freshness_status: "stale"` in metadata, and an async sync is triggered.
2. **Given** a stale index and `freshness_policy: "strict"`, **When**
   `search_code` is called, **Then** the query returns an error with code
   `index_stale` and guidance to run `sync_repo`.
3. **Given** a stale index and `freshness_policy: "best_effort"`, **When**
   `search_code` is called, **Then** results are returned immediately with
   `freshness_status: "stale"` and no background sync is triggered.
4. **Given** `freshness_policy` is set in `config.toml`, **When** a per-request
   `freshness_policy` is also provided, **Then** the per-request value takes
   precedence.

### Edge Cases

- What happens when `detail_level: "context"` is requested but the symbol has
  no parent or related symbols?
  The `parent` and `related_symbols` fields are omitted (not null), and the
  `body_preview` is still included if available.
- What happens when `get_file_outline` is called on a file with no symbols
  (e.g., a config file without tree-sitter grammar)?
  An empty `symbols` array is returned with file metadata.
- What happens when prewarming fails (e.g., corrupted index segment)?
  Health status reports `"error"` with diagnostic details. Queries are still
  accepted but fall back to cold-start behavior.
- What happens when `ranking_reasons` is requested on a `locate_symbol` call?
  Ranking reasons are included since `locate_symbol` also uses the ranking
  pipeline (definition-first policy, exact match boost).
- What happens when freshness check itself is slow (e.g., large repo with
  many files)?
  The freshness check uses lightweight signals (HEAD commit comparison for VCS
  mode, manifest hash cursor for single-version). It does not scan files.
- What happens when index schema is incompatible after upgrade?
  `health_check` and `index_status` expose `startup_checks.index.status =
  reindex_required`; query tools return actionable `index_incompatible` errors
  until `codecompass index --force` completes.

## Requirements

### Functional Requirements

- **FR-101**: System MUST accept a `detail_level` parameter on `search_code`
  and `locate_symbol` with values `"location"`, `"signature"` (default),
  `"context"`, controlling response field inclusion per `design.md` Section 10.3.
- **FR-102**: At `detail_level: "location"`, results MUST contain only `path`,
  `line_start`, `line_end`, `kind`, `name` (~50 tokens per result).
- **FR-103**: At `detail_level: "signature"`, results MUST additionally contain
  `qualified_name`, `signature`, `language`, `visibility` (~100 tokens).
- **FR-104**: At `detail_level: "context"`, results MUST additionally contain
  `body_preview` (first N lines of body, truncated), `parent` context
  (kind, name, path, line), and `related_symbols` array (~300-500 tokens).
- **FR-105**: `detail_level` MUST be applied to response serialization only,
  not to query logic (all results are retrieved and ranked identically
  regardless of detail level).
- **FR-105a**: `search_code` and `locate_symbol` MUST accept `compact: bool`
  (default false). When true, serialization omits large optional payload fields
  while preserving identity/location/score/follow-up handles.
- **FR-105b**: Query responses MUST deduplicate near-identical hits by
  symbol/file-region before final top-k emission and surface suppressed count in metadata.
- **FR-105c**: Query responses MUST enforce hard payload safety limits and use
  `result_completeness: "truncated"` + deterministic suggested next actions
  instead of hard failure.
- **FR-106**: System MUST provide a `get_file_outline` MCP tool that returns
  a nested symbol tree for a given file path and ref.
- **FR-107**: `get_file_outline` MUST accept `path`, `ref`, `depth`
  (`"top"` | `"all"`), and optional `language` parameters.
- **FR-108**: `get_file_outline` MUST query the `symbol_relations` SQLite table
  using `parent_symbol_id` chains to build nested trees, with p95 < 50ms.
- **FR-109**: `get_file_outline` with `depth: "top"` MUST return only symbols
  where `parent_symbol_id IS NULL`.
- **FR-110**: System MUST provide a `health_check` MCP tool that returns
  operational status including Tantivy index health, SQLite integrity, grammar
  availability, active job status, prewarm status, and startup compatibility checks.
- **FR-111**: System MUST prewarm Tantivy indices on `serve-mcp` startup by
  default, touching segment metadata and running warmup queries to force mmap
  pages into memory, and prewarm MUST run asynchronously after the MCP server
  accepts client connections.
- **FR-112**: System MUST support `--no-prewarm` CLI flag on `serve-mcp` to
  disable prewarming.
- **FR-113**: Health status MUST report `"warming"` during prewarm, then
  transition to `"ready"` on completion.
- **FR-114**: System MUST include an optional `ranking_reasons` field in search
  response metadata when `ranking_explain_level` is not `off`.
- **FR-115**: `ranking_reasons` MUST contain per-result breakdown with fields:
  `exact_match_boost`, `qualified_name_boost`, `path_affinity`,
  `definition_boost`, `kind_match`, `bm25_score`.
- **FR-115a**: `search_code` and `locate_symbol` MUST support
  `ranking_explain_level` (`off` | `basic` | `full`) where:
  - `off` omits ranking explanations,
  - `basic` emits compact normalized factors for agent routing,
  - `full` emits full debug scoring breakdown for diagnostics.
- **FR-116**: System MUST perform a pre-query freshness check with configurable
  policy levels: `strict`, `balanced` (default), `best_effort`.
- **FR-117**: Under `strict` policy, the system MUST block queries when the
  index is stale and return an error with guidance.
- **FR-118**: Under `balanced` policy, the system MUST return results with a
  stale indicator and trigger an async sync in the background.
- **FR-119**: Under `best_effort` policy, the system MUST always return results
  immediately without triggering a sync.
- **FR-120**: Freshness policy MUST be configurable in `config.toml` and
  overridable per-request.
- **FR-121**: MCP handshake requests (`initialize`, `tools/list`) MUST succeed
  while prewarming is in progress; prewarming MUST NOT block server readiness.
- **FR-122**: System MUST include startup compatibility payload in `health_check`
  and `index_status` with `index.status`, `current_schema_version`, and
  `required_schema_version`.
- **FR-123**: When startup compatibility indicates `reindex_required` or
  `corrupt_manifest`, query tools MUST return `index_incompatible` with explicit
  remediation guidance.

### Key Entities

No new entities are introduced. This spec extends the behavior of existing
entities from 001-core-mvp:

- **Symbol**: Extended with detail-level-aware serialization (location /
  signature / context response shapes).
- **Protocol v1 Metadata**: Extended with optional `ranking_reasons` field
  and refined `freshness_status` semantics via policy levels.

## Success Criteria

### Measurable Outcomes

- **SC-101**: `detail_level: "location"` responses are <= 60 tokens per result
  on average across benchmark queries.
- **SC-102**: `detail_level: "signature"` responses are <= 120 tokens per result
  on average.
- **SC-102a**: `compact: true` responses are <= 20% payload bytes vs
  non-compact responses for the same query/limit.
- **SC-103**: `get_file_outline` responds in p95 < 50ms on files with up to
  200 symbols.
- **SC-104**: First query after `serve-mcp` startup (with prewarm) completes
  in p95 < 500ms.
- **SC-105**: `health_check` responds in p95 < 10ms.
- **SC-106**: Stale-aware `balanced` policy returns results within the same
  p95 latency envelope as non-stale queries (freshness check adds < 5ms).
- **SC-107**: `ranking_explain_level: "basic"` increases warm `search_code`
  p95 latency by <= 10% versus `off` on the same benchmark suite.
