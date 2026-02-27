# Data Model: VCS GA

## Overview

This document describes the additional data structures introduced by the VCS GA feature.
The base schemas (Tantivy indices, SQLite tables) are defined in 001-core-mvp data-model.md
and remain unchanged. This document covers:

1. Overlay directory layout
2. `sync_id` extension to `index_jobs`
3. Worktree metadata tables
4. Tombstone usage patterns
5. `branch_state` extensions
6. Two-phase write staging layout

## Overlay Directory Layout

Each project maintains a base index and zero or more overlay indices. The directory
structure under the CodeCompass data root:

```text
~/.codecompass/data/<project_id>/
  base/
    symbols/          # Tantivy index: base symbols (default branch)
    snippets/         # Tantivy index: base snippets
    files/            # Tantivy index: base files
  overlay/
    feat-auth/        # Branch name with '/' replaced by '-'
      symbols/        # Tantivy index: overlay symbols
      snippets/       # Tantivy index: overlay snippets
      files/          # Tantivy index: overlay files
    fix-typo/
      symbols/
      snippets/
      files/
  staging/            # Temporary staging area for two-phase writes
    <sync_id>/
      symbols/
      snippets/
      files/
```

### Directory Naming

Branch names are normalized for filesystem safety:

- `/` is replaced with `-` (e.g., `feat/auth` becomes `feat-auth`)
- Characters unsafe for filesystems are percent-encoded
- The normalized name is stored in `branch_state.overlay_dir`
- Reverse mapping is always available via `branch_state` table lookup

### Base Index

- Contains the full index for the default branch (typically `main`)
- Updated only when the default branch itself is synced
- Read-only from the perspective of overlay operations
- All overlay queries open the base reader as a shared snapshot

### Overlay Index

- Contains ONLY records for files that differ from merge-base
- One complete set of three indices (symbols, snippets, files) per active branch
- Records in overlay use the branch name as `ref` field
- Records in base use the default branch name as `ref` field

## SQLite Schema Extensions

### `branch_state` Table (Extended from 001-core-mvp)

The `branch_state` table defined in 001-core-mvp is extended with additional fields
for VCS GA tracking:

```sql
CREATE TABLE branch_state (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  merge_base_commit TEXT,              -- NULL for default branch
  last_indexed_commit TEXT NOT NULL,
  overlay_dir TEXT,                     -- normalized filesystem path for overlay
  file_count INTEGER DEFAULT 0,        -- number of files in overlay
  symbol_count INTEGER DEFAULT 0,      -- NEW: number of symbols in overlay
  is_default_branch INTEGER DEFAULT 0, -- NEW: 1 if this is the base/default branch
  status TEXT DEFAULT 'active',        -- NEW: 'active', 'stale', 'rebuilding', 'evicted'
  eviction_eligible_at TEXT,           -- NEW: ISO8601, set based on TTL policy
  created_at TEXT NOT NULL,
  last_accessed_at TEXT NOT NULL,
  PRIMARY KEY(repo, ref)
);

-- Index for eviction queries
CREATE INDEX idx_branch_state_eviction
  ON branch_state(repo, status, last_accessed_at);
```

New fields:

| Field | Type | Purpose |
|-------|------|---------|
| `symbol_count` | INTEGER | Track overlay size for monitoring and eviction decisions |
| `is_default_branch` | INTEGER | Distinguish base branch from overlay branches |
| `status` | TEXT | Track branch overlay lifecycle state |
| `eviction_eligible_at` | TEXT | When this overlay becomes eligible for cleanup |

Status transitions:

```text
(new branch) -> active -> stale (ancestry break detected)
                  |                    |
                  v                    v
               evicted            rebuilding -> active
```

### `branch_tombstones` Table (Extended from 001-core-mvp)

```sql
CREATE TABLE branch_tombstones (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  path TEXT NOT NULL,
  tombstone_type TEXT DEFAULT 'deleted', -- NEW: 'deleted' or 'replaced'
  created_at TEXT NOT NULL,              -- NEW: track when tombstone was created
  PRIMARY KEY(repo, ref, path)
);
```

New fields:

| Field | Type | Purpose |
|-------|------|---------|
| `tombstone_type` | TEXT | Distinguish files deleted vs. files replaced in overlay |
| `created_at` | TEXT | Track tombstone age for debugging and auditing |

Usage patterns:

- **Deleted file**: `tombstone_type = 'deleted'` - file exists in base but was removed
  on the branch. No overlay record exists. Base results for this path are suppressed.
- **Replaced file**: `tombstone_type = 'replaced'` - file exists in both base and overlay
  but with different content. Overlay records exist. Base results for this path are
  suppressed (overlay wins on merge key collision anyway, but tombstone ensures
  completeness for files where merge key might not collide, e.g., new symbols added
  to a modified file that have no base counterpart).

### `index_jobs` Table (`sync_id` Extension)

The `index_jobs` table from 001-core-mvp already includes `sync_id`. The VCS GA
feature uses it as follows:

```sql
-- sync_id usage for two-phase writes:
--
-- Phase 1 (staging):
--   INSERT INTO index_jobs (job_id, project_id, ref, mode, sync_id, status, ...)
--   VALUES (<uuid>, <proj>, <branch>, 'incremental', <sync_uuid>, 'running', ...);
--   -- Write overlay records to staging/<sync_id>/ directory
--
-- Phase 2 (commit):
--   -- Atomic rename: staging/<sync_id>/ -> overlay/<branch>/
--   UPDATE index_jobs SET status = 'published' WHERE sync_id = <sync_uuid>;
--
-- Rollback:
--   -- Delete staging/<sync_id>/ directory
--   UPDATE index_jobs SET status = 'rolled_back' WHERE sync_id = <sync_uuid>;
```

Additional `mode` values for VCS GA:

| Mode | Description |
|------|-------------|
| `full` | Full re-index of entire repository (existing) |
| `incremental` | Incremental update based on changed files (existing) |
| `overlay_rebuild` | Full overlay rebuild from new merge-base (NEW) |
| `overlay_incremental` | Incremental overlay update since last indexed commit (NEW) |

### `worktree_leases` Table (New)

```sql
CREATE TABLE worktree_leases (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  worktree_path TEXT NOT NULL,          -- absolute path to worktree directory
  owner_pid INTEGER NOT NULL,           -- owning process (lease collision guard)
  refcount INTEGER NOT NULL DEFAULT 0,  -- number of active consumers
  created_at TEXT NOT NULL,
  last_used_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,             -- compatibility mirror for last_used_at
  status TEXT DEFAULT 'active',         -- 'active', 'stale', 'removing'
  PRIMARY KEY(repo, ref)
);

CREATE INDEX idx_worktree_leases_status
  ON worktree_leases(status, last_used_at);
```

| Field | Type | Purpose |
|-------|------|---------|
| `repo` | TEXT | Project identity |
| `ref` | TEXT | Branch/ref for this worktree |
| `worktree_path` | TEXT | Absolute filesystem path to the Git worktree |
| `owner_pid` | INTEGER | Current lease owner process id (0 when released) |
| `refcount` | INTEGER | Number of active consumers holding the lease |
| `created_at` | TEXT | ISO8601 creation timestamp |
| `last_used_at` | TEXT | ISO8601 last access timestamp |
| `updated_at` | TEXT | Compatibility timestamp mirror (same value as `last_used_at`) |
| `status` | TEXT | Lifecycle state: `active`, `stale`, `removing` |

Lifecycle:

1. `EnsureWorktree(ref)` creates or reuses a worktree, increments `refcount`.
2. Consumer completes work, decrements `refcount`.
3. When `refcount` reaches 0, worktree becomes eligible for cleanup.
4. Cleanup respects `last_used_at` - worktrees used recently are kept.
5. On restart, worktrees with `refcount > 0` are reset to `refcount = 0`
   (stale detection) and their status is set to `stale` for review.

Default worktree root: `~/.codecompass/worktrees/<project_id>/<normalized_ref>/`

## Tantivy Index Field Additions

No new Tantivy fields are added to the index schemas. The existing schemas from
001-core-mvp are sufficient:

- `ref` field (STRING, exact) already scopes records to branches
- `commit` field (STRING, stored) tracks the commit SHA
- `repo` field (STRING, exact) identifies the project

The VCS GA feature uses these existing fields for overlay records. The difference
is operational: overlay records are stored in separate index directories, not mixed
into the base index.

## Query-Time Merge Algorithm

### Inputs

- Base reader: Tantivy reader for `~/.codecompass/data/<project_id>/base/`
- Overlay reader: Tantivy reader for `~/.codecompass/data/<project_id>/overlay/<branch>/`
- Tombstone set: `SELECT path FROM branch_tombstones WHERE repo = ? AND ref = ?`

### Algorithm

```text
function merged_search(query, ref):
  # Step 1: Load tombstones
  tombstones = load_tombstones(repo, ref)

  # Step 2: Parallel search
  base_results = search(base_reader, query)
  overlay_results = search(overlay_reader, query)

  # Step 3: Tag source layer
  for r in base_results: r.source_layer = "base"
  for r in overlay_results: r.source_layer = "overlay"

  # Step 4: Suppress tombstoned paths from base
  base_results = [r for r in base_results if r.path not in tombstones]

  # Step 5: Merge by key (overlay wins)
  merged = {}
  for r in base_results:
    key = merge_key(r)
    merged[key] = r
  for r in overlay_results:
    key = merge_key(r)
    merged[key] = r       # overlay overwrites base on collision

  # Step 6: Sort by score, return
  return sorted(merged.values(), by=score, descending=True)
```

### Merge Keys (from 001-core-mvp data-model.md)

| Index | Cross-ref merge key |
|-------|---------------------|
| symbols | `repo + symbol_stable_id + kind` |
| snippets | `repo + path + chunk_type + line_start + line_end` |
| files | `repo + path` |

## Two-Phase Write Protocol

### Phase 1: Stage

1. Generate `sync_id` (UUID).
2. Create staging directory: `~/.codecompass/data/<project_id>/staging/<sync_id>/`
3. Write all overlay records to staging indices.
4. Record `sync_id` in `index_jobs` with `status = 'running'`.
5. Validate record counts match expected changed files.

### Phase 2: Commit

1. If overlay directory exists, rename to backup:
   `overlay/<branch>/` -> `overlay/<branch>.bak/`
2. Rename staging to overlay:
   `staging/<sync_id>/` -> `overlay/<branch>/`
3. Delete backup: `overlay/<branch>.bak/`
4. Update `index_jobs` status to `'published'`.
5. Update `branch_state` with new `last_indexed_commit` and `merge_base_commit`.
6. Update `branch_tombstones` for the new overlay state.

### Rollback

1. Delete staging directory: `staging/<sync_id>/`
2. If backup exists, restore: `overlay/<branch>.bak/` -> `overlay/<branch>/`
3. Update `index_jobs` status to `'rolled_back'`.
4. Log error details for debugging.

### Atomicity Guarantee

The critical operation is the rename in Phase 2 step 2. On POSIX systems,
`rename(2)` is atomic within the same filesystem. The staging directory MUST
be on the same filesystem as the overlay directory (both under
`~/.codecompass/data/<project_id>/`).

## State Portability Note

Portable export/import data flows are defined in `006-vcs-ga-tooling` and are
validated there. The VCS core data model in this spec focuses on overlay
correctness, merge semantics, and write atomicity.
