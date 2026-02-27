# Claude Code Integration

## Prerequisites

- `codecompass` installed and available on `PATH`
- A project repository initialized with CodeCompass
- Claude Code with MCP enabled

## 1) Install and Index

```bash
# From your project root
codecompass init
codecompass index
codecompass doctor
```

## 2) Configure MCP

Copy `configs/mcp/claude-code.json` into your Claude Code MCP config and set:

- `${CODECOMPASS_WORKSPACE}` -> absolute project path
- `${CODECOMPASS_CONFIG}` -> optional explicit config file path (or keep empty)

Template format uses `mcp_servers` and launches:

```bash
codecompass serve-mcp --workspace <absolute-project-path>
```

## 3) First Run

1. Restart Claude Code.
2. Open MCP tool list and verify `codecompass` tools are present.
3. Run a smoke call such as `health_check`.

## 4) Example Workflow

1. `search_code` with a natural-language query.
2. `locate_symbol` for exact definitions.
3. `get_file_outline` for file structure.
4. `find_references` when tracing usage.

Recommended prompt rule:

> use CodeCompass tools before file reads

## Troubleshooting

### Tools not showing

- Check Claude Code MCP config JSON parses successfully.
- Confirm `codecompass` is in `PATH`.
- Run `codecompass serve-mcp --workspace <path>` manually to confirm startup.

### Results look stale

- Run `codecompass sync --workspace <path>`.
- Re-run `codecompass doctor`.
- For branch-heavy repos, run `switch_ref` before query calls.

### Permission errors

- Ensure workspace path is readable.
- Avoid symlinked workspace paths that resolve outside allowed roots.
- Verify your shell/user in Claude Code can execute `codecompass`.
