# Data Model: Cruxe Core MVP

## Storage Architecture

Two embedded storage engines, each with clear responsibility:

| Layer | Engine | Purpose |
|-------|--------|---------|
| Full-text search | Tantivy | BM25 retrieval, custom code tokenizers, segment-per-branch |
| Structured data | SQLite | Symbol relations, file manifest, branch state, job state, config |

## Tantivy Index Schemas

### 1. `symbols` Index

One record per symbol definition (v1). Reference records optional in v2+.

| Field | Tantivy Type | Tokenizer | Purpose |
|-------|-------------|-----------|---------|
| `repo` | STRING | exact | Project identity (canonical repo root path) |
| `ref` | STRING | exact | Branch name or `"live"` for single-version mode |
| `commit` | STRING (stored) | — | Best-effort commit SHA |
| `path` | TEXT | `code_path` | Source file path |
| `language` | STRING | exact | Programming language |
| `symbol_exact` | STRING | exact | Short symbol name (exact match) |
| `qualified_name` | TEXT | `code_dotted` | Full qualified name |
| `kind` | STRING | exact | function, struct, class, method, trait, etc. |
| `signature` | TEXT | `code_camel` + `code_snake` | Type signature |
| `line_start` | U64 (stored) | — | Start line number |
| `line_end` | U64 (stored) | — | End line number |
| `content` | TEXT | default | Symbol body text for full-text matching |
| `visibility` | STRING (stored) | — | public, private, etc. |

### 2. `snippets` Index

One record per code block / function body. Used for full-text matching on
natural language and error string queries.

| Field | Tantivy Type | Tokenizer | Purpose |
|-------|-------------|-----------|---------|
| `repo` | STRING | exact | Project identity |
| `ref` | STRING | exact | Branch/ref scope |
| `commit` | STRING (stored) | — | Best-effort commit SHA |
| `path` | TEXT | `code_path` | Source file path |
| `language` | STRING | exact | Programming language |
| `chunk_type` | STRING | exact | function_body, class_body, module_top, etc. |
| `imports` | TEXT | `code_dotted` | Import statements in scope |
| `line_start` | U64 (stored) | — | Start line |
| `line_end` | U64 (stored) | — | End line |
| `content` | TEXT | default + `code_camel` + `code_snake` | Code content |

### 3. `files` Index

One record per indexed source file.

| Field | Tantivy Type | Tokenizer | Purpose |
|-------|-------------|-----------|---------|
| `repo` | STRING | exact | Project identity |
| `ref` | STRING | exact | Branch/ref scope |
| `commit` | STRING (stored) | — | Best-effort commit SHA |
| `path` | TEXT | `code_path` | Full file path |
| `filename` | STRING | exact | Basename for exact match |
| `language` | STRING | exact | Programming language |
| `updated_at` | STRING (stored) | — | ISO8601 timestamp |
| `content_head` | TEXT | default | First N lines for preview |

## Custom Tantivy Tokenizers

| Name | Input | Output | Use |
|------|-------|--------|-----|
| `code_camel` | `CamelCaseName` | `[camel, case, name]` | Signature, content |
| `code_snake` | `snake_case_name` | `[snake, case, name]` | Signature, content |
| `code_dotted` | `pkg.module.Class` | `[pkg, module, class]` | Qualified names |
| `code_path` | `src/auth/handler.rs` | `[src, auth, handler, rs]` | File paths |

All tokenizers lowercase their output. They are registered as named analyzers
in Tantivy and applied per-field as specified in the index schemas above.

## SQLite Schema

### `projects` Table

```sql
CREATE TABLE projects (
  project_id TEXT PRIMARY KEY,        -- blake3(realpath(repo_root))[:16]
  repo_root TEXT NOT NULL UNIQUE,     -- absolute path to repo root
  display_name TEXT,                  -- optional human-friendly name
  default_ref TEXT DEFAULT 'main',
  vcs_mode INTEGER NOT NULL DEFAULT 1, -- 0=single-version, 1=VCS
  schema_version INTEGER NOT NULL DEFAULT 1,
  parser_version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,           -- ISO8601
  updated_at TEXT NOT NULL
);
```

### `file_manifest` Table

```sql
CREATE TABLE file_manifest (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  path TEXT NOT NULL,
  content_hash TEXT NOT NULL,         -- blake3
  size_bytes INTEGER NOT NULL,
  mtime_ns INTEGER,                   -- fast pre-filter only
  language TEXT,
  indexed_at TEXT NOT NULL,           -- ISO8601
  PRIMARY KEY(repo, ref, path)
);
```

### `symbol_relations` Table

```sql
CREATE TABLE symbol_relations (
  id INTEGER PRIMARY KEY,
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  commit TEXT,
  path TEXT NOT NULL,
  symbol_id TEXT NOT NULL,
  symbol_stable_id TEXT NOT NULL,
  name TEXT NOT NULL,
  qualified_name TEXT NOT NULL,
  kind TEXT NOT NULL,
  language TEXT NOT NULL,
  line_start INTEGER NOT NULL,
  line_end INTEGER NOT NULL,
  signature TEXT,
  parent_symbol_id TEXT,
  visibility TEXT,
  content_hash TEXT NOT NULL,
  UNIQUE(repo, ref, path, qualified_name, kind, line_start),
  UNIQUE(repo, ref, symbol_stable_id, kind)
);

CREATE INDEX idx_symbol_relations_lookup
  ON symbol_relations(repo, ref, path, line_start);
CREATE INDEX idx_symbol_relations_name
  ON symbol_relations(repo, ref, name);
```

### `symbol_edges` Table (schema-ready, populated in Phase 1.5+)

```sql
CREATE TABLE symbol_edges (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT NOT NULL,
  edge_type TEXT NOT NULL,
  confidence TEXT DEFAULT 'static',
  PRIMARY KEY(repo, ref, from_symbol_id, to_symbol_id, edge_type)
);
```

### `branch_state` Table

```sql
CREATE TABLE branch_state (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  merge_base_commit TEXT,
  last_indexed_commit TEXT NOT NULL,
  overlay_dir TEXT,
  file_count INTEGER DEFAULT 0,
  created_at TEXT NOT NULL,
  last_accessed_at TEXT NOT NULL,
  PRIMARY KEY(repo, ref)
);
```

### `branch_tombstones` Table

```sql
CREATE TABLE branch_tombstones (
  repo TEXT NOT NULL,
  ref TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY(repo, ref, path)
);
```

### `index_jobs` Table

```sql
CREATE TABLE index_jobs (
  job_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(project_id),
  ref TEXT NOT NULL,
  mode TEXT NOT NULL,                 -- 'full', 'incremental', 'overlay_rebuild'
  head_commit TEXT,
  sync_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
  changed_files INTEGER DEFAULT 0,
  duration_ms INTEGER,
  error_message TEXT,
  retry_count INTEGER DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_jobs_status ON index_jobs(status, created_at);
```

### `known_workspaces` Table (schema-ready, used in Phase 1.5+)

```sql
CREATE TABLE known_workspaces (
  workspace_path TEXT PRIMARY KEY,
  project_id TEXT REFERENCES projects(project_id),
  auto_discovered INTEGER DEFAULT 0,
  last_used_at TEXT NOT NULL,
  index_status TEXT DEFAULT 'unknown'
);
```

### SQLite Pragmas (applied on every connection open)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -64000;       -- 64MB
```

## Identity Rules

### `project_id`

- Input: `realpath(repo_root)` (absolute, symlink-resolved)
- Algorithm: `blake3(input)` → first 16 hex characters
- Deterministic: same path = same ID across runs

### `symbol_id` (ref-local, changes on line movement)

- Input: `repo + ref + path + kind + line_start + name`
- Algorithm: `blake3(input)` → hex string
- Purpose: unique within a single ref snapshot

### `symbol_stable_id` (location-insensitive, survives line movement)

- Input: `"stable_id:v1|" + language + "|" + kind + "|" + qualified_name + "|" + normalized_signature`
- Excluded: `line_start`, `line_end`, `path`, `ref`, `commit`
- Algorithm: `blake3(input)` → hex string
- If signature is empty, use empty string in input
- Version: `stable_id_version = 1` (tracked in `projects` table for migration)

## Dedup and Merge Keys

### Within a single ref (storage dedup)

| Index | Key |
|-------|-----|
| symbols | `repo + ref + symbol_stable_id + kind` |
| snippets | `repo + ref + path + chunk_type + line_start + line_end` |
| files | `repo + ref + path` |

### Cross-ref query merge (base + overlay in Phase 2)

| Index | Key |
|-------|-----|
| symbols | `repo + symbol_stable_id + kind` |
| snippets | `repo + path + chunk_type + line_start + line_end` |
| files | `repo + path` |

Overlay wins on key collision. Tombstoned paths suppress base results.
