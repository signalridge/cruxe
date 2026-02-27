# Copilot Integration

## Current Status

Direct MCP server integration for Copilot clients is evolving and may vary by host IDE/version.

For now, use the CLI workflow below as the stable fallback.

## Fallback Workflow (CLI-first)

```bash
codecompass init
codecompass index
codecompass doctor
codecompass search "auth middleware"
codecompass sync --workspace .
```

## Suggested Usage Pattern with Copilot

1. Use CodeCompass CLI commands in terminal for retrieval.
2. Paste focused results (paths/symbols) into Copilot chat.
3. Ask Copilot to reason over those exact symbols/files.

## Migration Path Once MCP Is Available

When your Copilot surface adds MCP config support:

1. Start from `configs/mcp/generic.json`.
2. Map fields to Copilot MCP schema.
3. Verify `tools/list` shows the CodeCompass tools.

## Troubleshooting

- If Copilot cannot call external tools, use CLI fallback above.
- If search accuracy drops after ref changes, run `codecompass sync`.
- If symbol lookups fail unexpectedly, run `codecompass doctor`.
