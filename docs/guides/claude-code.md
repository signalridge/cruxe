# Claude Code Integration

## Prerequisites

- `cruxe` installed and available on `PATH`
- A project repository initialized with Cruxe
- Claude Code with MCP enabled

## 1) Install and Index

```bash
# From your project root
cruxe init
cruxe index
cruxe doctor
```

## 2) Configure MCP

Copy `configs/mcp/claude-code.json` into your Claude Code MCP config and set:

- `${CRUXE_WORKSPACE}` -> absolute project path
- `${CRUXE_CONFIG}` -> optional explicit config file path (or keep empty)

Template format uses `mcp_servers` and launches:

```bash
cruxe serve-mcp --workspace <absolute-project-path>
```

## 3) First Run

1. Restart Claude Code.
2. Open MCP tool list and verify `cruxe` tools are present.
3. Run a smoke call such as `health_check`.

## 4) Example Workflow

1. `search_code` with a natural-language query.
2. `locate_symbol` for exact definitions.
3. `get_file_outline` for file structure.
4. `find_references` when tracing usage.

Recommended prompt rule:

> use Cruxe tools before file reads

## Troubleshooting

### Tools not showing

- Check Claude Code MCP config JSON parses successfully.
- Confirm `cruxe` is in `PATH`.
- Run `cruxe serve-mcp --workspace <path>` manually to confirm startup.

### Results look stale

- Run `cruxe sync --workspace <path>`.
- Re-run `cruxe doctor`.
- For branch-heavy repos, run `switch_ref` before query calls.

### Permission errors

- Ensure workspace path is readable.
- Avoid symlinked workspace paths that resolve outside allowed roots.
- Verify your shell/user in Claude Code can execute `cruxe`.
