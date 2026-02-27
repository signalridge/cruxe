# Development Guide

> Conventions, environment setup, and workflow rules for Cruxe contributors.
> This is the single reference for how to develop against any spec.

> See also: [design.md](design.md) for architecture specifications, [INDEX.md](INDEX.md) for the master index.

## Prerequisites

### Required Tools

| Tool | Version | Purpose |
|------|---------|---------|
| Rust (rustup) | stable (latest) | Primary language |
| cargo | (bundled) | Build, test, lint |
| clippy | (bundled) | Linting |
| rustfmt | (bundled) | Formatting |
| git | >= 2.38 | Version control, worktrees |
| tree-sitter CLI | >= 0.22 | Grammar debugging (optional) |

### Recommended Tools

| Tool | Purpose |
|------|---------|
| `cargo-nextest` | Faster test runner with better output |
| `cargo-watch` | Auto-rebuild on file changes |
| `bacon` | Background Rust code checker |
| `hyperfine` | CLI benchmarking |
| `jq` | JSON output inspection for MCP testing |

### Environment Setup

```bash
# Clone
git clone https://github.com/signalridge/cruxe.git
cd cruxe

# Verify toolchain
rustup show
cargo --version
cargo clippy --version
cargo fmt --version

# Build (first time, fetches all dependencies)
cargo build --workspace

# Run all tests
cargo test --workspace

# Run lints
cargo clippy --workspace -- -D warnings
cargo fmt --check --all
```

## Workspace Structure

```
crates/
  cruxe-cli/       # Binary crate: CLI entry point
  cruxe-core/      # Library: shared types, config, errors
  cruxe-state/     # Library: SQLite + Tantivy storage
  cruxe-indexer/   # Library: file scanning, tree-sitter parsing
  cruxe-query/     # Library: search, locate, ranking
  cruxe-mcp/       # Library: MCP server, tool handlers, protocol
  cruxe-vcs/       # Library: Git adapter, worktree manager (spec 005+)
```

### Crate Dependency DAG

```
cruxe-cli
  ├── cruxe-mcp
  │     ├── cruxe-query
  │     │     ├── cruxe-state
  │     │     │     └── cruxe-core
  │     │     └── cruxe-core
  │     ├── cruxe-indexer
  │     │     ├── cruxe-state
  │     │     └── cruxe-core
  │     └── cruxe-core
  ├── cruxe-vcs (spec 005+)
  │     └── cruxe-core
  └── cruxe-core
```

**Rule**: No circular dependencies. `cruxe-core` is the leaf crate depended on by all others.

## Coding Conventions

### Error Handling

- **Library crates** (`core`, `state`, `indexer`, `query`, `mcp`, `vcs`): use `thiserror` with per-crate error enums.
- **CLI binary** (`cli`): use `anyhow` for ad-hoc context. Library errors wrapped via `From` impls.
- **Never**: `unwrap()` or `expect()` in library code outside tests. Use `?` propagation.
- **Protocol errors**: MCP/HTTP surfaces MUST map to canonical codes in
  [`protocol-error-codes.md`](protocol-error-codes.md). Do not invent ad-hoc wire codes per tool.

```rust
// Good: library crate
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("file too large: {path} ({size} bytes > {max} limit)")]
    FileTooLarge { path: PathBuf, size: u64, max: u64 },
    #[error("parse failed for {path}: {source}")]
    ParseFailed { path: PathBuf, source: tree_sitter::Error },
}

// Good: CLI binary
fn main() -> anyhow::Result<()> {
    let project = load_project().context("failed to load project")?;
    Ok(())
}
```

### Naming

| Item | Convention | Example |
|------|-----------|---------|
| Crate names | `cruxe-{name}` (kebab-case) | `cruxe-state` |
| Module names | snake_case | `tantivy_index.rs` |
| Struct/Enum | PascalCase | `SymbolRecord`, `QueryIntent` |
| Functions | snake_case | `locate_symbol`, `build_query` |
| Constants | SCREAMING_SNAKE | `SCHEMA_VERSION`, `MAX_FILE_SIZE` |
| Test functions | `test_` prefix + descriptive | `test_locate_returns_correct_line` |
| Tracing spans | `module::operation` | `cruxe::index::scan` |

### Tracing

Use structured tracing (not `println!` or `eprintln!`):

```rust
use tracing::{debug, info, instrument, warn};

#[instrument(skip(db), fields(project_id = %project.id))]
pub fn index_project(db: &Database, project: &Project) -> Result<IndexResult> {
    info!("starting index");
    // ...
    debug!(files = count, "scan complete");
}
```

Span naming convention:
- Top-level commands: `cruxe::cmd::{init,doctor,index,search,serve_mcp}`
- Index operations: `cruxe::index::{scan,parse,write,commit}`
- Query operations: `cruxe::query::{plan,retrieve,rerank,respond}`
- State operations: `cruxe::state::{db,manifest,jobs}`

### Testing

```rust
// Unit tests: in the same file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_classification_symbol() {
        let intent = classify_intent("AuthHandler");
        assert_eq!(intent, QueryIntent::Symbol);
    }
}

// Integration tests: in tests/ directory of each crate
// crates/cruxe-query/tests/search_integration.rs
#[test]
fn test_search_returns_correct_results() {
    let fixture = setup_indexed_fixture("rust-sample");
    let results = search_code(&fixture.db, "validate_token", None);
    assert!(results[0].path.ends_with("auth.rs"));
}
```

**Test fixture convention**: All test data lives in `testdata/fixtures/`. Each language has its own sample directory. VCS fixtures use setup scripts.

### Documentation

- Public APIs: doc comments (`///`) on all public types, functions, and modules.
- Crate-level: `//!` doc comment in `lib.rs` explaining the crate's role.
- No inline comments that repeat the code. Only explain _why_, not _what_.

### MCP Contract Consistency

- Retrieval responses should include stable follow-up handles (`symbol_id`, `symbol_stable_id`, `result_id`) where applicable.
- Protocol metadata fields should be additive and backward-compatible.
- Handshake/`tools/list` must remain responsive during background prewarm operations.

## Git Workflow

### Branch Naming

```
feat/<spec-id>/<short-description>    # Feature work
fix/<spec-id>/<short-description>     # Bug fixes
refactor/<scope>/<short-description>  # Refactoring
test/<spec-id>/<short-description>    # Test additions
docs/<scope>/<short-description>      # Documentation
chore/<scope>/<short-description>     # Tooling, CI, dependencies
```

Examples:
```
feat/001/init-command
feat/001/tantivy-tokenizers
feat/002/detail-level
fix/001/sqlite-wal-mode
test/001/search-integration
```

### Commit Messages

Conventional Commits format:

```
type(scope): description

feat(indexer): implement Rust symbol extraction via tree-sitter
fix(query): handle empty search results without panic
test(state): add SQLite WAL mode integration test
refactor(core): extract config loading into separate module
docs(mcp): add tool schemas to contracts/
chore(ci): add clippy lint job to CI workflow
```

**Types**: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`
**Scope**: crate name or spec ID (e.g., `indexer`, `001`, `query`)

### PR Process

1. Create branch from `main`: `git checkout -b feat/001/init-command`
2. Implement tasks (commit per task or logical group)
3. Ensure all checks pass:
   ```bash
   cargo test --workspace
   cargo clippy --workspace -- -D warnings
   cargo fmt --check --all
   ```
4. Open PR targeting `main`
5. PR title follows conventional commit format
6. PR body references spec and task IDs

### Worktree Model (recommended for C3/C4 work)

```bash
# Create worktree for a feature
git worktree add .worktrees/feat-001-init feat/001/init-command

# Work in the worktree
cd .worktrees/feat-001-init

# When done, remove worktree
git worktree remove .worktrees/feat-001-init
```

## Build Verification Checklist

Run before every PR:

```bash
# Build
cargo build --workspace

# Tests (all)
cargo test --workspace

# Lints
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --check --all

# Optional: release build check
cargo build --release
```

## Task Workflow

When working on a task from any spec's `tasks.md`:

1. **Read the task**: Understand the deliverable and which files are affected
2. **Check dependencies**: Ensure predecessor tasks are complete
3. **Create branch**: Use branch naming convention above
4. **Implement**: Follow coding conventions
5. **Write tests**: Unit tests for logic, integration tests for cross-crate behavior
6. **Verify**: Run build verification checklist
7. **Commit**: Use conventional commit messages
8. **Mark complete**: Check off the task in `tasks.md`

### Parallel Task Rules

Tasks marked `[P]` in `tasks.md` can be implemented in parallel:
- They modify different files with no shared dependencies
- They can be developed on separate branches and merged independently
- Merge conflicts, if any, should be trivial (different files or additive changes)

Tasks WITHOUT `[P]` must be executed sequentially in the order listed.

## Configuration Precedence

When developing features that read configuration:

1. CLI flags / environment variables (highest priority)
2. Project config: `<repo>/.cruxe/config.toml`
3. Global config: `~/.cruxe/config.toml`
4. Built-in defaults in `configs/default.toml` (lowest priority)

Environment variable prefix: `CRUXE_`

## Performance-Sensitive Code

Areas where performance matters (see [benchmark-targets.md](benchmark-targets.md)):

| Area | Target | Guideline |
|------|--------|-----------|
| Tantivy queries | p95 < 300ms | Use segment-level optimizations, avoid full-index scans |
| SQLite queries | p95 < 50ms | Use indexed columns, prepared statements, limit result sets |
| tree-sitter parsing | < 60s for 5k files | Process files in parallel, skip binary/ignored files early |
| MCP response serialization | minimal overhead | Use `serde` skip_serializing_if, avoid deep clones |
| Token estimation | O(n) in text length | Simple whitespace-split * 1.3, no regex |

When in doubt, benchmark with `hyperfine` or `tracing` spans before optimizing.
