# Cruxe

Code search and navigation engine for AI coding assistants.

## Features

- **Multi-language symbol extraction** -- Rust, TypeScript, Python, and Go via tree-sitter
- **Full-text code search** with intent classification (symbol, path, error, natural language)
- **Symbol location** with definition-first ranking
- **MCP server** (Model Context Protocol) for AI agent integration over stdio
- **Incremental indexing** with content-hash-based change detection (blake3)
- **Ref-scoped search** -- branch-level isolation for worktree correctness

## Installation

```bash
cargo install --path crates/cruxe-cli
```

### Prebuilt Releases

Prebuilt archives are published on GitHub Releases for:

- macOS: `aarch64`, `x86_64`
- Linux: `x86_64`, `aarch64` (musl static)
- Windows: `x86_64`

Checksums are published alongside each release.

## Quick Start

```bash
# Initialize Cruxe in your project
cruxe init

# Index the codebase
cruxe index

# Search for symbols or code
cruxe search "validate_token"

# Check project health
cruxe doctor
```

## MCP Configuration

To use Cruxe as an MCP server (e.g. with Claude Desktop or similar tools), add the following to your MCP client configuration:

```json
{
  "mcpServers": {
    "cruxe": {
      "command": "cruxe",
      "args": ["serve-mcp", "--workspace", "/path/to/project"]
    }
  }
}
```

Ready-to-use templates are available in `configs/mcp/`:

- `claude-code.json`
- `cursor.json`
- `codex.json`
- `generic.json`
- `tool-schemas.json` (machine-readable tool schema export)

Human-readable schema reference: `docs/reference/mcp-tools-schema.md`
Agent guides: `docs/guides/`
Auto-indexing templates: `configs/templates/`

## Architecture

Cruxe is a Rust workspace with 6 crates:

| Crate | Responsibility |
|-------|---------------|
| `cruxe-core` | Shared types, constants, config, error types |
| `cruxe-state` | SQLite (rusqlite) + Tantivy storage layer |
| `cruxe-indexer` | tree-sitter parsing and per-language symbol extractors |
| `cruxe-query` | Search, locate, intent classification, ranking |
| `cruxe-mcp` | MCP JSON-RPC server (stdio transport) |
| `cruxe-cli` | clap-based CLI entry point |

Storage is fully embedded -- Tantivy for full-text search, SQLite (WAL mode) for structured data. No external services required.

## Specification Source of Truth

- Canonical product/design specs live under `specs/`.
- `openspec/` contains workflow artifacts and experimental change drafts.
- When behavior changes, update `specs/` first and only mirror into `openspec/` when required by workflow.

## CLI Commands

```
cruxe init [--path PATH]                      Initialize project configuration
cruxe index [--path PATH] [--ref REF] [--force]   Index source code
cruxe sync [--workspace PATH] [--force]           Incremental sync
cruxe search <query> [--ref REF] [--lang LANG]    Search code in the index
cruxe doctor [--path PATH]                        Check project health
cruxe serve-mcp [--workspace PATH]                Start MCP server (stdio transport)
```

## Search Intent Strategy Configuration

Intent classification is configurable via `search.intent` in config TOML (for example in
`.cruxe/config.toml` or your explicit `--config` file):

```toml
[search.intent]
rule_order = ["error_pattern", "path", "quoted_error", "symbol", "natural_language"]
error_patterns = ["error:", "panic:", "traceback", "thread '"]
path_extensions = [".rs", ".ts", ".py", ".go"]
symbol_kind_keywords = ["fn", "struct", "class", "interface"]
enable_wrapped_quoted_error_literal = true
```

Supported `rule_order` values:

- `error_pattern`
- `path`
- `quoted_error`
- `symbol`
- `natural_language`

Runtime environment variable overrides:

- `CRUXE_SEARCH_INTENT_RULE_ORDER` (CSV list)
- `CRUXE_SEARCH_INTENT_ERROR_PATTERNS` (CSV list)
- `CRUXE_SEARCH_INTENT_PATH_EXTENSIONS` (CSV list)
- `CRUXE_SEARCH_INTENT_SYMBOL_KIND_KEYWORDS` (CSV list)
- `CRUXE_SEARCH_INTENT_ENABLE_WRAPPED_QUOTED_ERROR_LITERAL` (`true/false`, `1/0`, `yes/no`, `on/off`)

## Semantic Query Tuning Configuration

Hybrid semantic tuning constants are config-backed via `search.semantic`:

```toml
[search.semantic]
# Confidence composite weights (defaults preserve legacy behavior)
confidence_top_score_weight = 0.55
confidence_score_margin_weight = 0.30
confidence_channel_agreement_weight = 0.15

# Local reranker boosts (defaults preserve legacy behavior)
local_rerank_phrase_boost = 0.75
local_rerank_token_overlap_weight = 2.5

# Semantic/hybrid fanout multipliers (defaults preserve legacy behavior)
semantic_limit_multiplier = 2
lexical_fanout_multiplier = 4
semantic_fanout_multiplier = 3
```

Relevant environment variable overrides:

- `CRUXE_SEMANTIC_CONFIDENCE_TOP_SCORE_WEIGHT`
- `CRUXE_SEMANTIC_CONFIDENCE_SCORE_MARGIN_WEIGHT`
- `CRUXE_SEMANTIC_CONFIDENCE_CHANNEL_AGREEMENT_WEIGHT`
- `CRUXE_SEMANTIC_LOCAL_RERANK_PHRASE_BOOST`
- `CRUXE_SEMANTIC_LOCAL_RERANK_TOKEN_OVERLAP_WEIGHT`
- `CRUXE_SEMANTIC_LIMIT_MULTIPLIER`
- `CRUXE_SEMANTIC_LEXICAL_FANOUT_MULTIPLIER`
- `CRUXE_SEMANTIC_SEMANTIC_FANOUT_MULTIPLIER`

## Verification

Default deterministic verification lane:

```bash
cargo fmt --all --check
cargo clippy --workspace -- -D warnings
CRUXE_ENABLE_FASTEMBED_RUNTIME=0 cargo test --workspace
```

Runtime-sensitive benchmark lane:

```bash
scripts/benchmarks/run_mcp_benchmarks.sh
```

Optional all-features lane (for feature-gated backends such as `lancedb`) requires
`protoc` preflight:

```bash
command -v protoc >/dev/null || {
  echo "Missing protoc. Install protobuf compiler first (e.g. brew install protobuf / apt-get install protobuf-compiler)."
  exit 1
}
protoc --version

cargo clippy --workspace --all-features -- -D warnings
CRUXE_ENABLE_FASTEMBED_RUNTIME=0 cargo test --workspace --all-features
```

## License

MIT
