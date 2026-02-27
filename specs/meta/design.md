# CodeCompass - Design Specification

> Authoritative design specifications for CodeCompass. Cross-cutting project documents live in sibling files within specs/meta/.

## Related Documents

- [roadmap.md](roadmap.md) — Phase sequence, version mapping
- [execution-order.md](execution-order.md) — Cross-spec task dependencies
- [protocol-error-codes.md](protocol-error-codes.md) — Canonical transport error registry
- [development-guide.md](development-guide.md) — Coding conventions, workflow
- [repo-maintenance.md](repo-maintenance.md) — CI/CD, release, governance
- [testing-strategy.md](testing-strategy.md) — Test layers, fixtures
- [benchmark-targets.md](benchmark-targets.md) — Performance acceptance criteria

---

## 1. Executive Decision

CodeCompass will be built with a **Rust-first, zero-external-service architecture**:

- `Rust` for CLI, indexer, query planner, and MCP server.
- `Tantivy` (embedded) as the default full-text search backend for v1.
- `SQLite` for state store, metadata, and symbol relation graph.
- Lexical + structural ranking as the default retrieval mode.
- Optional semantic/hybrid layer and rerank providers as pluggable modules.
- **VCS mode MUST support branch and worktree as first-class features** (not optional).
- **Agent-aware response design**: MCP responses optimized for token budget and agent workflow.

Rust is selected for quality-first execution:

- strong compile-time guarantees and type safety,
- memory safety by default without GC pauses,
- deterministic performance for long-running local indexing workloads.

This matches the current product requirement:

- no local model inference required for initial launch,
- accurate code location and symbol lookup are primary goals,
- **true single-binary distribution** is required (no external services to install or manage).

Primary success target for v1:

- maximize symbol and definition location precision first,
- keep natural language semantic recall as an optional enhancement layer.
- guarantee branch-isolated retrieval correctness in VCS mode.

### 1.1 Why Tantivy over Meilisearch (design decision)

Previous drafts selected Meilisearch as the default search backend. After competitive analysis,
Tantivy (embedded Rust full-text search engine, equivalent to Lucene) is selected instead.

Rationale:

1. **True single binary**: `cargo install codecompass` works without installing/running a separate service.
   Meilisearch requires a separate process, port management, health checks, and version compatibility.
2. **Branch overlay via index segments**: Tantivy's segment model maps naturally to the base+overlay
   indexing strategy. Each branch overlay can be an isolated segment set, merged at query time.
   With Meilisearch, branch isolation requires separate index names and API-level routing.
3. **Zero service cold-start dependency**: Embedded index opens in-process. No "wait for Meilisearch ready" step.
   (First-query latency still exists on cold mmap pages; see [Section 10.11](#1011-tantivy-index-prewarming).)
4. **Full control over tokenization**: Code-specific tokenizers (CamelCase splitting, snake_case splitting,
   dotted-name tokenization) can be implemented as custom Tantivy tokenizers compiled into the binary.
5. **Simpler CI/distribution**: No Docker/sidecar dependency for testing or release.

Meilisearch remains a valid alternative backend behind the storage abstraction interface.
It can be re-evaluated if hosted/multi-tenant scenarios become a priority.

### 1.2 Competitive positioning (from research)

No existing open-source project delivers all three of:

1. **Branch/worktree-correct search** — zero competitors cover this.
2. **Symbol-level precision (`file:line`)** — most alternatives return text chunks, not code entities.
3. **Zero external service dependency** — most require Meilisearch, Elasticsearch, Qdrant, or cloud APIs.

This is the defensible gap CodeCompass fills.

### 1.3 Storage architecture decision: Tantivy + SQLite (no LMDB)

CodeCompass uses exactly two embedded storage engines. No third storage layer is needed.

| Layer | Engine | Purpose |
|-------|--------|---------|
| Full-text search | **Tantivy** (embedded) | BM25 retrieval, custom code tokenizers, segment-per-branch overlay |
| Structured data | **SQLite** (embedded) | Symbol relation graph, file manifest, branch state, job state, tombstones, project config |

**Why not LMDB**:

LMDB is an embedded key-value store (mmap-based B+tree) used internally by Meilisearch.
It is not relevant to CodeCompass because:

1. Tantivy has its own storage format (segment files, similar to Lucene). It does not use or need LMDB.
2. SQLite provides relational queries (JOIN, WHERE, GROUP BY, CTE) required for the symbol relation
   graph (`symbol_edges`, `symbol_relations`) and operational state. LMDB is key-value only.
3. Adding LMDB would mean three storage engines managing overlapping data with no capability gain.

**Why not RocksDB/sled/redb**:

Same reasoning: Tantivy owns search storage, SQLite owns structured data.
A third KV store adds complexity without new capability.

**SQLite configuration for CodeCompass**:

```sql
PRAGMA journal_mode = WAL;          -- concurrent read/write without blocking
PRAGMA synchronous = NORMAL;        -- balanced performance vs. durability
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;         -- 5s wait on lock contention
PRAGMA cache_size = -64000;         -- 64MB page cache
```

WAL mode is critical: it allows the MCP server to serve read queries while the indexer writes,
without either blocking the other. This matches the "search during indexing" requirement.

## 2. Research Findings (Consolidated)

### 2.1 Findings from `meilisearch-mcp`

- `meilisearch-mcp` is an MCP adapter over Meilisearch APIs, not an opinionated code retrieval engine.
- It proves a practical MCP pattern: `list_tools` + `call_tool`, thin handlers, stdio transport.
- It exposes broad operational capabilities (indexes, documents, settings, tasks, monitoring), useful as operational reference.
- Search behavior is delegated to Meilisearch SDK; ranking logic is mostly backend-side.

### 2.2 Findings from `mcp-local-rag`

- It implements a full retrieval pipeline in-process:
  parser -> semantic chunker -> embedder -> vector DB -> optional keyword boost.
- "Semantic search" in that project is mostly "semantic via vectors" plus hybrid keyword rerank.
- It is strong for local document Q/A workflows, but code navigation needs stronger symbol-level indexing than chunk-only pipelines.
- Good references:
  - staged ingestion,
  - safety boundaries (`BASE_DIR`),
  - well-scoped MCP tool surface.

### 2.3 Terminology alignment

- **Semantic search** is a retrieval goal (meaning-level relevance).
- **Vector search** is one implementation path for semantic retrieval.
- They are related but not identical.
- For code location, lexical + symbol retrieval often outperforms vector-only retrieval on precision queries.

### 2.4 Rerank findings

- Rerank should be stage-2 only over top-k candidates (not full corpus).
- External rerank services are viable for Rust (no local model needed):
  - Cohere Rerank,
  - Voyage Rerank,
  - Vertex AI Ranking API.
- Local non-ML rerank should exist as fallback:
  - exact symbol match boost,
  - qualified name boost,
  - file path and extension boost,
  - definition-over-reference priority.

### 2.5 Findings from Augment Context Engine / Context Connectors

High-value patterns to borrow:

- deterministic indexing lifecycle:
  - `discover -> filter -> hash -> diff -> index -> save_state`.
- source abstraction:
  - connector model first for local filesystem/worktree; remote providers can be added later.
- stateful incremental indexing:
  - persist index state outside process memory for restart/CI continuity.
- MCP operating modes:
  - prioritize local mode for workstation workflows.
- security-friendly retrieval split:
  - search via index, full-file read via source adapter with access control.
- automation-first operations:
  - prioritize local watcher + manual/CLI triggers; webhook automation is optional future work.

### 2.6 Findings from Augment MCP deep dive and open-source landscape (2026-02)

Key observations from systematic comparison of Augment MCP, VibeRAG, Sourcebot, Serena,
mcp-local-rag, mcp-rag-server, GitHub MCP Server, git-mcp, awslabs git-repo-research,
Elastic semantic code search, and Context-Engine-AI:

1. **No open-source project does branch-aware search**.
   All candidates index a single snapshot. Branch isolation, overlay merging,
   and VCS transition correctness are unique to CodeCompass.

2. **Chunk-level vs. symbol-level granularity gap**.
   VibeRAG, mcp-local-rag, and mcp-rag-server all return text chunks.
   Augment MCP returns "relevant context" without structured location data.
   Serena provides symbol-level navigation but depends on LSP, not a search index.
   CodeCompass's tree-sitter-based symbol index with `file:line` precision fills this gap.

3. **Augment's "intelligent context trimming" is a token budget problem**.
   Augment's value proposition includes "smart compression" of context for agents.
   This can be replicated without ML: structured `detail_level` responses + token budget
   awareness in MCP tool design. See [Section 10.4](#104-context-budget-aware-responses).

4. **External service dependency is the norm, not the exception**.
   Meilisearch, Elasticsearch, Qdrant, AWS Bedrock, Zilliz Cloud — every candidate
   requires at least one external service. CodeCompass with Tantivy embedded is the
   only zero-dependency option.

5. **MCP security is an unsolved ecosystem problem**.
   Prompt injection, tool chain RCE, DNS rebinding — all documented in the wild.
   mcp-rag-server's REPO_ROOT constraint + DNS rebinding protection + /health readiness
   is the best reference baseline. CodeCompass must match or exceed this.

6. **VibeRAG's intent-routed retrieval is the right design pattern**.
   Classifying queries into definitions/files/blocks/usages before retrieval
   (rather than one-size-fits-all search) aligns with CodeCompass's query intent router.

7. **Sourcebot and Serena show that structural code understanding (not just text search)
   is the quality differentiator**. Goto definition, find references, call graph —
   these require a symbol relation graph, not just flat index records.

8. **Elastic's dual-index model (chunks + locations join) is worth borrowing**.
   Separating "what matched" (snippet content) from "where exactly" (symbol location)
   enables precise `file:line` responses from fuzzy text matches.

### 2.7 Competitive re-exploration refresh (claude-context + grepai, 2026-02)

Second-pass investigation of `zilliztech/claude-context` and
`yoanbernabeu/grepai` confirms several patterns worth standardizing in
CodeCompass now (not as optional backlog notes):

1. **Background indexing + explicit status state machine is table-stakes**.
   `claude-context` exposes clear indexing lifecycle states and allows search while
   indexing with partial-result messaging. CodeCompass should keep non-blocking
   indexing and standardize status semantics across all tools.

2. **Watcher UX matters as much as retrieval quality**.
   `grepai` makes freshness practical with one command surface (`watch`,
   `--background`, `--status`, `--stop`) and debounced incremental updates.
   CodeCompass should adopt the same lifecycle ergonomics in Rust-first form.

3. **Structural path boost should be default-on for code agents**.
   `grepai` path-based penalties/bonuses (tests/mocks/generated/docs down;
   `src/lib/app` up) are simple and effective. CodeCompass should codify this as
   deterministic ranking policy, not an experimental option.

4. **Token-thrifty output modes are now mandatory**.
   `grepai` compact/TOON patterns and `claude-context` partial-state messaging both
   reduce agent retries and token burn. CodeCompass should pair `detail_level` with
   `compact` serialization and explicit truncation metadata.

5. **Semantic should remain optional and local-first**.
   `claude-context` quality often depends on external vector infrastructure.
   CodeCompass should preserve zero-external-service default and gate semantic
   behind `off | rerank_only | hybrid`, with local model-first execution.

## 3. Product Vision

Build a distributable code search and location tool that:

- returns reliable `file:line` answers for symbols and implementation queries,
- works natively with AI assistants through MCP tools,
- supports incremental indexing for fast refresh,
- scales from single repo to multi-repo workspaces,
- is optimized for local repository/worktree workflows first.

## 4. Product Principles

1. **Code navigation first**
   Definition/reference precision beats broad semantic recall.

2. **Explainable ranking**
   Every top result can expose why it was ranked high.

3. **Fail-soft operation**
   If rerank provider fails, fall back to lexical ranking without blocking users.

4. **Incremental by design**
   Git-aware incremental indexing is core, not optional.

5. **Distributable by default**
   Single binary + simple MCP setup.

6. **Connector-based ingestion**
   Indexing logic should stay modular, but local filesystem/worktree is the required source in v1.

## 5. Scope and Non-goals

### In scope (v1-v2)

- local repository and worktree indexing,
- lexical + structural retrieval over Tantivy (embedded),
- symbol relation graph with parent/import/call edges (v1.5+),
- agent-aware response design: detail levels, token budget, diff context,
- optional semantic/hybrid and provider rerank,
- MCP tooling for AI coding agents,
- CLI commands for init/index/sync/search/serve/doctor,
- branch-aware indexing and worktree-aware query isolation in VCS mode,
- branch diff context tool for PR review and change-aware search,
- persisted index state store for incremental resume and CI portability,
- local watcher + manual sync triggers,
- multi-workspace auto-discovery (dynamic workspace registration via MCP requests),
- `get_file_outline` tool (file-level symbol skeleton without full file read),
- `.codecompassignore` file support (layered on top of `.gitignore`),
- MCP health/readiness endpoint (HTTP transport) and initialize status (stdio transport),
- index progress notifications via MCP notification protocol,
- Tantivy index prewarming on `serve-mcp` startup.

### Out of scope (early)

- full LSP replacement,
- cloud multi-tenant SaaS control plane,
- local GPU model hosting,
- remote source connectors and hosted remote MCP.

### 5.1 Mandatory capability: Branch and Worktree (VCS mode)

This is a hard requirement for the project in VCS mode.
This is a release gate for VCS GA: Phase 1 can ship single-version/ref-scoped preview,
but must not be labeled VCS-ready until Phase 2 acceptance criteria pass.

Required behaviors:

1. branch-aware indexing
   - search must target an explicit or inferred `ref` scope.
   - results from other refs must not leak into current ref results.

2. worktree-aware isolation
   - each active ref must map to an isolated worktree context (logical or physical).
   - index updates from one worktree must only affect that ref overlay.

3. base + overlay execution model
   - default branch (`main`) keeps a base index.
   - feature branches use incremental overlay index from merge-base diff.
   - query resolution must merge base and overlay with deterministic precedence (overlay wins).

4. VCS transition correctness
   - checkout/rebase/merge/reset/force-push equivalent events must trigger ref state validation.
   - ancestry break must trigger overlay rebuild for that ref.

Acceptance criteria (must pass):

- same query on two different refs returns ref-consistent results.
- switching worktree does not reuse stale overlay from previous ref.
- deleted file in feature branch is not returned from base for that ref.
- rebase after indexing produces correct refreshed results without full base rebuild.

## 6. Rust-first Architecture

```text
Sources (Local FS / Local Worktrees)
  -> Source Adapter
  -> Scanner
  -> Parser/Extractor (tree-sitter + heuristics)
  -> Symbol Relation Builder (parent/import/call edges)
  -> Index Builder (symbols/snippets/files -> Tantivy)
  -> State Store (SQLite: metadata + relations + job state)
  -> Search Backend (Tantivy embedded, segment-per-branch)
  -> Query Planner (intent router + detail level + token budget)
  -> Snippet-to-Symbol Join (dual-index location resolution)
  -> Reranker (local rule or external provider)
  -> MCP Server + CLI
```

### Canonical repository layout (Rust workspace)

> **Note**: The original design listed ~10 crates (`codecompass-config`, `codecompass-parser`,
> `codecompass-ranker`, `codecompass-protocol`, etc.). During spec creation (001-core-mvp),
> these were consolidated into 6 implementation crates plus 1 deferred crate to reduce
> inter-crate complexity and simplify the dependency graph.

```text
Cargo.toml
crates/
  codecompass-cli/       # Binary entry point (clap commands, CLI UX)
  codecompass-core/      # Shared types, errors, config, constants (config folded in)
  codecompass-state/     # SQLite state store + Tantivy index management
  codecompass-indexer/   # File scanning, tree-sitter parsing, index writing (parser folded in)
  codecompass-query/     # Search, locate, query planner, ranking (ranker folded in)
  codecompass-mcp/       # MCP server, tool handlers, stdio transport (protocol folded in)
  codecompass-vcs/       # VCS adapter, branch overlay, worktree manager (deferred to spec 005)
configs/
testdata/
```

### 6.1 Recommended Rust libraries (initial)

- runtime and async: `tokio`
- CLI: `clap`
- serialization: `serde`, `serde_json`
- logging/observability: `tracing`, `tracing-subscriber`
- HTTP client/server: `reqwest` (client), `axum` or `hyper` (HTTP transport for MCP health endpoint)
- **full-text search engine: `tantivy`** (embedded, Lucene-equivalent)
- SQLite state store: `rusqlite` or `sqlx` (pick one in Phase 0)
- git/VCS integration: `git2` (or evaluate `gix` in Phase 1)
- file watching: `notify`
- parser infrastructure: `tree-sitter` + language grammars
- local embedding/rerank runtime (optional): `fastembed` (ONNX-backed, Rust-native)
- token estimation: word-count heuristic (`ceil(word_count * 1.3)`) or optional `tiktoken-rs` (for context budget)
- content hashing: `blake3` (fast, collision-resistant, used for `file_manifest.content_hash`)
- gitignore/ignore patterns: `ignore` crate (handles `.gitignore` + `.codecompassignore` with same semantics)

## 7. Retrieval and Ranking Strategy

### 7.1 v1 default: lexical + structural via Tantivy (no vector required)

- Primary retrieval via Tantivy embedded on three index schemas (`symbols`, `snippets`, `files`).
- All indices live in-process; no network calls for search.
- Query planner classifies:
  - symbol lookup,
  - path lookup,
  - error string lookup,
  - natural language query.
- Rule-based rerank over top candidates:
  - exact symbol > qualified name > signature > path > content body.
  - definition records > reference records.

Custom Tantivy tokenizers for code (built into the binary):

- `code_camel`: splits `CamelCase` into `[camel, case]`.
- `code_snake`: splits `snake_case` into `[snake, case]`.
- `code_dotted`: splits `pkg.module.Class` into `[pkg, module, class]`.
- `code_path`: splits file paths into components.
- These tokenizers are applied per-field (e.g., `symbol_exact` uses exact match,
  `qualified_name` uses `code_dotted`, `content` uses default + code_camel + code_snake).

### 7.1.1 Dual-index join model (snippets -> symbols location resolution)

Inspired by Elastic semantic code search's dual-index architecture:

- **snippets index**: stores content for full-text matching (BM25 retrieval).
- **symbols index**: stores precise location data (`path`, `line_start`, `line_end`, `kind`, `signature`).
- **Join at query time**:
  1. Run BM25 retrieval on `snippets` index -> top-k content matches.
  2. For each match, resolve `(path, line_range)` against `symbols` index.
  3. If a snippet maps to a known symbol, enrich the result with symbol metadata
     (kind, qualified_name, signature, parent).
  4. If no symbol match, return the snippet with file-level location only.

This solves the "found relevant text but can't pinpoint the exact entity" problem
that plagues chunk-based RAG approaches.

For symbol-type queries (`locate_symbol`), skip the join — query `symbols` index directly.

### 7.1.2 Structural path boost (default on)

To reduce top-k noise for agent workflows, ranking applies deterministic path
multipliers after first-stage retrieval:

- penalties (`factor < 1.0`): `tests`, `test`, `__tests__`, `mocks`, `fixtures`,
  `testdata`, `generated`, docs-heavy paths/files.
- bonuses (`factor > 1.0`): `src`, `lib`, `app`, `internal`, `core`.
- multiple rules multiply; final score remains explainable via `ranking_reasons`.
- match semantics should be segment-aware glob/regex, not naive substring-only
  matching, to avoid accidental penalties/bonuses (`contest` vs `test`).

Rules are config-backed with safe defaults and can be tuned without changing
index schema.

### 7.2 v2 optional: hybrid semantic

- Add optional vector index with a pluggable local backend (`sqlite` metadata +
  local vector segment/table by default; LanceDB adapter optional).
- Enable hybrid search for natural language query types only.
- Keep symbol queries lexical-first.
- Treat `semantic_ratio` as a runtime cap; allow adaptive lowering when lexical confidence is high.
- Add lexical short-circuit (`lexical_short_circuit_threshold`) to skip semantic branch on easy queries.
- Keep hybrid-tuning constants config-backed under `search.semantic` so behavior
  can be tuned without code changes:
  - confidence composite weights: `confidence_top_score_weight`,
    `confidence_score_margin_weight`, `confidence_channel_agreement_weight`
  - local reranker boosts: `local_rerank_phrase_boost`,
    `local_rerank_token_overlap_weight`
  - fanout multipliers: `semantic_limit_multiplier`,
    `lexical_fanout_multiplier`, `semantic_fanout_multiplier`
- Key vector records by stable symbol identity (`symbol_stable_id`) + snippet hash + model version.
- Enforce external provider privacy gates (`external_provider_enabled`, `allow_code_payload_to_external`) defaulting to false.
- Start from `semantic_mode = rerank_only`, then enable `hybrid` only after benchmark gates pass.
- Feature flags:
  - `semantic_mode` (`off | rerank_only | hybrid`),
  - `semantic_ratio`,
  - per-query-type overrides.
- Local embedding profiles should ship with Rust-friendly presets:
  - `fast_local`: `NomicEmbedTextV15Q`, `BGESmallENV15Q`
  - `code_quality`: `BGEBaseENV15Q`, `JinaEmbeddingsV2BaseCode`
  - `high_quality`: `BGELargeENV15`, `GTELargeENV15`, `SnowflakeArcticEmbedL`
- Add optional profile advisor (`profile_advisor_mode = suggest`) that inspects
  repo size/language mix and returns a recommendation without silently changing
  configured profile.

### 7.3 v2 optional: external rerank provider

- Abstract interface:
  - `Rerank(ctx, query, docs) -> scores`.
- Providers:
  - Cohere,
  - Voyage,
  - Vertex AI Ranking.
- Fallback:
  - if timeout/error, use local rule-based rerank.

### 7.4 Augment-inspired search behaviors to adopt (local-first)

Even without remote mode or embeddings, adopt these search behaviors:

1. **Intent-aware search profile**
   - classify query into `symbol`, `path`, `error`, `natural_language`.
   - apply profile-specific field weights and filters.
   - emit `query_intent_confidence` so agents can decide whether to escalate
     to semantic/rerank or refine the query first.

2. **Two-stage retrieval pipeline**
   - stage 1: fast parallel recall from `symbols`, `snippets`, `files`.
   - stage 2: deterministic local rerank over top candidates only.

3. **Stale-aware query execution**
   - run freshness check before query.
   - if index is stale, return best available result set with freshness metadata and trigger background incremental sync (balanced mode).

4. **Search during indexing**
   - allow queries while indexing is running.
   - attach `indexing_status` and `result_completeness` fields in response metadata ([Section 10.2](#102-search-response-metadata-contract-protocol-v1)).

5. **Search/index read split**
   - ranking and hit selection come from index records.
   - full file content fetch uses local source adapter only when needed.

6. **Explainable ranking payload**
   - optionally include machine-readable ranking reasons for each hit:
     exact symbol hit, qualified name hit, path hit, definition boost, final score composition.

## 8. Recommended Feature Backlog and Algorithms

The following set is prioritized by expected impact for code-location quality.

### 8.1 High-priority features (recommended for MVP or soon after)

1. **Query Intent Router**
   - classify query into `symbol`, `path`, `error`, `natural_language`.
   - choose retrieval/ranking profile per intent.

2. **Three-index federated retrieval**
   - query `symbols`, `snippets`, and `files` in parallel via Tantivy.
   - merge and rerank with explicit per-index weights.

3. **Dual-index join (snippet -> symbol location)**
   - snippet content matches are resolved to symbol-level locations.
   - enables `file:line` precision from fuzzy text queries.
   - see [Section 7.1.1](#711-dual-index-join-model-snippets-symbols-location-resolution) for detailed design.

4. **Definition-first navigation policy**
   - definitions are ranked and returned before references by default.
   - references exposed as follow-up data when needed.

5. **Agent-aware detail levels**
   - `location`, `signature`, `context` response granularity.
   - reduces agent token consumption by 5-10x on location-only queries.
   - see [Section 10.3](#103-agent-aware-detail-levels-token-budget-optimization) for detailed design.

6. **Context budget management (`max_tokens`)**
   - `get_code_context` respects agent's remaining context window.
   - breadth/depth strategies for result fitting.
   - see [Section 10.4](#104-context-budget-aware-responses) for detailed design.

7. **Explainable ranking output**
   - include compact ranking reasons in debug mode:
     exact symbol hit, qualified name hit, path hit, language hit, backend score.

8. **Incremental git-aware indexing**
   - update only changed files and dependent symbol records.

9. **Branch diff context tool (`diff_context`)**
   - symbol-level change summary between branches.
   - no existing open-source competitor offers this.
   - see [Section 10.5](#105-branch-diff-context-tool-diff_context) for detailed design.

10. **Symbol relation graph**
    - parent/import/call edges between code entities.
    - enables `get_call_graph`, `get_file_outline`, `find_related_symbols`.
    - see [Section 9.0.2](#902-symbol-relation-graph-sqlite) for schema design.

11. **Fail-soft rerank**
    - provider timeout/error should not fail query; fallback to local rerank.

12. **Portable index state**
    - export/import index state to speed up CI and ephemeral runner execution.

13. **Connector capability matrix**
    - define required local source capabilities first:
      refs, metadata, file read, incremental diff.

14. **Safe full-file fetch path**
    - search hits come from index, full content retrieval comes from connector source APIs.

15. **Automation hooks**
    - local watcher + scheduled reconcile + manual CLI sync.

16. **File outline tool (`get_file_outline`)**
    - file-level symbol skeleton without reading full file content.
    - ~100-200 tokens vs. ~2000+ tokens for full file read.
    - see [Section 10.6](#106-file-outline-tool-get_file_outline) for detailed design.

17. **Multi-workspace auto-discovery**
    - dynamic workspace registration via MCP tool `workspace` parameter.
    - on-demand indexing for newly discovered workspaces.
    - borrowed from Augment MCP's `--mcp-auto-workspace`.
    - see [Section 10.7](#107-multi-workspace-auto-discovery) for detailed design.

18. **`.codecompassignore` file support**
    - layered ignore: built-in defaults -> `.gitignore` -> `.codecompassignore`.
    - see [Section 10.8](#108-ignore-file-support-codecompassignore) for detailed design.

19. **MCP health/readiness**
    - HTTP `/health` endpoint and stdio `health_check` tool.
    - project-level status reporting.
    - see [Section 10.9](#109-mcp-health-and-readiness) for detailed design.

20. **Index progress notifications**
    - MCP `notifications/progress` for long-running index operations.
    - see [Section 10.10](#1010-index-progress-notifications) for detailed design.

21. **Tantivy index prewarming**
    - force mmap pages into memory on `serve-mcp` startup.
    - reduce cold-start p95 latency.
    - see [Section 10.11](#1011-tantivy-index-prewarming) for detailed design.

22. **Workspace parameter parity across tool surface**
    - all query/path tools accept optional `workspace`, including future additions.
    - middleware auto-injects startup workspace when request omits it.

23. **Structural path boost defaults**
    - apply deterministic path penalties/bonuses (`tests/mocks/generated/docs` down,
      `src/lib/app/internal/core` up).
    - see [Section 7.1.2](#712-structural-path-boost-default-on).

24. **Compact output mode**
    - `compact: true` for token-thrifty search/locate outputs while preserving
      deterministic follow-up handles.
    - see [Section 10.3](#103-agent-aware-detail-levels-token-budget-optimization).

25. **Result dedup + hard-limit graceful degrade**
    - suppress near-duplicate hits by symbol/file region.
    - enforce payload limits with `result_completeness: "truncated"` metadata.
    - see [Section 10.2](#102-search-response-metadata-contract-protocol-v1).

26. **Watcher daemon lifecycle UX**
    - expose `watch`, `watch --background`, `watch --status`, `watch --stop`.
    - ensure full initial scan before switching to incremental updates.

### 8.2 Recommended algorithms

1. **RRF (Reciprocal Rank Fusion) for multi-index merging**
   - robust first-stage fusion for heterogeneous candidate lists.

2. **Field-aware weighted scoring**
   - prioritize `symbol_exact`, `qualified_name`, `signature`, `path` over body text.

3. **Rule-based stage-2 rerank**
   - deterministic weighted formula combining:
     lexical score, exact match flags, symbol kind, path affinity, language match.

4. **Identifier-aware query rewrite**
   - expand and normalize:
     `CamelCase`, `snake_case`, package-qualified names, and dotted symbols.

5. **Result diversification**
   - reduce duplicate-heavy top results from the same file when confidence is similar.

6. **Confidence threshold + agent guidance**
   - if confidence is low, return top candidates plus recommended next tool call.

## 9. Index Schema (Tantivy + SQLite)

Maintain separate indices for control and explainability.
Tantivy handles full-text search. SQLite handles structured metadata and relations.

### 9.0 Schema overview

Two storage layers:

- **Tantivy indices** (full-text, BM25): `symbols`, `snippets`, `files`.
  Used for retrieval (matching and ranking).
- **SQLite tables**: `symbol_relations`, `symbol_edges`, `projects`, `file_manifest`,
  `branch_state`, `branch_tombstones`, `index_jobs`, `known_workspaces`.
  Used for graph queries, state management, and ref-scoped join resolution.

### 9.0.1 Tantivy index schemas

1. **symbols**
   - one record per definition (v1), reference records optional in v2.
   - key fields:
     `repo`, `ref`, `commit`, `path`, `language`, `symbol_exact`,
     `qualified_name`, `kind`, `signature`, `line_start`, `line_end`, `content`.
   - Tantivy field types:
     - `symbol_exact`: STRING (exact match, no tokenization).
     - `qualified_name`: TEXT (tokenized with `code_dotted`).
     - `signature`: TEXT (tokenized with `code_camel` + `code_snake`).
     - `content`: TEXT (default tokenizer).
     - `kind`, `language`, `repo`, `ref`: STRING (facet/filter).
     - `line_start`, `line_end`: U64 (stored, not indexed).

2. **snippets**
   - one record per code block/function body.
   - key fields:
     `repo`, `ref`, `commit`, `path`, `language`, `chunk_type`, `imports`,
     `line_start`, `line_end`, `content`.
   - Purpose: full-text matching for natural language and error string queries.
   - Linked to symbols via `(repo, ref, path, line_start, line_end)` overlap.

3. **files**
   - one record per file.
   - key fields:
     `repo`, `ref`, `commit`, `path`, `filename`, `language`, `updated_at`, `content_head`.
   - Purpose: path-based lookup and file-level metadata.

### 9.0.2 Symbol relation graph (SQLite)

The symbol relation graph enables structural code navigation beyond flat search.
Inspired by Sourcebot (goto definition/references), Serena (symbol-level semantic navigation),
and Sourcegraph SCIP (cross-reference graph).

**`symbol_relations` table**:

```sql
CREATE TABLE symbol_relations (
  id INTEGER PRIMARY KEY,
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,                -- branch/tag/commit, or 'live' for single-version mode
  commit TEXT,                      -- best-effort commit SHA for this ref snapshot
  path TEXT NOT NULL,
  symbol_id TEXT NOT NULL,          -- ref-local ID: hash(repo + ref + path + kind + line_start + name)
  symbol_stable_id TEXT NOT NULL,   -- location-insensitive ID for diffing (line moves should not change it)
  name TEXT NOT NULL,               -- short name (e.g., "validate_token")
  qualified_name TEXT NOT NULL,     -- full name (e.g., "auth::jwt::validate_token")
  kind TEXT NOT NULL,               -- function, struct, class, method, trait, interface, enum, const, ...
  language TEXT NOT NULL,
  line_start INTEGER NOT NULL,
  line_end INTEGER NOT NULL,
  signature TEXT,                   -- full type signature if available

  -- Relation fields (v1.5+, nullable until parser produces them)
  parent_symbol_id TEXT,            -- enclosing class/module/namespace symbol_id
  visibility TEXT,                  -- public, private, protected, internal (language-specific)

  -- Content hash for incremental updates
  content_hash TEXT NOT NULL,

  UNIQUE(repo, ref, path, qualified_name, kind, line_start),
  UNIQUE(repo, ref, symbol_stable_id, kind)
);

CREATE INDEX idx_symbol_relations_lookup
  ON symbol_relations(repo, ref, path, line_start);
```

**`symbol_stable_id` v1 rule (frozen for implementation)**:

- Goal: line movement should not change identity, but signature-level API change should.
- Canonical input fields (normalized): `language`, `kind`, `qualified_name`, normalized `signature`.
- Excluded fields: `line_start`, `line_end`, `path`, `ref`, `commit`.
- Hash: `blake3("stable_id:v1|" + canonical_input)`.
- If signature is missing, use empty string in canonical input.
- Future algorithm updates must bump `stable_id_version` and run planned migration.

**`symbol_edges` table** (v1.5+):

```sql
CREATE TABLE symbol_edges (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT NOT NULL,
  edge_type TEXT NOT NULL,          -- 'imports', 'calls', 'implements', 'extends', 'references'
  confidence TEXT DEFAULT 'static', -- 'static' (parser-confirmed) or 'heuristic'

  PRIMARY KEY(repo, ref, from_symbol_id, to_symbol_id, edge_type)
);
```

All graph queries MUST be ref-scoped (`repo + ref`). In single-version mode,
`ref='live'` is mandatory and keeps semantics consistent with VCS mode.

**What this enables** (MCP tools that no competitor offers):

- `get_call_graph(symbol)` — "who calls this function" without grep.
- `get_file_outline(path)` — class/function skeleton of a file (saves token vs. reading full file).
- `find_related_symbols(symbol)` — symbols in same module/package.
- `get_symbol_hierarchy(symbol)` — parent chain (method -> class -> module -> package).

**Parser requirements for relation extraction**:

- v1: `parent_symbol_id` (tree-sitter scope nesting — straightforward).
- v1.5: `imports` edges (tree-sitter import statement parsing per language).
- v2: `calls` edges (tree-sitter function call site matching — higher complexity).
- v2+: `implements`/`extends` edges (language-specific, lower priority).

### 9.0.3 Full SQLite schema (operational + metadata tables)

Beyond the symbol graph tables ([Section 9.0.2](#902-symbol-relation-graph-sqlite)), SQLite stores all operational state.
This is the single source of truth for "what has been indexed, what needs updating".

Identity convention (to avoid ambiguity):

- `project_id`: internal primary identity for routing and job ownership.
- `repo_root`: canonical absolute workspace path (unique per project).
- `repo` fields in index/graph tables store canonical `repo_root` (normalized path string),
  not remote name or display label.

**`projects` table** — registered workspaces:

```sql
CREATE TABLE projects (
  project_id TEXT PRIMARY KEY,
  repo_root TEXT NOT NULL UNIQUE,       -- absolute path to repo root
  display_name TEXT,
  default_ref TEXT DEFAULT 'main',
  vcs_mode INTEGER NOT NULL DEFAULT 1,  -- 0 = single-version, 1 = VCS mode
  schema_version INTEGER NOT NULL DEFAULT 1,
  parser_version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,             -- ISO8601
  updated_at TEXT NOT NULL
);
```

**`file_manifest` table** — file fingerprint map for incremental diff:

```sql
CREATE TABLE file_manifest (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,              -- 'main', branch name, or 'live' (single-version mode)
  path TEXT NOT NULL,
  content_hash TEXT NOT NULL,     -- blake3 (fast, collision-resistant)
  size_bytes INTEGER NOT NULL,
  mtime_ns INTEGER,               -- optional, for fast pre-check before hashing
  language TEXT,
  indexed_at TEXT NOT NULL,        -- ISO8601
  PRIMARY KEY(repo, ref, path)
);
```

Design note: `content_hash` (not mtime) is the authoritative diff signal.
mtime is unreliable after `git checkout` and is used only as a fast pre-filter
("if mtime unchanged, skip hashing").

**`branch_state` table** — per-branch overlay tracking:

```sql
CREATE TABLE branch_state (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  merge_base_commit TEXT,           -- NULL for default branch
  last_indexed_commit TEXT NOT NULL,
  overlay_dir TEXT,                  -- path to Tantivy overlay index directory
  file_count INTEGER DEFAULT 0,     -- number of files in overlay
  created_at TEXT NOT NULL,
  last_accessed_at TEXT NOT NULL,    -- drives overlay eviction policy
  PRIMARY KEY(repo, ref)
);
```

**`branch_tombstones` table** — paths suppressed from base index per branch:

```sql
CREATE TABLE branch_tombstones (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  path TEXT NOT NULL,               -- base index path to suppress
  tombstone_type TEXT DEFAULT 'deleted',  -- 'deleted' or 'replaced'
  created_at TEXT NOT NULL,
  PRIMARY KEY(repo, ref, path)
);
```

**`symbol_edges` table** — inter-symbol relationship edges:

```sql
CREATE TABLE symbol_edges (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT,                -- nullable: unresolved targets use to_name
  to_name TEXT,                     -- fallback target name when to_symbol_id is NULL
  edge_type TEXT NOT NULL,          -- 'imports', 'calls', 'implements', 'extends'
  confidence TEXT DEFAULT 'static', -- 'static' or 'heuristic'
  source_file TEXT,                 -- call site source file
  source_line INTEGER,              -- call site line number
  CHECK (to_symbol_id IS NOT NULL OR to_name IS NOT NULL)
);
CREATE UNIQUE INDEX idx_symbol_edges_unique
  ON symbol_edges(repo, ref, from_symbol_id, edge_type,
    COALESCE(to_symbol_id, ''), COALESCE(to_name, ''),
    COALESCE(source_file, ''), COALESCE(source_line, -1));
CREATE INDEX idx_symbol_edges_to ON symbol_edges(repo, ref, to_symbol_id);
CREATE INDEX idx_symbol_edges_from_type ON symbol_edges(repo, ref, from_symbol_id, edge_type);
CREATE INDEX idx_symbol_edges_to_type ON symbol_edges(repo, ref, to_symbol_id, edge_type);
CREATE INDEX idx_symbol_edges_source_file ON symbol_edges(repo, ref, source_file, edge_type);
```

**`index_jobs` table** — job state machine:

```sql
CREATE TABLE index_jobs (
  job_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(project_id),
  ref TEXT NOT NULL,
  mode TEXT NOT NULL,                -- 'full', 'incremental', 'overlay_rebuild'
  head_commit TEXT,
  sync_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
    -- queued -> running -> validating -> published -> failed -> rolled_back
  changed_files INTEGER DEFAULT 0,
  duration_ms INTEGER,
  error_message TEXT,
  retry_count INTEGER DEFAULT 0,
  progress_token TEXT,              -- MCP progress notification token
  files_scanned INTEGER DEFAULT 0,
  files_indexed INTEGER DEFAULT 0,
  symbols_extracted INTEGER DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_jobs_status ON index_jobs(status, created_at);
CREATE INDEX idx_jobs_project_status_created ON index_jobs(project_id, status, created_at DESC);
CREATE UNIQUE INDEX idx_jobs_active_project_ref
  ON index_jobs(project_id, ref) WHERE status IN ('queued', 'running', 'validating');
```

**`known_workspaces` table** — multi-workspace auto-discovery registry:

```sql
CREATE TABLE known_workspaces (
  workspace_path TEXT PRIMARY KEY,   -- absolute path
  project_id TEXT REFERENCES projects(project_id),
  auto_discovered INTEGER DEFAULT 0, -- 1 if discovered via MCP workspace param
  last_used_at TEXT NOT NULL,
  index_status TEXT DEFAULT 'unknown' -- unknown, indexed, indexing, error
);
```

**`semantic_vectors` and `semantic_vector_meta` tables** — embedding store (V11):

```sql
CREATE TABLE semantic_vector_meta (
  meta_key TEXT PRIMARY KEY,
  meta_value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE semantic_vectors (
  project_id TEXT NOT NULL,
  ref TEXT NOT NULL,
  symbol_stable_id TEXT NOT NULL,
  snippet_hash TEXT NOT NULL,
  embedding_model_id TEXT NOT NULL,
  embedding_model_version TEXT NOT NULL,
  embedding_dimensions INTEGER NOT NULL,
  path TEXT NOT NULL,
  line_start INTEGER NOT NULL,
  line_end INTEGER NOT NULL,
  language TEXT NOT NULL,
  chunk_type TEXT,
  snippet_text TEXT NOT NULL,
  vector_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (project_id, ref, symbol_stable_id, snippet_hash, embedding_model_version)
);
CREATE INDEX idx_semantic_vectors_query ON semantic_vectors(project_id, ref, language);
```

### 9.1 Branch-aware indexing strategy (default: no full reindex)

Goal: support multi-branch search without reindexing the whole repository for every feature branch.

Branch indexing model with Tantivy segments:

- `base snapshot`:
  - maintain baseline Tantivy index directory for default branch (`main`) at indexed commit.
  - base index contains committed segments for symbols, snippets, files.
- `branch overlay`:
  - for each active branch, maintain a separate Tantivy index directory containing
    full replacement records for each changed file relative to merge-base with `main`.
  - overlay directory: `~/.codecompass/indices/<project>/overlay_<branch>/`.
  - SQLite `branch_tombstones` table tracks deleted/replaced paths per branch
    (so base results for those paths are suppressed at query time).
- `query merge`:
  - open both base and overlay Tantivy indices as readers.
  - search both in parallel, then deduplicate by merge key.
  - overlay result wins when key collision exists.
  - tombstoned paths from overlay are filtered out of base results.

Dedup keys are split into two levels:

- Storage key (within one ref/layer):
  - symbols: `repo + ref + symbol_stable_id + kind`
  - snippets: `repo + ref + path + chunk_type + line_start + line_end`
  - files: `repo + ref + path`
- Merge key (base + overlay query merge):
  - symbols: `repo + symbol_stable_id + kind`
  - snippets: `repo + path + chunk_type + line_start + line_end`
  - files: `repo + path`

### 9.2 Branch sync lifecycle

First-time branch index:

1. find merge-base between branch HEAD and default branch indexed commit.
2. compute changed file set using `git diff --name-status merge_base..HEAD`.
3. for each changed file, rebuild that file's full symbol/snippet/file records into overlay indices.
4. mark changed/deleted file paths in `branch_tombstones` so base rows are suppressed safely.

Incremental updates:

1. store `last_indexed_commit` per branch.
2. compute `git diff --name-status last_indexed_commit..HEAD`.
3. atomically replace overlay records on a per-file basis (never partial per-file writes).
4. refresh tombstones for paths deleted or replaced in this branch.

Rebase/force-push handling:

- if `last_indexed_commit` is no longer ancestor of `HEAD`, rebuild overlay from new merge-base.
- base indices stay unchanged.

When full rebuild is required:

- parser or schema major version change,
- hash algorithm or canonical key change,
- large rename/refactor wave crossing a threshold (for example > 30% files changed),
- data corruption detection or failed recovery.

### 9.3 Index consistency and rollback

- two-phase write for branch sync:
  1. write staged records with `sync_id`,
  2. validate counts/checksums,
  3. atomically switch active `sync_id`.
- on failure:
  - keep previous `sync_id` as read target,
  - rollback staged records asynchronously.
- all indexing operations must be idempotent by canonical key + content hash.
- overlay publish unit is a file (not an individual symbol row) to prevent mixed old/new records.

### 9.4 Version mode abstraction (single-version + VCS-like)

To support both repositories with and without Git-like history, CodeCompass will define two runtime modes:

1. **Single-version mode (no VCS metadata)**
   - treat the workspace as one mutable revision (`live`).
   - maintain `snapshot_id` from file manifest hash for incremental diff and rollback.
   - no branch overlay logic required.
   - all indexing and search requests default to `revision=live`.

2. **VCS mode (Git-like / branch-capable)**
   - treat `ref` (branch/tag/commit) as first-class search scope.
   - maintain `base` index for default branch and `overlay` index per active ref.
   - enforce one ref-one worktree context (logical or physical) to avoid result mixing.
   - queries can explicitly target `ref`, with current workspace `HEAD` as default.

### 9.5 VCS adapter and worktree manager design

Introduce a `VCSAdapter` abstraction to avoid hard-coupling indexing logic to Git commands:

- `DetectRepo(root) -> bool`
- `ResolveHEAD(root) -> commit`
- `ListRefs(root) -> []ref`
- `MergeBase(root, refA, refB) -> commit`
- `DiffNameStatus(root, from, to) -> []fileChange`
- `IsAncestor(root, older, newer) -> bool`
- `EnsureWorktree(root, ref) -> worktreePath`

Worktree manager rules:

- default root: `~/.codecompass/worktrees/<project>/<ref>/`.
- refcount-based lease for cleanup safety.
- no force remove by default; manual prune command required.
- worktree metadata persisted in local state DB for recovery after restart.

### 9.6 Query-time resolution (base + overlay merge)

For VCS mode queries, execution plan:

1. query overlay indices for target `ref`,
2. query base indices in parallel,
3. apply tombstone/path suppression from overlay state,
4. deduplicate by merge key (defined in [Section 9.1](#91-branch-aware-indexing-strategy-default-no-full-reindex)),
5. overlay wins on key collision,
6. run fusion (`RRF`) and local rerank,
7. return `source_layer` (`base` or `overlay`) for explainability.

### 9.7 Index update timing and trigger policy (authoritative)

Index update timing must be explicit and deterministic:

1. **Initial/manual trigger**
   - `codecompass index` or MCP `index_repo` starts bootstrap indexing.

2. **Event-driven trigger (preferred)**
   - watch only registered repository/worktree roots.
   - daemon lifecycle must support `watch`, `watch --background`,
     `watch --status`, `watch --stop`.
   - first start performs full scan + symbol extraction before switching to
     incremental event mode.
   - debounce per file path and batch updates (for example 500-1500ms).
   - changed files go to incremental job queue.

3. **Pre-query freshness trigger**
   - before each `search_code`/`locate_symbol`, run lightweight freshness check:
     - Single-version mode: compare manifest hash or mtime cursor.
     - VCS mode: compare recorded `HEAD` and changed file count.
   - if stale, enqueue fast incremental sync before serving or serve with stale warning based on policy.

4. **Periodic reconcile trigger (safety net)**
   - run low-frequency full consistency check (for example every 5 minutes).
   - catch missed watcher events and crashed sync jobs.

5. **VCS transition trigger**
   - on `checkout`, `rebase`, `merge`, `reset --hard`, force-push equivalent:
     - re-evaluate `HEAD`, `merge-base`, ancestry,
     - if non-fast-forward or ancestry break, rebuild overlay for that ref.

6. **Schema/parser trigger**
   - if `schema_version` / `parser_version` / `canonical_key` changes:
     - trigger planned reindex migration path.

7. **Operational override trigger**
   - support `sync --force` / MCP `sync_repo(force=true)` for manual remediation.

Freshness policy levels:

- `strict`: block result until incremental sync finishes.
- `balanced` (default): return current results + stale indicator + async sync.
- `best_effort`: return immediately and sync in background.

### 9.8 Index job state machine

- states: `queued -> running -> validating -> published -> failed -> rolled_back`.
- each job records:
  - `job_id`, `project_id`, `mode`, `ref`, `head_commit`, `sync_id`,
  - `changed_files`, `duration_ms`, `error_code`, `retry_count`.
- retry policy:
  - transient backend errors: exponential backoff.
  - deterministic parser/schema errors: fail fast and mark project unhealthy.
- startup recovery policy:
  - jobs found in `running`/`validating` after process restart are marked failed
    with `error_code = interrupted`.
  - if no previously published snapshot exists for that scope, expose
    `indexing_status: "not_indexed"`; otherwise keep last published data and mark
    freshness stale.

### 9.9 Augment-inspired indexing lifecycle contract

The local source adapter must implement the lifecycle:

1. discover candidate files from source,
2. filter by ignore/include/security constraints,
3. hash file content or canonical fingerprint,
4. diff with persisted state snapshot,
5. index only changed units (add/modify/delete),
6. save new state snapshot atomically.

Benefits:

- consistent behavior across local repositories and worktrees,
- lower risk of source-specific drift,
- deterministic retries and easier observability.

### 9.10 State store and portability design

State layers:

- local operational state (SQLite):
  - projects, refs, worktrees, jobs, last sync cursor.
- index diff state (manifest snapshot):
  - file fingerprint map used by incremental diff.
- optional portable snapshot package:
  - export/import for CI cache, shared runners, or disaster recovery.

Minimal commands:

- `codecompass state export --project <id> --out <file>`
- `codecompass state import --project <id> --in <file>`
- `codecompass state inspect --project <id>`

## 10. MCP Tool Surface (v1)

- `index_repo`
- `sync_repo`
- `search_code`
- `locate_symbol`
- `get_code_context`
- `get_file_outline` — file-level symbol skeleton (see [Section 10.6](#106-file-outline-tool-get_file_outline))
- `index_status`
- `health_check`
- `list_refs` (VCS mode only)
- `switch_ref` (optional helper for worktree-backed sessions)

`index_status` also exposes active job-level progress for clients without notifications support.

All tools that accept a query or path also accept an optional `workspace` parameter
for multi-workspace support (see [Section 10.7](#107-multi-workspace-auto-discovery)).

### 10.1 Candidate v1.5/v2 MCP tools (high value)

- `find_references` — uses symbol_edges table for call/import graph.
- `explain_ranking` — return scoring breakdown for a specific result.
- `get_call_graph` — who calls this symbol / what does this symbol call.
- `diff_context` — branch diff-aware symbol change summary (see [Section 10.5](#105-branch-diff-context-tool-diff_context)).
- `search_similar_symbol` — symbols with similar names/signatures.
- `compare_symbol_between_commits` — how a symbol changed across versions.
- `suggest_followup_queries` — agent guidance when confidence is low.

### 10.2 Search response metadata contract (Protocol v1)

`search_code` and `locate_symbol` responses should include:

- `codecompass_protocol_version`: `"1.0"`.
- `freshness_status`: `fresh | stale | syncing`.
- `indexing_status`: `not_indexed | indexing | ready | failed`.
- `result_completeness`: `complete | partial | truncated`.
- compatibility guidance for pre-migration runtimes:
  - `idle` -> `ready`
  - `partial_available` -> `ready`
- `ranking_reasons` (`ranking_explain_level != off`): array of deterministic scoring factors.
- `source_layer`: `base | overlay` for VCS mode.
- `ref`: effective query ref (`main`, feature branch, or `live`).
- `safety_limit_applied`: boolean, true when hard caps are enforced.
- `suppressed_duplicate_count`: integer, number of deduplicated sibling hits omitted.
- phase-3 semantic fields when applicable:
  - `semantic_triggered`,
  - `semantic_skipped_reason`,
  - `semantic_ratio_used`,
  - `embedding_model_version`,
  - `external_provider_blocked`.
- error contract fields:
  - `error.code` (stable machine code),
  - `error.message` (human-readable),
  - `error.data` (optional structured remediation payload).

This keeps agent behavior predictable and reduces unnecessary repeated tool calls.

### Response contract principles

- always include `repo`, `path`, `line_start`, `line_end`,
- include `ranking_reasons` when `ranking_explain_level` is enabled,
- return compact machine-readable JSON for agent orchestration.
- deduplicate near-identical hits by symbol/file region before final top-k emission.
- never hard-fail on size pressure; return `result_completeness: "truncated"` with actionable metadata.
- keep a single canonical error-code registry across CLI/MCP transports to avoid drift.

### 10.3 Agent-aware detail levels (token budget optimization)

Inspired by Augment Context Engine's "intelligent context trimming" and the observation
that most open-source MCP search tools return fixed-format results regardless of agent needs.

All search/locate tools accept an optional `detail_level` parameter:

```jsonc
// detail_level: "location" (most token-efficient, ~50 tokens per result)
// Use case: agent wants to confirm existence/location before reading
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token"
}

// detail_level: "signature" (default, ~100 tokens per result)
// Use case: agent wants to understand the API shape without reading the body
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token",
  "qualified_name": "auth::jwt::validate_token",
  "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
  "visibility": "public"
}

// detail_level: "context" (richest, ~300-500 tokens per result)
// Use case: agent needs to understand implementation details
{
  "path": "src/auth/jwt.rs",
  "line_start": 87,
  "line_end": 112,
  "kind": "fn",
  "name": "validate_token",
  "qualified_name": "auth::jwt::validate_token",
  "signature": "pub fn validate_token(token: &str, key: &[u8]) -> Result<Claims>",
  "visibility": "public",
  "body_preview": "// first N lines of function body, truncated to fit budget",
  "parent": { "kind": "impl", "name": "JwtValidator", "path": "src/auth/jwt.rs", "line": 45 },
  "related_symbols": [
    { "kind": "struct", "name": "Claims", "path": "src/auth/jwt.rs", "line": 12 },
    { "kind": "enum", "name": "TokenError", "path": "src/auth/error.rs", "line": 5 }
  ]
}
```

Default is `signature`. Agent systems (Claude Code, Cursor, etc.) can set a preference
via MCP configuration or per-request.

`search_code` and `locate_symbol` also accept optional `compact: true`:

- `compact = true` keeps identity/location/score fields and removes large body
  fragments by default.
- `compact` is orthogonal to `detail_level`:
  - `location + compact` for cheapest routing queries,
  - `signature + compact` for default agent browsing,
  - `context + compact` for constrained deep dives.

Explainability payload depth is independently controlled by
`ranking_explain_level`:

- `off` (default): no ranking reasons in payload.
- `basic`: compact normalized factors (`exact`, `path`, `definition`,
  `semantic`, `final`) suitable for agent policy routing.
- `full`: full debug payload for human tuning and offline audits.

### 10.4 Context budget-aware responses

`get_code_context` accepts a `max_tokens` parameter to enable agent-driven context management:

```jsonc
{
  "tool": "get_code_context",
  "input": {
    "query": "how does authentication work",
    "max_tokens": 4000,
    "strategy": "breadth",  // "breadth": many files, less content each
                             // "depth": fewer files, more content each
    "detail_level": "context"
  }
}
```

Response behavior:

1. Retrieve top-k results ranked by relevance.
2. Serialize each result at the requested `detail_level`.
3. Accumulate estimated token count per result.
4. Stop adding results when `max_tokens` would be exceeded.
5. Return the fitted result set plus metadata:

```jsonc
{
  "results": [ /* ... fitted results ... */ ],
  "metadata": {
    "total_candidates": 47,
    "returned": 12,
    "estimated_tokens": 3842,
    "truncated": true,
    "remaining_candidates": 35,
    "suggestion": "Use locate_symbol for specific symbols, or increase max_tokens"
  }
}
```

Token estimation uses a word-count heuristic: `estimated_tokens = ceil(whitespace_split_word_count * 1.3)`.
Optional `tiktoken-rs` integration for exact counts can be enabled via config.

This is a key differentiator: all competitors return "top-k then done".
CodeCompass actively helps the agent manage its context window.

### 10.5 Branch diff context tool (`diff_context`)

This tool has **no equivalent in any open-source competitor**. It enables:

- PR review agents to understand structural changes without reading raw diffs.
- Feature development agents to self-check "what public API did I change on this branch".
- CI agents to validate scope of changes before merge.

```jsonc
{
  "tool": "diff_context",
  "input": {
    "ref": "feat/oauth2",        // optional, default: current HEAD
    "base": "main",               // optional, default: default branch
    "scope": "symbols",           // "symbols" | "files" | "all"
    "detail_level": "signature"   // reuses the same detail_level system
  }
}
```

Response:

```jsonc
{
  "ref": "feat/oauth2",
  "base": "main",
  "merge_base_commit": "abc123def",
  "affected_files": 5,
  "summary": {
    "added_symbols": [
      { "kind": "fn", "name": "refresh_token", "path": "src/auth/oauth.rs", "line": 45,
        "signature": "pub fn refresh_token(token: &str) -> Result<TokenPair>" }
    ],
    "modified_symbols": [
      { "kind": "fn", "name": "authenticate", "path": "src/auth/handler.rs", "line": 23,
        "before_signature": "pub fn authenticate(req: &Request) -> Result<User>",
        "after_signature": "pub fn authenticate(req: &Request, provider: AuthProvider) -> Result<User>" }
    ],
    "deleted_symbols": [
      { "kind": "fn", "name": "legacy_auth", "path": "src/auth/legacy.rs", "line": 12 }
    ]
  },
  "metadata": {
    "freshness_status": "fresh",
    "overlay_commit": "def456abc"
  }
}
```

Implementation:

1. Compute merge-base between `ref` and `base`.
2. Get changed file list from VCS adapter (`DiffNameStatus`).
3. For changed files, compare symbol records in base index vs. overlay index.
4. Classify each symbol as added/modified/deleted using `symbol_stable_id`
   plus signature/content hash deltas.
5. For modified symbols, include before/after signatures from base and overlay records.

### 10.6 File outline tool (`get_file_outline`)

One of the highest-value tools for agent workflows. Agents frequently know a file path
(from git diff, error output, or previous search results) but don't want to read
the entire file (wastes tokens). This tool returns the file's symbol skeleton.

```jsonc
{
  "tool": "get_file_outline",
  "input": {
    "path": "src/auth/handler.rs",
    "depth": "all",              // "top" (top-level only) | "all" (nested symbols)
    "ref": "feat/oauth2"         // optional, default: current HEAD
  }
}
```

Response:

```jsonc
{
  "path": "src/auth/handler.rs",
  "language": "rust",
  "line_count": 142,
  "symbols": [
    { "kind": "use",    "name": "crate::auth::Claims",       "line": 1 },
    { "kind": "use",    "name": "crate::error::AppError",    "line": 2 },
    { "kind": "struct", "name": "AuthHandler",               "line": 12, "visibility": "pub",
      "line_end": 18 },
    { "kind": "impl",   "name": "AuthHandler",               "line": 20,
      "children": [
        { "kind": "fn", "name": "new",               "line": 21, "visibility": "pub",
          "signature": "pub fn new(config: AuthConfig) -> Self" },
        { "kind": "fn", "name": "authenticate",      "line": 35, "visibility": "pub",
          "signature": "pub fn authenticate(&self, req: &Request) -> Result<User>" },
        { "kind": "fn", "name": "validate_session",  "line": 68, "visibility": "pub",
          "signature": "pub fn validate_session(&self, token: &str) -> Result<Session>" },
        { "kind": "fn", "name": "hash_password",     "line": 95, "visibility": "pub(crate)",
          "signature": "pub(crate) fn hash_password(raw: &str) -> String" }
      ]
    }
  ],
  "metadata": {
    "freshness_status": "fresh",
    "source_layer": "overlay"
  }
}
```

Why this matters:

- **Token savings**: ~100-200 tokens for outline vs. ~2000+ tokens for full file read.
- **Agent decision quality**: agent can pick exactly which function to read in detail,
  reducing wasted tool calls.
- **No competitor does this as an MCP tool**: Serena has similar capability but via LSP,
  not as a standalone MCP service.

Implementation:

- Query is strictly ref-scoped:
  `SELECT * FROM symbol_relations WHERE repo=? AND ref=? AND path=? ORDER BY line_start`.
- Build nested tree using `parent_symbol_id` relationships.
- If `depth="top"`, return only records where `parent_symbol_id IS NULL`.
- This query is purely SQLite — no Tantivy involvement, sub-millisecond latency.

### 10.7 Multi-workspace auto-discovery

Borrowed from Augment MCP's `--mcp-auto-workspace` pattern.

Problem: agents switch between projects during a session. A fixed single-repo MCP server
forces the user to restart or reconfigure between projects.

Design:

All tools that accept a query or path also accept an optional `workspace` parameter:

```jsonc
{
  "tool": "search_code",
  "input": {
    "query": "RateLimiter",
    "workspace": "/Users/dev/project-b"   // optional
  }
}
```

Behavior:

1. If `workspace` is omitted, use the default registered project (from `codecompass init`).
2. If `workspace` is provided and already indexed:
   - route the query to that project's indices.
3. If `workspace` is provided but **not yet indexed** and `--auto-workspace` is disabled:
   - reject request with explicit error (`workspace_not_registered`).
   - instruct user/agent to pre-register via CLI or restart with `--auto-workspace`.
4. If `workspace` is provided but **not yet indexed** and `--auto-workspace` is enabled:
   - normalize path (`realpath`) and verify it is under configured `--allowed-root` prefixes.
   - reject if outside allowlist (`workspace_not_allowed`).
   - register it in `known_workspaces` table (auto_discovered = 1).
   - trigger on-demand bootstrap indexing.
   - return results with `indexing_status: "indexing"` and `result_completeness: "partial"`.
   - subsequent queries will use the completed index.
5. `known_workspaces` tracks `last_used_at` for cleanup of stale auto-discovered workspaces.

MCP server startup:

- `serve-mcp` can accept `--workspace <path>` (repeatable) to pre-register multiple projects.
- `--auto-workspace` is **off by default** and must be explicitly enabled.
- If `--auto-workspace` is enabled, at least one `--allowed-root <path>` is required.
- This mirrors Augment's `auggie --mcp --mcp-auto-workspace` pattern.

Warmset policy (latency optimization):

- maintain a bounded `workspace_warmset` from recent `known_workspaces.last_used_at`
  (for example top 3-5 entries).
- prewarm only warmset workspaces during startup to keep boot fast while reducing
  first-query latency on active projects.
- expose warmset hit/miss metadata in `health_check` for operator visibility.

### 10.8 Ignore file support (`.codecompassignore`)

File filtering chain (applied in order, all additive):

1. **Built-in defaults**: skip binary files, lock files, common generated patterns.
   - Extensions: `.exe`, `.dll`, `.so`, `.dylib`, `.o`, `.a`, `.wasm`, `.pyc`, `.class`, `.jar`
   - Directories: `.git/`, `node_modules/`, `__pycache__/`, `.tox/`, `target/` (Rust), `build/`
   - Patterns: `*.min.js`, `*.min.css`, `*.generated.*`, `*.pb.go`, `*_generated.rs`
2. **`.gitignore`**: respected automatically in VCS mode.
3. **`.codecompassignore`**: additional patterns specific to code search indexing.
   Uses the same syntax as `.gitignore` (gitignore-style glob patterns).

Example `.codecompassignore`:

```gitignore
# Skip vendored code (already in .gitignore for some projects, explicit here for search)
vendor/
third_party/

# Skip generated protobuf
*.pb.go
*.pb.rs

# Skip test fixtures with large data files
testdata/fixtures/large/

# Skip documentation build output
docs/_build/
```

Load order: built-in defaults -> `.gitignore` -> `.codecompassignore` (union, not override).

CLI support:

- `codecompass doctor` reports which ignore rules are active and how many files are excluded.
- `codecompass index --show-ignored` lists files that would be skipped (dry-run mode).

### 10.9 MCP health and readiness

Borrowed from mcp-rag-server's `/health` endpoint and readiness pattern.

**HTTP transport mode** (`serve-mcp --transport http`):

```
GET /health

Response:
{
  "status": "ready",              // "ready" | "indexing" | "warming" | "error"
  "projects": [
    {
      "project_id": "backend",
      "repo_root": "/Users/dev/backend",
      "index_status": "ready",    // "ready" | "warming" | "indexing" | "error"
      "last_indexed_at": "2026-02-22T10:30:00Z",
      "ref": "main",
      "file_count": 3891
    }
  ],
  "version": "0.1.0",
  "uptime_seconds": 3600
}
```

Status meanings:

- `warming`: Tantivy index prewarming in progress (see [Section 10.11](#1011-tantivy-index-prewarming)). Queries may be slow.
- `indexing`: bootstrap or large incremental sync running. Queries return partial results.
- `ready`: all registered projects indexed and warm. Queries at full speed.
- `error`: a project has failed indexing. `projects[].index_status` shows which.

**stdio transport mode**:

Health info is included in the MCP `initialize` response `serverInfo` field
and available via the `health_check` tool.

### 10.10 Index progress notifications

Long-running index operations (bootstrap, large incremental sync) should report progress
to the MCP client so the agent knows when results will be available.

Uses the MCP notification protocol (server -> client, no response expected):

```jsonc
// Progress notification
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "index-job-abc123",
    "value": {
      "kind": "report",
      "title": "Indexing project: backend",
      "message": "Parsing files: 1247/3891 (32%)",
      "percentage": 32
    }
  }
}

// Completion notification
{
  "jsonrpc": "2.0",
  "method": "notifications/progress",
  "params": {
    "progressToken": "index-job-abc123",
    "value": {
      "kind": "end",
      "title": "Indexing complete: backend",
      "message": "Indexed 3891 files, 12847 symbols in 14.2s"
    }
  }
}
```

Progress tokens are also tracked in `index_jobs` table so `index_status` can
return the same information via tool call (for clients that don't support notifications).

Restart safety:

- if process restarts while jobs are active, unfinished jobs are marked
  `interrupted` during bootstrap reconciliation.
- `index_status` and `health_check` should include
  `interrupted_recovery_report` (recent interrupted jobs, affected workspaces,
  recommended remediation) until a successful subsequent sync clears it.

### 10.11 Tantivy index prewarming

Tantivy uses mmap for segment files. On cold start, the first queries pay page fault latency.

**Prewarming strategy on `serve-mcp` startup**:

```rust
// Pseudocode only (exact APIs depend on Tantivy version).
// After opening indices, before accepting MCP connections:
for project in registered_projects {
    let reader = project.index.reader()?;
    let searcher = reader.searcher();
    // Touch segment metadata and run tiny warm-up searches against hot fields.
    let _ = searcher.segment_readers().iter().map(|seg| seg.num_docs()).sum::<u32>();
    run_warmup_query(&searcher, "main");
    run_warmup_query(&searcher, "init");
    run_warmup_query(&searcher, "error");
}
```

Behavior:

- During prewarming, health status reports `"warming"`.
- Queries during warming are allowed but may have higher p95 latency.
- Prewarming is optional and can be disabled via `--no-prewarm` flag.
- `doctor` command reports whether indices are warm or cold.

Warm index benchmark target: symbol lookup p95 < 300ms (see [benchmark-targets.md](benchmark-targets.md)).
Cold index first-query target: < 2000ms (mmap page fault cost).

### 10.12 MCP-first, service-ready boundary (not MCP-only)

CodeCompass remains **MCP-first** for agent workflows, but architecture is intentionally
**service-ready** without introducing daemon complexity prematurely.

Interface layers:

1. CLI (`codecompass ...`) for humans and CI automation.
2. MCP (stdio + HTTP transport) as the primary agent integration surface.
3. Internal service boundary (query/index/status traits) that can be daemonized later if needed.

Tool-surface discipline (borrowed from VibeRAG):

- keep one primary retrieval entrypoint (`search_code` / `locate_symbol`),
- keep follow-up tools focused (`get_file_outline`, `get_code_context`, later `find_references`),
- return stable follow-up handles in results (`symbol_id` and `symbol_stable_id`) to reduce repeated broad queries.

Daemonization is explicitly deferred until one of these triggers is observed in real usage:

1. repeated multi-client stale-handle issues,
2. duplicate watchers/indexers consuming significant resources,
3. correctness drift between concurrent CLI and MCP operations on the same project.

When none of these triggers exist, the simpler single-process MCP model is preferred.

## 11. CLI UX (Rust binary)

```bash
codecompass init
codecompass index --workspace /path/to/repo
codecompass sync --workspace /path/to/repo
codecompass search "where is rate limiter implemented" --workspace /path/to/backend --lang go
codecompass serve-mcp --workspace /path/to/backend --project backend --allowed-root /Users/dev
codecompass doctor
codecompass state export --project backend --out .codecompass-state.tar.zst
codecompass state import --project backend --in .codecompass-state.tar.zst
```

## 12. Competitive Landscape and Backend Alternatives

The question is not "better globally", but "better for which objective".
This section captures both market references and backend tradeoffs.

### 12.1 Comparable projects and what to borrow

1. **Sourcegraph (Zoekt + SCIP)**
   - strongest off-the-shelf code intelligence (definition/reference/cross-repo navigation).
   - borrow: symbol graph quality bar, indexed metadata discipline.
   - avoid for v1: platform complexity and heavier operational footprint.

2. **Sourcebot**
   - self-hosted platform for code search + navigation + MCP.
   - borrow: "goto definition / find references" as MCP tool design pattern.
   - avoid for v1: heavier deployment model (multi-service).

3. **Serena**
   - symbol-level semantic retrieval and editing via MCP.
   - borrow: symbol-level precision over chunk-level, structured code entity model.
   - avoid for v1: LSP dependency; CodeCompass uses tree-sitter directly.

4. **VibeRAG**
   - intent-routed codebase search (definitions/files/blocks/usages) + LanceDB + watcher.
   - borrow: intent routing pattern, auto-index with .gitignore respect, agent-first design.
   - borrow: startup compatibility checks (`schema mismatch -> reindex required`) and compact MCP tool surface with stable follow-up handles.
   - borrow later (only with trigger): per-project daemon ownership for multi-client coordination.
   - avoid for v1: AGPL-3.0 copyleft; TypeScript/Node stack; vector-first retrieval.

5. **Augment Context Engine MCP**
   - commercial reference for "context retrieval for coding agents".
   - borrow: single-tool MCP simplicity (codebase-retrieval), auto-workspace discovery,
     local/remote mode split, intelligent context trimming concept.
   - avoid: closed-source, per-call pricing, external service dependency.

6. **Elastic Semantic Code Search**
   - dual-index model (chunks + locations join) for precise file:line responses.
   - borrow: the join pattern for "snippets match content, symbols provide location".
   - avoid for v1: SSPL license, Elasticsearch dependency, heavy operational model.

7. **OpenGrok**
   - proven enterprise code search with strong cross-reference workflows.
   - borrow: stable indexing pipeline and conservative operation model.
   - avoid for v1: lower MCP-native ergonomics for AI tool orchestration.

8. **ast-grep / tree-sitter query tools**
   - precise structural matching with syntax awareness.
   - borrow: AST-powered extraction quality and language-aware heuristics.
   - avoid for v1: not a complete retrieval engine by itself.

9. **mcp-local-rag**
   - local MCP retrieval with chunk + embedding + vector path.
   - borrow: clear MCP tool boundaries, staged ingestion flow, keyword boost over vectors.
   - avoid for v1: vector-first complexity/cost when symbol precision is primary.

10. **mcp-rag-server**
    - local MCP RAG with security-conscious design.
    - borrow: REPO_ROOT path constraint, DNS rebinding protection, /health readiness endpoint.
    - avoid for v1: chunk-only retrieval, no symbol awareness.

11. **meilisearch-mcp**
    - good MCP adapter pattern around backend APIs.
    - borrow: clean MCP server shape and operational tooling coverage.
    - avoid for v1: it is not a full code-intelligence product architecture.

12. **GitHub MCP Server (official)**
    - platform-level repo/PR/Issue/Actions context via MCP.
    - borrow: complementary "remote context" pattern; CodeCompass handles local, GitHub MCP handles platform.
    - avoid as replacement: no semantic search, no local index, no branch-aware search.

### 12.2 Positioning of CodeCompass

CodeCompass should position itself as:

- **local-first, AI-native code locator** (MCP-first integration),
- **branch/worktree-correct search engine** (hard requirement, key differentiator),
- **lexical + structural precision-first tool** for coding workflows,
- **single-binary distributable developer utility** with low operational friction,
- **extensible retrieval platform** (semantic/rerank optional, not mandatory).

This positioning fills a practical gap:

- many tools are either enterprise-heavy (excellent but operationally expensive),
- or vector/remote-first (powerful but costly/complex),
- or text-fast but not branch/worktree precise for AI orchestration.

### 12.3 Backend alternatives (beyond default)

#### Option A: Tantivy embedded (default choice)

- Pros:
  - Rust native, compiles into the binary, zero external service.
  - Full BM25 with custom tokenizers, faceting, and filtering.
  - Segment model maps naturally to branch overlay architecture.
  - Sub-millisecond warm query latency for symbol lookups.
- Cons:
  - Less turnkey than Meilisearch for rapid prototyping.
  - No built-in HTTP API (but CodeCompass provides MCP, so not needed).
  - Vector search support is experimental/planned (use the embedded vector layer
    with optional adapter enablement when needed).

#### Option B: Meilisearch (alternative for hosted/multi-tenant scenarios)

- Pros:
  - simple ops and fast iteration,
  - strong lexical search and filtering,
  - built-in hybrid search with embedder support,
  - HTTP API suitable for multi-service architectures.
- Cons:
  - requires separate process (breaks single-binary goal),
  - branch overlay requires separate index names and API routing (more complex),
  - not a full code-intelligence platform by itself.

#### Option C: Sourcegraph stack (Zoekt + SCIP)

- Pros:
  - strongest symbol/definition/reference navigation out of the box,
  - mature code-intelligence model.
- Cons:
  - heavier platform and integration complexity,
  - less flexible as a lightweight embeddable component.

#### Option D: Elasticsearch/OpenSearch

- Pros:
  - deep query DSL, advanced ranking controls, mature enterprise ops.
- Cons:
  - higher operational complexity and tuning overhead for small teams.

#### Option E: Embedded vector backend (local-first, adapter-pluggable)

- Pros:
  - embedded, no external service, keeps single-binary workflow.
  - backend adapter flexibility (default local segment/table, optional LanceDB).
  - Strong for semantic/hybrid search when needed in v2+.
- Cons:
  - vector-first retrieval is weaker for symbol precision queries.
  - adds embedding model dependency (local or API).
- Decision: use as optional additive layer for NL queries, not as primary backend.

#### Option F: Vector-as-a-service (Qdrant/Weaviate/Pinecone)

- Pros:
  - strong vector-native workflows, mature hosted options.
- Cons:
  - external service dependency (breaks single-binary goal),
  - weaker fit when code navigation precision is lexical/symbol first,
  - adds operational cost and network latency.
- Decision: not recommended. If vector search is needed, use local embedded backend first.

### 12.4 Decision recommendation

- Use Tantivy embedded for v1-v2 (default).
- Maintain backend abstraction trait so alternatives can be swapped.
- Re-evaluate after benchmark:
  - if hosted/multi-tenant is needed, Meilisearch is the first alternative to evaluate.
  - if code-intelligence depth is lacking, add SCIP pipeline or evaluate Sourcegraph integration path.
  - if NL semantic recall is insufficient with Tantivy alone, enable the embedded vector layer (LanceDB adapter optional).
  - Qdrant/external vector services: only if true multi-tenant scale demands it.

## 13. Risk Register

1. Symbol extraction quality drift across languages
   -> staged parser roadmap with language-specific fixtures.

2. Rerank provider instability/cost
   -> optional provider use + local rule fallback.

3. Index growth and stale data
   -> retention rules, periodic compaction, consistency checks.

4. Backend lock-in
   -> storage/query abstraction interfaces from day one.

5. Branch index drift after rebase/force-push
   -> ancestry check + overlay rebuild from merge-base.

6. Partial write inconsistency during sync
   -> staged `sync_id` + rollbackable two-phase publish.

7. Tantivy API stability and feature gaps
   -> Tantivy is mature (v0.22+) but not 1.0. Pin version, wrap behind abstraction trait.
   -> If native vector support is delayed, rely on the local vector layer and keep
      adapter fallback optional.

8. Symbol relation graph accuracy (call edges, import edges)
   -> Start with `parent_symbol_id` only (tree-sitter scope nesting, high confidence).
   -> Add import/call edges incrementally with per-language test fixtures.
   -> Tag edges with `confidence: static | heuristic` to let consumers decide trust level.

9. Token budget estimation accuracy
   -> Use word-count heuristic: `estimated_tokens = ceil(whitespace_split_word_count * 1.3)`.
   -> Always undershoot rather than overshoot the budget.
   -> Expose `estimated_tokens` in metadata so agents can calibrate.

10. Tantivy segment proliferation from many branch overlays
    -> Set overlay eviction policy (prune overlays for branches inactive > N days).
    -> Monitor total segment count per project.
    -> Force-merge segments on `sync --force` or periodic reconcile.

11. MCP security (prompt injection, tool chain RCE)
    -> Match mcp-rag-server baseline: REPO_ROOT constraint, no shell execution from tool params.
    -> Validate all path inputs against repo root allowlist.
    -> Reject symlink escape outside repo root.
    -> Log all tool invocations for audit.

12. Multi-workspace auto-discovery abuse
    -> Rate-limit on-demand indexing (max N concurrent bootstrap jobs).
    -> Require explicit `--auto-workspace` plus mandatory `--allowed-root`.
    -> Validate workspace paths against allowlist after `realpath` normalization.
    -> Auto-discovered workspaces evicted after configurable inactivity period.

13. Symbol identity churn on line movement
    -> Use `symbol_stable_id` for diff and graph continuity.
    -> Treat `line_start` as location metadata, not identity.

14. `.codecompassignore` inconsistency with `.gitignore`
    -> Use the same glob parsing library for both (e.g., `ignore` crate in Rust).
    -> `doctor` command reports effective ignore rules for verification.

15. Tantivy prewarm latency on large indices
    -> Prewarm only critical fields (`symbol_exact`, `qualified_name`, fast fields).
    -> Allow `--no-prewarm` flag for environments where cold start is acceptable.
    -> Monitor prewarm duration and report in `doctor`.

16. Long-lived process memory drift (parser/search/index loops)
    -> enforce deterministic lifecycle ownership for parse trees and storage handles.
    -> cap in-memory failure history and large transient buffers.
    -> add memory regression tests to CI for repeated index/search cycles.

## 14. Security Model (Implementation Guardrails)

1. Path and repo boundary controls
   - enforce repository root allowlist.
   - `workspace` parameters must pass `realpath` and stay inside configured `allowed_roots`.
   - reject path traversal (`..`) and symlink escape outside repo root.

2. Input safety controls
   - max file size and max line length limits.
   - file extension allowlist and generated/binary file skip rules.

3. MCP execution safety
   - strict tool input validation and explicit error codes.
   - no shell execution from tool parameters.

4. Secret and key hygiene
   - load provider keys from env or secret manager only.
   - never log API keys or full sensitive payloads.

## 15. Schema Versioning and Migration Plan

- add fields:
  - `schema_version`,
  - `parser_version`,
  - `rank_profile_version`,
  - `stable_id_version`,
  - `content_hash`.
- migration policy:
  - minor schema update: additive fields + background backfill.
  - major schema update: create new index set, backfill, then cut over.
- maintain migration command:
  - `codecompass migrate-index --from vX --to vY`.

### 15.1 Startup compatibility checks and reindex gate

At startup (CLI + MCP), CodeCompass should run a lightweight compatibility check:

- compare persisted `schema_version` with the binary's required version,
- validate index manifest readability,
- classify state as:
  - `compatible`,
  - `not_indexed`,
  - `reindex_required`,
  - `corrupt_manifest`.

Behavior contract:

- if `reindex_required` or `corrupt_manifest`, query tools return an explicit actionable error
  (`index_incompatible`) with remediation (`codecompass index --force`),
- incremental sync refuses to proceed until compatibility is restored,
- status surfaces (`index_status`, `health_check`) include required and current schema versions.

## 16. Open Questions

### Resolved

> These questions have been resolved during design and spec creation. Kept for traceability.

1. **v1 first-class indexed source languages**: Go/TypeScript/Python only or more?
   -- **RESOLVED**: Rust, Go, TypeScript, Python as initial language set. Additional languages added incrementally via tree-sitter grammars.

2. **Definition-only in strict v1, or include references in v1.5?**
   -- **RESOLVED**: Definitions-only in v1. References via `symbol_edges` in v1.5+.

3. **Single-repo first or workspace multi-repo from day one?**
   -- **RESOLVED**: Single-repo first (v1). Multi-workspace auto-discovery in v1.5 (see [Section 10.7](#107-multi-workspace-auto-discovery)).

4. **Is semantic/hybrid enabled by default for NL queries?**
   -- **RESOLVED**: No. Lexical-first for all query types in v1. Semantic/hybrid is opt-in via feature flag in v2+ (see [Section 7.2](#72-v2-optional-hybrid-semantic)).

6. **What benchmark set and acceptance thresholds define "better backend" for us?**
   -- **RESOLVED**: See [benchmark-targets.md](benchmark-targets.md) for acceptance criteria.

8. **Tantivy custom tokenizer scope**: implement all four in Phase 0 or start with two?
   -- **RESOLVED**: All four tokenizers (`code_camel`, `code_snake`, `code_dotted`, `code_path`) implemented in Phase 0. They are small, self-contained, and required for schema setup.

9. **Symbol relation edges**: should `imports` edges be stored in Tantivy or SQLite only?
   -- **RESOLVED**: SQLite only. Import edges are used for graph navigation, not full-text retrieval.

10. **`diff_context` tool**: should it support diffing between arbitrary commits, or only ref-vs-base?
    -- **RESOLVED**: ref-vs-base only for v1. Arbitrary commit diffing deferred to v2+.

12. **Token budget**: default `max_tokens` for `get_code_context` when agent doesn't specify?
    -- **RESOLVED**: 4000 tokens (conservative, fits most agent context windows).

13. **Should `get_file_outline` return the full file skeleton or limit depth?**
    -- **RESOLVED**: Support `depth: "top" | "all"` parameter, default `"all"`.

14. **Multi-workspace**: max number of concurrently indexed auto-discovered workspaces?
    -- **RESOLVED**: 10 (prevent runaway resource usage). Configurable via `--max-auto-workspaces`.

15. **Multi-workspace**: auto-eviction period for unused workspaces?
    -- **RESOLVED**: 7 days since `last_used_at`. Configurable via `--workspace-eviction-days`.

16. **`.codecompassignore`**: should it support `!` (negation/re-include) patterns?
    -- **RESOLVED**: Yes, same as `.gitignore` semantics. The `ignore` crate handles this natively.

17. **Prewarm**: should it be blocking or non-blocking?
    -- **RESOLVED**: Blocking with status `"warming"` reported via health. Configurable via `--no-prewarm` flag.

### Open

5. **Which rerank provider is first: Cohere or Voyage?**
   -- Decision deferred to Phase 3 implementation.

7. **Should we add an early SCIP export/import bridge for future Sourcegraph compatibility?**
   -- Low priority for v1-v2. Re-evaluate when symbol graph is mature.

11. **Should `stable_id_version=2` add AST-shape fingerprint after v1 is stable?**
    -- Deferred to post-v1 evaluation. Depends on observed identity churn rates.
