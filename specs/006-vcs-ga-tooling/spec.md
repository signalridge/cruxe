# Feature Specification: VCS GA Tooling & Portability

**Feature Branch**: `006-vcs-ga-tooling`
**Created**: 2026-02-23
**Status**: Draft
**Version**: v1.0.0 (GA gate completion)
**Depends On**: 005-vcs-core
**Input**: `specs/meta/design.md` Section 9.9-9.10 and Section 10.5; VCS GA tooling scope split from legacy 005 spec

## Overview

This spec completes VCS GA by adding advanced VCS tool surface and state
portability on top of the correctness foundation provided by `005-vcs-core`.

It delivers:

- branch comparison tooling (`diff_context`)
- graph-based symbol reference tooling (`find_references`)
- deterministic ranking introspection (`explain_ranking`)
- ref lifecycle helpers (`list_refs`, `switch_ref`)
- portable state export/import and overlay maintenance commands

## Readiness Baseline Update (2026-02-25)

- `symbol_edges` now has composite forward/reverse type indexes and query-shape
  regression tests to support low-latency `find_references`/graph traversals.
- Runtime SQLite handle management is now explicitly bounded to avoid
  unbounded file-descriptor growth in long-lived multi-workspace servers.
- High-fanout MCP fixture tests now use configurable bounded parallelism
  (`CODECOMPASS_TEST_FIXTURE_PARALLELISM`) instead of global serial execution.
- Cross-process maintenance lock (parent-scoped
  `locks/state-maintenance-<path-hash>.lock`) now coordinates destructive state
  mutations across import/prune/sync publish paths.

## User Scenarios & Testing

### User Story 1 - Diff Context for PR Review (Priority: P1)

An AI coding agent (or developer) calls `diff_context` with two ref names
(e.g., `base: "main"`, `head: "feat/auth"`) to get a symbol-level summary of
what changed between branches. The system computes git diff, classifies each
affected symbol as added, modified, or deleted, and returns a structured
`DiffContextResult`. The agent uses this to explain a pull request without
reading full file diffs, significantly reducing token consumption.

**Why this priority**: Symbol-aware diff is the highest-value VCS GA tool.
Agents reviewing PRs need structured change summaries, not raw diffs. This tool
directly enables agentic code review workflows.

**Independent Test**: Index two branches of a fixture repository where one branch
adds a function, modifies a struct, and deletes a constant. Call `diff_context`
and verify the result correctly classifies each symbol change.

**Acceptance Scenarios**:

1. **Given** branches `main` and `feat/auth` where `feat/auth` adds function
   `validate_token`, **When** `diff_context` is called with
   `base: "main", head: "feat/auth"`, **Then** the result includes an entry with
   `symbol: "validate_token"`, `kind: "function"`, `change_type: "added"`, and
   the file path where it was added.
2. **Given** a branch that modifies the body of struct `Config` (adds a field),
   **When** `diff_context` is called, **Then** the result includes
   `symbol: "Config"`, `change_type: "modified"` with the affected file and line
   range.
3. **Given** a branch that deletes constant `MAX_RETRIES`, **When**
   `diff_context` is called, **Then** the result includes
   `symbol: "MAX_RETRIES"`, `change_type: "deleted"`.
4. **Given** a file that was renamed without symbol changes, **When**
   `diff_context` is called, **Then** the rename is reported at the file level
   and no symbol-level changes are emitted for that file.
5. **Given** an unindexed ref passed as `head`, **When** `diff_context` is
   called, **Then** the tool returns an error with remediation guidance
   (e.g., "run sync_repo for ref 'feat/new' first") and does not block other
   search workflows.

---

### User Story 2 - Find References via Symbol Graph (Priority: P1)

A developer or AI agent calls `find_references` with a symbol name and optional
ref scope. The system traverses indexed `symbol_edges` relation edges to find
all locations where the symbol is referenced (imported, called, or used) within
the specified ref. Results include file path, line range, reference kind, and
`source_layer` metadata. This enables code navigation without reading entire
files.

**Why this priority**: Reference lookup is essential for understanding symbol
usage across a codebase. Without it, agents must fall back to grep-style
searches that miss semantic relationships.

**Independent Test**: Index a fixture repository with a function that is imported
in three files. Call `find_references` and verify all three import sites are
returned with correct file paths and edge types.

**Acceptance Scenarios**:

1. **Given** a function `validate_token` that is imported in three files,
   **When** `find_references` is called with
   `symbol_name: "validate_token", ref: "main"`, **Then** the result contains
   three `ReferenceResult` entries, each with `file_path`, `line_start`,
   `line_end`, and `edge_type: "imports"`.
2. **Given** a struct `Claims` with both import edges and call-site edges,
   **When** `find_references` is called, **Then** all stored relation edges for
   that symbol in the given ref are returned.
3. **Given** `ref: "feat/auth"` where a new reference was added in the overlay,
   **When** `find_references` is called, **Then** the overlay reference is
   included alongside base references, and each result carries `source_layer`
   (`base` or `overlay`) metadata.
4. **Given** a symbol name that does not exist in the index, **When**
   `find_references` is called, **Then** a tool-level `symbol_not_found` error
   is returned with remediation metadata.
5. **Given** a tooling failure in `find_references` (e.g., corrupted edge data),
   **When** the tool is called, **Then** the failure is returned as a tool-level
   error and does not degrade `search_code` or `locate_symbol` availability.

---

### User Story 3 - Explain Ranking for Debug (Priority: P2)

A maintainer investigating unexpected search result ordering calls
`explain_ranking` with a query and a specific result entry. The system returns
a `RankingExplanation` showing deterministic scoring components (`bm25`,
`exact_match`, `qualified_name`, `path_affinity`, `definition_boost`,
`kind_match`, `total`) plus per-component explanation strings. This allows
maintainers to diagnose and tune search quality.

**Why this priority**: Without ranking transparency, debugging search quality
issues requires guesswork. This tool provides the introspection needed to
maintain and improve search relevance over time.

**Independent Test**: Run the same query twice against the same index state.
Call `explain_ranking` for the same result both times. Verify the scoring
breakdown is byte-identical across invocations.

**Acceptance Scenarios**:

1. **Given** a query `"validate_token"` that returns a ranked result list,
   **When** `explain_ranking` is called with the query and one result entry,
   **Then** the response contains a `RankingExplanation` with fields for
   `bm25`, `exact_match`, `qualified_name`, `path_affinity`,
   `definition_boost`, `kind_match`, and `total`.
2. **Given** the same query and index state, **When** `explain_ranking` is
   called twice, **Then** both responses produce identical scoring values
   (deterministic output).
3. **Given** a query where the top result is a definition, **When**
   `explain_ranking` is called, **Then** the `definition_boost` component is
   non-zero and its contribution to the final rank is visible.
4. **Given** a result entry that does not belong to the query's result set,
   **When** `explain_ranking` is called, **Then** a clear error is returned
   indicating the result was not found for the given query.

---

### User Story 4 - List and Switch Refs (Priority: P2)

An AI agent calls `list_refs` to discover all indexed refs for the current
project, receiving a list of `RefDescriptor` entries with ref name, indexed
commit hash, status, and counts. The agent then calls `switch_ref` to set the
active ref scope for subsequent queries. These tools provide predictable ref
lifecycle management for multi-branch workflows.

**Why this priority**: Agents operating on multi-branch repositories need to
discover which refs are available and switch context without guessing. These
helpers eliminate ref-related errors in agentic workflows.

**Independent Test**: Index a fixture repository on two branches (`main` and
`feat/auth`). Call `list_refs` and verify both appear. Call `switch_ref` to
`feat/auth`, then run a search and verify results are scoped to that ref.

**Acceptance Scenarios**:

1. **Given** a project with refs `main` and `feat/auth` indexed, **When**
   `list_refs` is called, **Then** the response contains two `RefDescriptor`
   entries, each with `ref`, `last_indexed_commit`, and `status` fields.
2. **Given** `list_refs` returns both `main` and `feat/auth`, **When** the
   agent reads the response metadata, **Then** protocol-level freshness is
   exposed in `metadata.freshness_status` and can be interpreted independently
   of per-ref status.
3. **Given** `switch_ref` is called with `ref: "feat/auth"`, **When** a
   subsequent `search_code` call is made, **Then** results are scoped to the
   `feat/auth` ref.
4. **Given** `switch_ref` is called with a ref name that has not been indexed,
   **When** the tool executes, **Then** it returns an error with clear
   remediation guidance (e.g., "ref 'feat/new' is not indexed; run sync_repo
   with ref='feat/new' first") and does not change the active ref.
5. **Given** `switch_ref` is called with the ref that is already active,
   **When** the tool executes, **Then** it succeeds idempotently without error.

---

### User Story 5 - Portable State Export/Import (Priority: P2)

A platform engineer runs `codecompass state export` to create a
`PortableStateBundle` archive containing the SQLite database, Tantivy index
artifacts, and version metadata. The bundle is transferred to an ephemeral CI
runner where `codecompass state import` restores full searchable state without
re-indexing. An overlay prune maintenance command cleans up stale overlays while
respecting active worktree leases.

**Why this priority**: Without state portability, every CI run or ephemeral
environment must re-index from scratch, wasting minutes of compute. Export/import
enables instant code intelligence in transient environments.

**Independent Test**: Index a fixture repository, export state, delete all local
index artifacts, import the bundle, then run a search query and verify results
match the pre-export state.

**Acceptance Scenarios**:

1. **Given** a fully indexed project, **When** `codecompass state export` is run,
   **Then** a `PortableStateBundle` archive is created containing SQLite state,
   Tantivy index directories, and a metadata manifest with schema version and
   export timestamp.
2. **Given** a valid `PortableStateBundle`, **When** `codecompass state import`
   is run in a clean environment, **Then** the SQLite database and Tantivy
   indices are restored, and subsequent `search_code` queries return the same
   results as on the exporting machine.
3. **Given** a bundle exported with schema version N, **When** import is
   attempted on a binary with schema version N-1, **Then** the import fails with
   a clear version mismatch error and does not corrupt local state.
4. **Given** an imported state bundle that is one commit behind HEAD, **When**
   `sync_repo` is run, **Then** delta recovery succeeds by incrementally syncing
   only the files changed since the exported commit, without requiring a full
   re-index.
5. **Given** a project with stale overlay indices from deleted branches, **When**
   `codecompass prune-overlays` is run, **Then** overlays without matching
   worktree leases are removed, but overlays with active leases are preserved.
6. **Given** a `state export` or `state import` failure (e.g., disk full),
   **When** the command fails, **Then** the error is reported cleanly and
   existing local index state is not corrupted.

### Edge Cases

- Tool-specific failures (e.g., in `diff_context` or `find_references`) must not
  block base search workflows (`search_code`, `locate_symbol`).
- `switch_ref` to an unindexed ref must fail with clear remediation guidance and
  must not change the active ref.
- Importing stale state must permit delta recovery through the next `sync_repo`
  invocation without requiring a full re-index.
- Prune operations must avoid deleting active overlays that have worktree leases.
- `diff_context` on two identical refs must return an empty change set, not an
  error.
- `explain_ranking` must remain deterministic even when called concurrently for
  different queries.
- Multi-workspace MCP servers must remain stable under sustained request
  concurrency without exhausting process file descriptors.
- Concurrent state-mutating operations (`state import`, `prune-overlays`,
  overlay publish during `sync_repo`) must not run simultaneously for the same
  project data directory.

## Requirements

### Functional Requirements

- **FR-500**: System MUST provide `diff_context` with symbol-level added/modified/deleted classification.
- **FR-501**: System MUST provide `find_references` based on indexed symbol relation edges.
- **FR-502**: System MUST provide deterministic `explain_ranking` breakdowns for given result context.
- **FR-503**: System MUST provide `list_refs` for indexed refs with freshness/state metadata.
- **FR-504**: System MUST provide `switch_ref` helper with validation and safe error semantics.
- **FR-505**: System MUST provide `state export` and `state import` CLI commands.
- **FR-506**: System MUST preserve schema/version safety checks during import.
- **FR-507**: System MUST register all VCS GA tooling in MCP `tools/list`.
- **FR-508**: System MUST provide overlay prune maintenance command with lease-awareness.
- **FR-509**: Tooling failures MUST degrade gracefully and preserve core query availability.
- **FR-510**: MCP runtime MUST bound cached SQLite connections using an idle
  eviction strategy so connection cache growth is finite under multi-workspace
  traffic.
- **FR-511**: MCP regression tests that build fixture indices MUST support
  configurable bounded parallelism and MUST NOT require fully serial test
  execution for stability.
- **FR-512**: State-mutating maintenance operations MUST acquire a per-project
  cross-process lock file and fail fast with retryable guidance when the lock
  is already held. The lock file location MUST remain stable across state data
  directory rename-swap operations (for example import commit/rollback).

### Key Entities

- **DiffContextResult**: Symbol-level change set between refs.
- **ReferenceResult**: Ref-scoped symbol usage result from graph edges.
- **RankingExplanation**: Deterministic scoring component map for one result.
- **RefDescriptor**: Indexed ref metadata (`name`, `indexed_commit`, freshness).
- **PortableStateBundle**: Exportable archive with SQLite + index artifacts + metadata.

## Success Criteria

### Measurable Outcomes

- **SC-500**: `diff_context` returns correct symbol-level summaries on fixture branches.
- **SC-501**: `find_references` returns all stored relation edges for fixture symbols.
- **SC-502**: `explain_ranking` output is deterministic for same query/index state.
- **SC-503**: `list_refs`/`switch_ref` operate correctly across multi-ref indexed fixtures.
- **SC-504**: Export/import roundtrip restores searchable state equivalence.
- **SC-505**: Tooling layer passes end-to-end validation and unlocks v1.0.0 GA labeling.
