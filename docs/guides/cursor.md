# Cursor Integration

## Prerequisites

- `cruxe` installed on `PATH`
- Cursor with MCP configuration support
- A repository already indexed by Cruxe

## 1) Initialize Project

```bash
cruxe init
cruxe index
cruxe doctor
```

## 2) Configure MCP in Cursor

Use `configs/mcp/cursor.json` as the base template.

Set:

- `${CRUXE_WORKSPACE}` -> absolute path of the repo
- `${CRUXE_CONFIG}` -> optional config file path

Cursor template format uses `mcpServers`.

## 3) Verify Connection

1. Restart Cursor.
2. Open MCP settings/tool panel.
3. Confirm Cruxe tools are listed.
4. Run `health_check`.

## 4) Recommended Query Flow

1. `search_code` for first-pass retrieval.
2. `locate_symbol` for precise symbol navigation.
3. `get_call_graph` / `find_references` for impact analysis.

## Troubleshooting

### No MCP tools displayed

- Validate JSON syntax in Cursor MCP config.
- Confirm workspace path exists.
- Run `cruxe serve-mcp --workspace <path>` locally to verify the command.

### Incomplete results

- Re-index: `cruxe sync --workspace <path>`.
- If branch changed, call `switch_ref`.
- Increase `limit` in `search_code`.

### Slow first call

- Normal when prewarm runs.
- Use `health_check` to inspect warm/index status.
