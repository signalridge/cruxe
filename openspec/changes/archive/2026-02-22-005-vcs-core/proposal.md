## Why

The Cruxe codebase needs a correctness-critical VCS foundation before adding the full GA tool surface. Without branch overlay indexing, incremental sync, and ancestry-break recovery, branch-scoped search is either impractical (full re-index on every sync) or silently incorrect (stale overlays after rebase/force-push). This spec isolates and delivers deterministic, ref-correct behavior as a standalone milestone — branch overlay indexing, incremental git-diff sync, rebase/force-push recovery, query-time base+overlay merge, and safe MCP routing of core search tools (`search_code`, `locate_symbol`) — before layering advanced VCS analysis tools.

All scoped FRs are implemented and all scoped SCs are covered by passing tests. 005 is closed as a merge-candidate spec. A `VcsAdapter` trait boundary was added in `cruxe-core::vcs_adapter` with a default Git-backed adapter implementation (FR-410 direction), along with a shared `OverlayMergeKey` domain type to avoid ad-hoc tuple/string merge-key drift.

## What Changes

1. Git-diff incremental sync: compute `git diff --name-status` against the merge-base, identify only changed files, update overlay index incrementally with per-file atomic updates, rename-as-delete+add handling, and rollback on failure (US1).
2. Branch overlay indexing: maintain separate, isolated overlay index directories per ref with an immutable shared base index; zero cross-ref leakage between branches (US2).
3. Rebase and force-push recovery: detect ancestry breaks via commit lineage check, discard stale overlay, recompute merge-base, and rebuild overlay from scratch without touching the base index (US3).
4. Query-time base+overlay merge: parallel search of base and overlay indices, merge by canonical merge key with overlay-wins precedence, tombstone suppression for deleted files (US4).
5. VCS adapter abstraction: `VcsAdapter` trait isolating all VCS operations (diff, merge-base, ancestry, ref listing, worktree) behind a clean boundary with Git2 as default implementation (US5).
6. Worktree manager: lease-based worktree lifecycle with persisted refcount state in SQLite, concurrent-access prevention, and orphaned-lease cleanup (US6).
7. Core MCP routing and metadata: route `search_code` and `locate_symbol` through merged VCS query path with `source_layer` provenance metadata on every result (US7).

## Capabilities

### New Capabilities

- **FR-400**: System MUST detect changed files via git diff name-status against merge-base.
- **FR-401**: System MUST treat rename as delete-old + add-new in overlay/tombstone logic.
- **FR-402**: System MUST perform per-file atomic updates; no partial file state may publish.
- **FR-403**: System MUST maintain separate overlay index directories per ref.
- **FR-404**: System MUST preserve an immutable base index for default branch operations.
- **FR-405**: System MUST track per-ref tombstones and suppress matching base paths at query time.
- **FR-406**: System MUST detect ancestry breaks and rebuild overlay from new merge-base.
- **FR-407**: System MUST merge base+overlay results by canonical merge key with overlay-wins precedence.
- **FR-408**: System MUST add `source_layer` (`base` | `overlay`) to VCS-mode search/locate results.
- **FR-409**: System MUST enforce single-active-sync-per-ref through `index_jobs` coordination.
- **FR-410**: System MUST provide `VcsAdapter` and a Git2 implementation.
- **FR-411**: System MUST manage worktree leases with persisted refcount state.

### Modified Capabilities

- **FR-412**: System MUST keep core MCP routing (`search_code`, `locate_symbol`) on merged VCS query path.

### Key Entities

- **OverlayIndex**: ref-scoped Tantivy index set for branch-specific deltas.
- **TombstoneEntry**: path suppression record preventing base leakage for a ref.
- **MergeKey**: dedup key for base/overlay collision resolution.
- **WorktreeLease**: persistent refcount lease for safe worktree lifecycle.

## Impact

- **SC-400**: Same query on two refs returns ref-consistent results with zero cross-ref leakage.
- **SC-401**: Rebase/force-push recovery rebuilds overlay correctness without base rebuild.
- **SC-402**: Deleted files in feature branch are never returned for that ref.
- **SC-403**: Incremental sync (10 changed files) completes in under 5 seconds.
- **SC-404**: Overlay bootstrap (50 changed files) completes in under 15 seconds.
- **SC-405**: VCS-mode `search_code`/`locate_symbol` always return `source_layer` metadata.

### Edge Cases

- Deleted files in feature branches must never be returned from base for that ref.
- Failed sync must preserve the previous published overlay state.
- Concurrent sync on same `(project, ref)` must fail fast with explicit status.
- Branch names with filesystem-unsafe characters must be normalized deterministically.

### Affected Crates

- `cruxe-vcs` (VcsAdapter + Git2 + worktree leases)
- `cruxe-indexer` (overlay lifecycle, staging, sync_incremental)
- `cruxe-query` (tombstones + overlay_merge + VCS routing)
- `cruxe-state` (branch/tombstone/lease state tables)
- `cruxe-mcp` (core routing updates for search/locate)
- `cruxe-core` (types: `source_layer` field, `OverlayMergeKey`)

### API Impact

- Additive: `source_layer` metadata field on VCS-mode search/locate results.
- MCP tool schemas (`search_code`, `locate_symbol`) remain unchanged — VCS routing is transparent to the agent.
- Single-version mode (no Git) bypasses overlay merge entirely; no `source_layer` metadata.

### Performance Impact

- Incremental sync targets < 5s for 10 changed files.
- Overlay bootstrap targets < 15s for 50 changed files.
- Query-time merge adds parallel base+overlay search with dedup overhead.
