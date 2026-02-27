# Research: Cruxe Core MVP

## Decision 1: Full-Text Search Engine — Tantivy

**Decision**: Use Tantivy (embedded Rust full-text search engine) as the primary
search backend.

**Rationale**:
- True single binary: compiles into the Cruxe binary, no separate process.
- Segment model maps naturally to base+overlay indexing for branch isolation.
- Zero cold-start dependency: in-process mmap, no "wait for service ready".
- Full control over tokenization: custom code tokenizers compiled in.
- Sub-millisecond warm query latency for symbol lookups.

**Alternatives considered**:
- Meilisearch: requires separate process, port management, health checks.
  Valid for multi-tenant scenarios but breaks single-binary goal.
- Elasticsearch/OpenSearch: heavy operational model, not suitable for local dev tool.
- LanceDB: Rust native embedded, strong for vectors but weaker for BM25 symbol
  precision. Considered as additive layer for Phase 3 semantic search.

## Decision 2: Structured Data Store — SQLite via rusqlite

**Decision**: Use SQLite via `rusqlite` crate for all structured data (symbol
relations, file manifest, branch state, index jobs, workspace registry).

**Rationale**:
- Synchronous API matches the non-async nature of SQLite operations.
- WAL mode enables concurrent read/write (MCP queries during indexing).
- Rich relational queries (JOIN, CTE) required for symbol relation graph.
- `rusqlite` is the most mature SQLite binding for Rust with good `bundled` feature.
- No need for async SQLite driver since all DB operations are fast (<10ms).

**Alternatives considered**:
- `sqlx`: async driver, good for network databases, overkill for embedded SQLite.
  Adds compile-time query checking complexity without clear benefit for in-process use.
- LMDB: key-value only, no relational queries, would need a third storage engine.
- RocksDB/sled/redb: same reasoning — Tantivy owns search, SQLite owns structure.

## Decision 3: Code Parser — tree-sitter

**Decision**: Use tree-sitter with per-language grammars for symbol extraction.

**Rationale**:
- Incremental parsing, error-tolerant (works on incomplete/broken code).
- Language grammar ecosystem covers all v1 languages (Rust, TypeScript, Python, Go).
- Produces concrete syntax trees suitable for scope nesting and parent extraction.
- Widely used in editors and code intelligence tools, well-tested grammars.

**Alternatives considered**:
- LSP: accurate but requires language server processes, breaks single-binary goal.
- regex/heuristics: fragile across languages, poor nesting support.
- SCIP: powerful cross-reference format but requires separate indexer per language.

## Decision 4: v1 Language Scope — Rust, TypeScript, Python, Go

**Decision**: Ship v1 with Rust, TypeScript, Python, and Go support.

**Rationale**:
- Covers the four most common languages in AI agent development workflows.
- tree-sitter grammars for all four are mature and well-maintained.
- Limits parser testing surface to a manageable scope for initial release.
- Additional languages can be added incrementally without architecture changes.

## Decision 5: Project Identity — blake3 Hash of Canonical Path

**Decision**: Generate `project_id` as the first 16 hex characters of
`blake3(realpath(repo_root))`.

**Rationale**:
- Deterministic: same path always produces same ID.
- Short enough for directory names and log output.
- blake3 is already a dependency for content hashing.
- `realpath` normalization prevents duplicates from symlinks or relative paths.

## Decision 6: MCP Protocol — JSON-RPC over stdio

**Decision**: Implement MCP server using JSON-RPC 2.0 over stdin/stdout for v1.

**Rationale**:
- stdio is the standard MCP transport supported by all major agents.
- No port management, no HTTP server complexity for MVP.
- HTTP transport planned for Phase 1.5 (health endpoint, multi-client support).

## Decision 7: Index Directory Layout

**Decision**: Store all index data under `~/.cruxe/data/<project_id>/`.

**Rationale**:
- Separates index data from source code (no pollution of repo).
- Per-project isolation prevents cross-project interference.
- Standard XDG-like location on all platforms.

**Layout**:
```
~/.cruxe/
  data/
    <project_id>/
      base/
        symbols/      # Tantivy symbols index
        snippets/     # Tantivy snippets index
        files/        # Tantivy files index
      overlay/
        <branch>/     # Per-branch overlay indices (Phase 2)
      state.db        # SQLite database
  config.toml         # Global configuration
```

## Decision 8: Error Handling Strategy

**Decision**: Use `thiserror` for per-crate error types with a unified top-level
error type in `cruxe-core`.

**Rationale**:
- Each crate defines its own error enum for specificity.
- `cruxe-core::Error` wraps all crate errors for CLI/MCP error reporting.
- `anyhow` is used only in the CLI binary for ad-hoc context; library crates
  use typed errors exclusively.

## Decision 9: Tracing Span Naming Convention

**Decision**: Use module-path-based span names.

**Convention**:
- Top-level commands: `cruxe::cmd::{init,doctor,index,search,serve_mcp}`
- Index operations: `cruxe::index::{scan,parse,write,commit}`
- Query operations: `cruxe::query::{plan,retrieve,rerank,respond}`
- State operations: `cruxe::state::{db,manifest,jobs}`

## Decision 10: Configuration File Format and Precedence

**Decision**: TOML configuration with three-layer precedence.

**Precedence** (highest to lowest):
1. CLI flags / environment variables
2. Project config: `<repo>/.cruxe/config.toml`
3. Global config: `~/.cruxe/config.toml`
4. Built-in defaults

**Rationale**:
- TOML is the Rust ecosystem standard (Cargo.toml).
- Three-layer precedence matches standard dev tool conventions.
- Environment variables use `CRUXE_` prefix.
