# CodeCompass

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
cargo install --path crates/codecompass-cli
```

## Quick Start

```bash
# Initialize CodeCompass in your project
codecompass init

# Index the codebase
codecompass index

# Search for symbols or code
codecompass search "validate_token"

# Check project health
codecompass doctor
```

## MCP Configuration

To use CodeCompass as an MCP server (e.g. with Claude Desktop or similar tools), add the following to your MCP client configuration:

```json
{
  "mcpServers": {
    "codecompass": {
      "command": "codecompass",
      "args": ["serve-mcp", "--workspace", "/path/to/project"]
    }
  }
}
```

## Architecture

CodeCompass is a Rust workspace with 6 crates:

| Crate | Responsibility |
|-------|---------------|
| `codecompass-core` | Shared types, constants, config, error types |
| `codecompass-state` | SQLite (rusqlite) + Tantivy storage layer |
| `codecompass-indexer` | tree-sitter parsing and per-language symbol extractors |
| `codecompass-query` | Search, locate, intent classification, ranking |
| `codecompass-mcp` | MCP JSON-RPC server (stdio transport) |
| `codecompass-cli` | clap-based CLI entry point |

Storage is fully embedded -- Tantivy for full-text search, SQLite (WAL mode) for structured data. No external services required.

## CLI Commands

```
codecompass init [--path PATH]                      Initialize project configuration
codecompass index [--path PATH] [--ref REF] [--force]   Index source code
codecompass sync [--workspace PATH] [--force]           Incremental sync
codecompass search <query> [--ref REF] [--lang LANG]    Search code in the index
codecompass doctor [--path PATH]                        Check project health
codecompass serve-mcp [--workspace PATH]                Start MCP server (stdio transport)
```

## Search Intent Strategy Configuration

Intent classification is configurable via `search.intent` in config TOML (for example in
`.codecompass/config.toml` or your explicit `--config` file):

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

- `CODECOMPASS_SEARCH_INTENT_RULE_ORDER` (CSV list)
- `CODECOMPASS_SEARCH_INTENT_ERROR_PATTERNS` (CSV list)
- `CODECOMPASS_SEARCH_INTENT_PATH_EXTENSIONS` (CSV list)
- `CODECOMPASS_SEARCH_INTENT_SYMBOL_KIND_KEYWORDS` (CSV list)
- `CODECOMPASS_SEARCH_INTENT_ENABLE_WRAPPED_QUOTED_ERROR_LITERAL` (`true/false`, `1/0`, `yes/no`, `on/off`)

## License

MIT
