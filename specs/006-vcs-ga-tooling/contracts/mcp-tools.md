# MCP Tool Contracts: VCS GA

Transport: JSON-RPC 2.0 over stdio (v1) and HTTP (v1.5+).

All responses include Protocol v1 metadata fields (defined in 001-core-mvp contracts).
VCS-mode responses additionally include `source_layer` per result.
Error codes MUST follow the canonical registry in `specs/meta/protocol-error-codes.md`.

## Protocol v1 Response Metadata (VCS Extension)

In addition to the base Protocol v1 metadata, VCS-mode responses include:

```json
{
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh | stale | syncing",
    "indexing_status": "not_indexed | indexing | ready | failed",
    "result_completeness": "complete | partial | truncated",
    "ref": "feat/auth",
    "schema_status": "compatible | not_indexed | reindex_required | corrupt_manifest"
  }
}
```

Per-result VCS fields:

```json
{
  "source_layer": "base | overlay"
}
```

---

## Tool: `diff_context`

Symbol-level change summary between two refs. Returns added, modified, and deleted
symbols with before/after signatures. No equivalent exists in any open-source competitor.

### Input

```json
{
  "base_ref": "main",
  "head_ref": "feat/oauth2",
  "workspace": "/path/to/repo",
  "path_filter": "src/auth/",
  "limit": 20
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `base_ref` | string | no | Base ref for comparison. Default: project's default branch. |
| `head_ref` | string | no | Head ref for comparison. Default: current HEAD. |
| `workspace` | string | no | Repo root path. Default: current project. |
| `path_filter` | string | no | Restrict to files matching this path prefix. |
| `limit` | int | no | Max number of symbol changes to return. Default: 50. |

### Output

```json
{
  "base_ref": "main",
  "head_ref": "feat/oauth2",
  "merge_base_commit": "abc123def456",
  "affected_files": 5,
  "file_changes": [
    {
      "path": "src/auth/oauth.rs",
      "change_type": "modified"
    },
    {
      "path": "src/auth/legacy.rs",
      "change_type": "deleted"
    }
  ],
  "changes": [
    {
      "symbol": "refresh_token",
      "change_type": "added",
      "before": null,
      "after": {
        "symbol_id": "sym_01HQ6T2W43AC9R9WQJPG2T2M8A",
        "symbol_stable_id": "b3:1e0f98...",
        "kind": "fn",
        "qualified_name": "auth::oauth::refresh_token",
        "signature": "pub fn refresh_token(token: &str) -> Result<TokenPair>",
        "path": "src/auth/oauth.rs",
        "line_start": 45,
        "line_end": 78
      },
      "path": "src/auth/oauth.rs",
      "lines": { "start": 45, "end": 78 }
    },
    {
      "symbol": "authenticate",
      "change_type": "modified",
      "before": {
        "symbol_id": "sym_01HQ6T2YH2M2E6XW2KJ8NAX4VV",
        "symbol_stable_id": "b3:74ad3a...",
        "kind": "fn",
        "qualified_name": "auth::handler::authenticate",
        "signature": "pub fn authenticate(req: &Request) -> Result<User>",
        "path": "src/auth/handler.rs",
        "line_start": 23,
        "line_end": 56
      },
      "after": {
        "symbol_id": "sym_01HQ6T2YH2M2E6XW2KJ8NAX4VV",
        "symbol_stable_id": "b3:74ad3a...",
        "kind": "fn",
        "qualified_name": "auth::handler::authenticate",
        "signature": "pub fn authenticate(req: &Request, provider: AuthProvider) -> Result<User>",
        "path": "src/auth/handler.rs",
        "line_start": 23,
        "line_end": 62
      },
      "path": "src/auth/handler.rs",
      "lines": { "start": 23, "end": 62 }
    },
    {
      "symbol": "legacy_auth",
      "change_type": "deleted",
      "before": {
        "symbol_id": "sym_01HQ6T307X4JX9J3R7F9WPZ8ZW",
        "symbol_stable_id": "b3:d09e5c...",
        "kind": "fn",
        "qualified_name": "auth::legacy::legacy_auth",
        "signature": "pub fn legacy_auth(creds: &Credentials) -> Result<Session>",
        "path": "src/auth/legacy.rs",
        "line_start": 12,
        "line_end": 34
      },
      "after": null,
      "path": "src/auth/legacy.rs",
      "lines": { "start": 12, "end": 34 }
    }
  ],
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "feat/oauth2"
  }
}
```

`file_changes` provides file-level add/modify/delete summaries and complements
symbol-level `changes`.

### Symbol Diff Classification

Symbols are matched between base and head using `symbol_stable_id`:

| Scenario | Change Type | `before` | `after` |
|----------|-------------|----------|---------|
| Symbol exists in head but not base | `added` | null | symbol record |
| Symbol exists in both, signature/content changed | `modified` | base record | head record |
| Symbol exists in base but not head | `deleted` | base record | null |
| Symbol exists in both, unchanged | Not included | — | — |

"Changed" is determined by comparing `content_hash` from `symbol_relations`.
If `content_hash` differs, the symbol is `modified` even if the signature is unchanged
(body change without signature change).

### Errors

| Code | Meaning |
|------|---------|
| `project_not_found` | No project registered for the given workspace path. |
| `ref_not_indexed` | The specified ref has not been indexed yet. |
| `merge_base_failed` | Unable to compute merge-base between the two refs. |

---

## Tool: `find_references`

Find all references to a symbol using the `symbol_edges` table for import/call
graph lookups. Returns each reference with file location and edge type.

### Input

```json
{
  "symbol_name": "validate_token",
  "ref": "main",
  "kind": "imports",
  "limit": 20
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `symbol_name` | string | yes | Name or qualified name of the symbol to find references for. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `kind` | string | no | Filter by edge type: `"imports"`, `"calls"`, `"implements"`, `"extends"`, `"references"`. Default: all types. |
| `limit` | int | no | Max results. Default: 20. |

### Output

```json
{
  "symbol": {
    "symbol_id": "sym_01HQ6T2W43AC9R9WQJPG2T2M8A",
    "symbol_stable_id": "b3:1e0f98...",
    "name": "validate_token",
    "qualified_name": "auth::jwt::validate_token",
    "kind": "fn",
    "path": "src/auth/jwt.rs",
    "line_start": 87
  },
  "references": [
    {
      "path": "src/middleware/auth_middleware.rs",
      "line_start": 15,
      "edge_type": "imports",
      "source_layer": "base",
      "context": "use crate::auth::jwt::validate_token;",
      "from_symbol": {
        "symbol_id": "sym_01HQ6T5M2YCB4FEHZ63M8Q2TY0",
        "symbol_stable_id": "b3:8fe0aa...",
        "name": "auth_middleware",
        "qualified_name": "middleware::auth_middleware",
        "kind": "module"
      }
    },
    {
      "path": "src/middleware/auth_middleware.rs",
      "line_start": 42,
      "edge_type": "calls",
      "source_layer": "overlay",
      "context": "let claims = validate_token(&token, &config.jwt_key)?;",
      "from_symbol": {
        "symbol_id": "sym_01HQ6T5P1BBFD4QV5X4PAXR2S9",
        "symbol_stable_id": "b3:4dc1f1...",
        "name": "check_auth",
        "qualified_name": "middleware::auth_middleware::check_auth",
        "kind": "fn"
      }
    },
    {
      "path": "src/handlers/login.rs",
      "line_start": 8,
      "edge_type": "imports",
      "source_layer": "base",
      "context": "use crate::auth::jwt::validate_token;",
      "from_symbol": {
        "symbol_id": "sym_01HQ6T5R6SFN3ETJQW7KQ8N0FM",
        "symbol_stable_id": "b3:35abf3...",
        "name": "login",
        "qualified_name": "handlers::login",
        "kind": "module"
      }
    }
  ],
  "total_references": 3,
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main"
  }
}
```

### Reference Resolution

1. Look up the target symbol in `symbol_relations` by name (or qualified name).
2. Query `symbol_edges` for edges where `to_symbol_id` matches the target.
3. For each edge, resolve `from_symbol_id` back to `symbol_relations` for location data.
4. Include a `context` snippet (the source line at the reference site).
5. Include `source_layer` (`base` or `overlay`) per reference in VCS mode.
6. All lookups are ref-scoped (base+overlay merge applies in VCS mode).

### Errors

| Code | Meaning |
|------|---------|
| `symbol_not_found` | No symbol matching the given name was found. |
| `no_edges_available` | `symbol_edges` table is not yet populated for this project. |

---

## Tool: `explain_ranking`

Full scoring breakdown for a specific search result. Returns the individual
contribution of each ranking factor, enabling debugging and transparency.

### Input

```json
{
  "query": "validate token",
  "result_path": "src/auth/jwt.rs",
  "result_line_start": 87,
  "ref": "main",
  "language": "rust",
  "limit": 200
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | The search query to explain. |
| `result_path` | string | yes | File path of the result to explain. |
| `result_line_start` | int | yes | Start line of the result to explain. |
| `ref` | string | no | Branch/ref scope. Default: current HEAD or `"live"`. |
| `language` | string | no | Language filter used when replaying ranking context (for example `"rust"`). |
| `limit` | int | no | Candidate limit used when replaying ranking context. Default: 200. |

### Output

```json
{
  "query": "validate token",
  "result": {
    "path": "src/auth/jwt.rs",
    "line_start": 87,
    "line_end": 112,
    "kind": "fn",
    "name": "validate_token",
    "source_layer": "base"
  },
  "scoring": {
    "bm25": 0.72,
    "exact_match": 0.15,
    "qualified_name": 0.05,
    "path_affinity": 0.0,
    "definition_boost": 0.10,
    "kind_match": 0.0,
    "total": 1.02
  },
  "scoring_details": {
    "bm25_source": "symbols.symbol_exact",
    "exact_match_reason": "query tokens match symbol_exact field",
    "qualified_name_reason": "partial match on 'auth::jwt::validate_token'",
    "path_affinity_reason": "no path bias applied",
    "definition_boost_reason": "symbol is a definition (not a reference)",
    "kind_match_reason": "no kind filter in query"
  },
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main"
  }
}
```

### Scoring Components

| Component | Range | Description |
|-----------|-------|-------------|
| `bm25` | 0.0+ | Raw BM25 score from Tantivy, normalized |
| `exact_match` | 0.0-0.2 | Bonus for exact token match on `symbol_exact` |
| `qualified_name` | 0.0-0.1 | Bonus for match on `qualified_name` field |
| `path_affinity` | 0.0-0.1 | Bonus for path proximity to recent user context |
| `definition_boost` | 0.0-0.15 | Bonus for definition records vs. references |
| `kind_match` | 0.0-0.05 | Bonus when result kind matches query intent |
| `total` | 0.0+ | Sum of all components |

### Determinism Guarantee

Per Constitution Principle VII: the same query against the same index state MUST
produce the same scoring breakdown. The `explain_ranking` output is fully
deterministic and reproducible.

### Errors

| Code | Meaning |
|------|---------|
| `result_not_found` | No result matching the given path and line was found. |
| `internal_error` | Query execution failed unexpectedly. Retry or inspect server logs. |

---

## Tool: `list_refs`

List all indexed refs (branches/tags) for a project. VCS mode only.

### Input

```json
{
  "workspace": "/path/to/repo"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `workspace` | string | no | Repo root path. Default: current project. |

### Output

```json
{
  "refs": [
    {
      "ref": "main",
      "is_default": true,
      "last_indexed_commit": "abc123def456",
      "merge_base_commit": null,
      "file_count": 3891,
      "symbol_count": 12847,
      "status": "active",
      "last_accessed_at": "2026-02-23T10:30:00Z"
    },
    {
      "ref": "feat/auth",
      "is_default": false,
      "last_indexed_commit": "def789abc012",
      "merge_base_commit": "abc123def456",
      "file_count": 12,
      "symbol_count": 45,
      "status": "active",
      "last_accessed_at": "2026-02-23T11:15:00Z"
    }
  ],
  "total_refs": 2,
  "vcs_mode": true,
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "main"
  }
}
```

### Non-VCS Mode

When called on a non-VCS project (single-version mode):

```json
{
  "refs": [
    {
      "ref": "live",
      "is_default": true,
      "last_indexed_commit": null,
      "merge_base_commit": null,
      "file_count": 3891,
      "symbol_count": 12847,
      "status": "active",
      "last_accessed_at": "2026-02-23T10:30:00Z"
    }
  ],
  "total_refs": 1,
  "vcs_mode": false,
  "metadata": { "..." }
}
```

### Errors

| Code | Meaning |
|------|---------|
| `project_not_found` | No project registered for the given workspace path. |

---

## Tool: `switch_ref`

Optional helper for worktree-backed sessions. Changes the default ref for
subsequent queries in the current session.

### Input

```json
{
  "ref": "feat/auth",
  "workspace": "/path/to/repo"
}
```

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| `ref` | string | yes | The ref to switch to. |
| `workspace` | string | no | Repo root path. Default: current project. |

### Output

```json
{
  "ref": "feat/auth",
  "previous_ref": "main",
  "worktree_path": "/home/user/.cruxe/worktrees/a1b2c3d4/feat-auth",
  "status": "active",
  "last_indexed_commit": "def789abc012",
  "metadata": {
    "cruxe_protocol_version": "1.0",
    "freshness_status": "fresh",
    "indexing_status": "ready",
    "result_completeness": "complete",
    "ref": "feat/auth"
  }
}
```

### Behavior

1. Validate that the ref exists in `branch_state`.
2. If the ref has an active overlay, switch session default.
3. If the ref has no overlay, return error with guidance.
4. Update `last_accessed_at` on the `branch_state` entry.
5. If worktree-backed: ensure worktree exists via `EnsureWorktree`, return path.
6. After `switch_ref`, queries without explicit `ref` parameter use this ref.

### Errors

| Code | Meaning |
|------|---------|
| `ref_not_indexed` | No queryable indexed state exists for the requested ref (missing in `branch_state` or not yet indexed). Guidance: call `index_repo` with the ref first. |
| `overlay_not_ready` | Ref overlay/worktree is not queryable yet (creation failed or still preparing). Retry after sync/index completion. |

---

## CLI Commands (non-MCP, new in VCS GA)

| Command | Description |
|---------|-------------|
| `cruxe state export <path>` | Export index + SQLite state to portable `.tar.zst` archive |
| `cruxe state import <path>` | Import archive and restore state |
| `cruxe prune-overlays [--older-than DAYS]` | Remove overlay indices for inactive branches |

All CLI commands support `--verbose` / `-v` for increased log output and
`--workspace PATH` for targeting a specific project.
