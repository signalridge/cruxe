## Why

The VCS core foundation (005-vcs-core) provides overlay correctness and branch-aware indexing, but lacks the advanced tool surface and state portability needed for GA readiness. AI coding agents need symbol-level diff context for PR review, graph-based reference lookup for code navigation, ranking introspection for search quality debugging, and ref lifecycle helpers for multi-branch workflows. Platform engineers need portable state export/import to avoid re-indexing in ephemeral CI environments.

## What Changes

1. Add branch comparison tooling (`diff_context`) with symbol-level added/modified/deleted classification.
2. Add graph-based symbol reference tooling (`find_references`) via indexed `symbol_edges`.
3. Add deterministic ranking introspection (`explain_ranking`) with per-component scoring breakdowns.
4. Add ref lifecycle helpers (`list_refs`, `switch_ref`) for multi-branch agent workflows.
5. Add portable state export/import CLI commands and overlay prune maintenance.
6. Add bounded runtime SQLite connection cache, configurable fixture test parallelism, and cross-process maintenance locking.

## Capabilities

### New Capabilities

- **diff_context** (US1, P1): Symbol-level change set between two refs. Computes merge-base, classifies affected symbols as added/modified/deleted, returns structured `DiffContextResult` with stable handles. Enables agentic code review without reading full file diffs.
  - FR-500: System MUST provide `diff_context` with symbol-level added/modified/deleted classification.

- **find_references** (US2, P1): Ref-scoped symbol usage lookup via `symbol_edges` relation edges. Returns file path, line range, reference kind, and `source_layer` metadata. Enables code navigation without reading entire files.
  - FR-501: System MUST provide `find_references` based on indexed symbol relation edges.

- **explain_ranking** (US3, P2): Deterministic scoring component breakdown (`bm25`, `exact_match`, `qualified_name`, `path_affinity`, `definition_boost`, `kind_match`, `total`) with per-component explanation strings. Enables search quality diagnosis and tuning.
  - FR-502: System MUST provide deterministic `explain_ranking` breakdowns for given result context.

- **list_refs / switch_ref** (US4, P2): Ref lifecycle management. `list_refs` returns `RefDescriptor` entries with ref name, indexed commit, status, and counts. `switch_ref` sets active ref scope with validation and safe error semantics.
  - FR-503: System MUST provide `list_refs` for indexed refs with freshness/state metadata.
  - FR-504: System MUST provide `switch_ref` helper with validation and safe error semantics.

- **state export / state import** (US5, P2): CLI commands for portable `PortableStateBundle` archives containing SQLite database, Tantivy index artifacts, and version metadata. Enables instant code intelligence in transient environments.
  - FR-505: System MUST provide `state export` and `state import` CLI commands.
  - FR-506: System MUST preserve schema/version safety checks during import.

- **prune-overlays** (US5): Overlay maintenance command that removes stale overlays while respecting active worktree leases.
  - FR-508: System MUST provide overlay prune maintenance command with lease-awareness.

- **Cross-process maintenance lock**: Per-project lock file for state-mutating maintenance operations (import, prune, overlay publish). Parent-scoped stable lock path safe across import rename-swap.
  - FR-512: State-mutating maintenance operations MUST acquire a per-project cross-process lock file and fail fast with retryable guidance when the lock is already held.

### Modified Capabilities

- **MCP tool registry**: All 5 new VCS GA tools registered in `tools/list`.
  - FR-507: System MUST register all VCS GA tooling in MCP `tools/list`.

- **MCP runtime**: Bounded cached SQLite connections with idle LRU eviction; configurable fixture test parallelism replacing fully serial execution.
  - FR-509: Tooling failures MUST degrade gracefully and preserve core query availability.
  - FR-510: MCP runtime MUST bound cached SQLite connections using an idle eviction strategy so connection cache growth is finite under multi-workspace traffic.
  - FR-511: MCP regression tests that build fixture indices MUST support configurable bounded parallelism and MUST NOT require fully serial test execution for stability.

### Key Entities

- **DiffContextResult**: Symbol-level change set between refs.
- **ReferenceResult**: Ref-scoped symbol usage result from graph edges.
- **RankingExplanation**: Deterministic scoring component map for one result.
- **RefDescriptor**: Indexed ref metadata (`name`, `indexed_commit`, freshness).
- **PortableStateBundle**: Exportable archive with SQLite + index artifacts + metadata.

## Impact

- SC-500: `diff_context` returns correct symbol-level summaries on fixture branches.
- SC-501: `find_references` returns all stored relation edges for fixture symbols.
- SC-502: `explain_ranking` output is deterministic for same query/index state.
- SC-503: `list_refs`/`switch_ref` operate correctly across multi-ref indexed fixtures.
- SC-504: Export/import roundtrip restores searchable state equivalence.
- SC-505: Tooling layer passes end-to-end validation and unlocks v1.0.0 GA labeling.

### Edge Cases

- Tool-specific failures (e.g., in `diff_context` or `find_references`) must not block base search workflows (`search_code`, `locate_symbol`).
- `switch_ref` to an unindexed ref must fail with clear remediation guidance and must not change the active ref.
- Importing stale state must permit delta recovery through the next `sync_repo` invocation without requiring a full re-index.
- Prune operations must avoid deleting active overlays that have worktree leases.
- `diff_context` on two identical refs must return an empty change set, not an error.
- `explain_ranking` must remain deterministic even when called concurrently for different queries.
- Multi-workspace MCP servers must remain stable under sustained request concurrency without exhausting process file descriptors.
- Concurrent state-mutating operations (`state import`, `prune-overlays`, overlay publish during `sync_repo`) must not run simultaneously for the same project data directory.

### Affected Crates

- `cruxe-query`: `diff_context`, `find_references`, `explain_ranking`
- `cruxe-mcp`: tool handlers, `tools/list` registration, connection cache, fixture parallelism
- `cruxe-state`: export/import flows, maintenance lock
- `cruxe-cli`: `state export`, `state import`, `prune-overlays` commands
- `cruxe-indexer`: maintenance lock application on overlay publish

### API Impact

- Additive: 5 new MCP tools, 2 new CLI commands, 1 maintenance CLI command.
- No breaking changes to existing tools or CLI commands.

### Performance Impact

- Bounded SQLite connection cache prevents unbounded FD growth under multi-workspace traffic.
- Configurable fixture test parallelism improves test throughput without sacrificing stability.
- State import enables instant code intelligence in ephemeral environments (avoids re-indexing).

### Readiness Baseline Notes (2026-02-25)

- `symbol_edges` now has composite forward/reverse type indexes and query-shape regression tests to support low-latency `find_references`/graph traversals.
- Runtime SQLite handle management is now explicitly bounded to avoid unbounded file-descriptor growth in long-lived multi-workspace servers.
- High-fanout MCP fixture tests now use configurable bounded parallelism (`CRUXE_TEST_FIXTURE_PARALLELISM`) instead of global serial execution.
- Cross-process maintenance lock (parent-scoped `locks/state-maintenance-<path-hash>.lock`) now coordinates destructive state mutations across import/prune/sync publish paths.
