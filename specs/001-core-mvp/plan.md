# Implementation Plan: Cruxe Core MVP

**Branch**: `001-core-mvp` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-core-mvp/spec.md`

## Summary

Build the foundational Rust workspace, embedded storage engines (Tantivy + SQLite),
tree-sitter-based code parser, symbol indexer, and MCP server (stdio) with
`locate_symbol` and `search_code` tools. The deliverable is a single binary that
can be installed via `cargo install`, indexes a code repository, and serves precise
`file:line` symbol location results to AI coding agents via MCP protocol.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Primary Dependencies**: tantivy, rusqlite, tree-sitter, tokio, clap, serde, tracing, git2, blake3, ignore
**Storage**: Tantivy (embedded full-text search) + SQLite (embedded structured data, WAL mode)
**Testing**: cargo test + fixture repos for integration/E2E
**Target Platform**: macOS (arm64, x86_64), Linux (x86_64, aarch64), Windows (x86_64)
**Project Type**: CLI + MCP server (single binary)
**Performance Goals**: symbol lookup p95 < 300ms warm, search p95 < 500ms, full index < 60s for 5k files
**Constraints**: zero external service dependencies, single binary distribution, < 100MB idle memory
**Scale/Scope**: single repo up to 50k files, v1 languages: Rust, TypeScript, Python, Go

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | PASS | `locate_symbol` with definition-first policy is the primary tool |
| II. Single Binary Distribution | PASS | Tantivy + SQLite embedded, `cargo install` distribution |
| III. Branch/Worktree Correctness | PASS (preview) | `ref` field in all schemas from day one; full VCS GA deferred to Phase 2 |
| IV. Incremental by Design | PASS | blake3 content hashing in `file_manifest`, idempotent operations |
| V. Agent-Aware Response Design | PASS | Protocol v1 metadata in all responses; detail_level deferred to Phase 1.1 |
| VI. Fail-Soft Operation | PASS | Search during indexing with partial results; no external providers in MVP |
| VII. Explainable Ranking | PASS (partial) | Basic ranking in MVP; debug ranking_reasons deferred to Phase 1.1 |

## Project Structure

### Documentation (this feature)

```text
specs/001-core-mvp/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Technology decisions and rationale
├── data-model.md        # Entity schemas (Tantivy + SQLite)
├── contracts/           # MCP tool schemas
│   └── mcp-tools.md     # Tool input/output contracts
└── tasks.md             # Actionable task list
```

### Source Code (repository root)

```text
Cargo.toml                    # Workspace root
crates/
├── cruxe-cli/          # Binary entry point (clap commands)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       └── commands/
│           ├── mod.rs
│           ├── init.rs
│           ├── doctor.rs
│           ├── index.rs
│           ├── search.rs
│           └── serve_mcp.rs
├── cruxe-core/         # Shared types, errors, config, constants
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── error.rs
│       ├── config.rs
│       ├── types.rs          # Project, Symbol, Snippet, File, Ref types
│       └── constants.rs      # REF_LIVE, default limits, schema versions
├── cruxe-state/        # SQLite state store + Tantivy index management
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── db.rs             # SQLite connection, pragmas, migrations
│       ├── schema.rs         # Table creation DDL
│       ├── project.rs        # Projects CRUD
│       ├── manifest.rs       # file_manifest operations
│       ├── symbols.rs        # symbol_relations CRUD
│       ├── jobs.rs           # index_jobs state machine
│       ├── tantivy_index.rs  # Tantivy index creation, schema setup
│       └── tokenizers.rs     # Custom code tokenizers
├── cruxe-indexer/      # Scanner, parser, index writer
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── scanner.rs        # File discovery with ignore chain
│       ├── parser.rs         # tree-sitter extraction dispatcher
│       ├── languages/        # Per-language tree-sitter queries
│       │   ├── mod.rs
│       │   ├── rust.rs
│       │   ├── typescript.rs
│       │   ├── python.rs
│       │   └── go.rs
│       ├── symbol_extract.rs # Symbol record builder
│       ├── snippet_extract.rs # Snippet record builder
│       └── writer.rs         # Tantivy + SQLite batch writer
├── cruxe-query/        # Search, locate, query planner
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── intent.rs         # Query intent classifier
│       ├── planner.rs        # Query execution planner
│       ├── search.rs         # search_code implementation
│       ├── locate.rs         # locate_symbol implementation
│       └── ranking.rs        # Rule-based rerank
└── cruxe-mcp/          # MCP server (stdio transport)
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── server.rs         # MCP JSON-RPC server loop
        ├── tools/
        │   ├── mod.rs
        │   ├── index_repo.rs
        │   ├── sync_repo.rs
        │   ├── search_code.rs
        │   ├── locate_symbol.rs
        │   └── index_status.rs
        └── protocol.rs       # Protocol v1 response types

configs/
└── default.toml              # Default configuration

testdata/
├── fixtures/
│   ├── rust-sample/          # Rust fixture repo
│   ├── ts-sample/            # TypeScript fixture repo
│   ├── python-sample/        # Python fixture repo
│   └── go-sample/            # Go fixture repo
└── golden/                   # Expected output snapshots
```

**Structure Decision**: Rust workspace with 6 crates, split by responsibility.
`cruxe-core` contains shared types and config (no separate config crate —
config logic is lightweight). Crates are added incrementally: Phase 0 starts with
`cli`, `core`, `state`; Phase 1 adds `indexer`, `query`, `mcp`.

## Complexity Tracking

No constitution violations to justify. All decisions follow the minimal-complexity
path described in the principles.
