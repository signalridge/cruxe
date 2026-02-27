# Codex Integration

## Prerequisites

- `codecompass` available on `PATH`
- Codex client with MCP server configuration support
- Indexed workspace

## 1) Initialize and Verify Index

```bash
codecompass init
codecompass index
codecompass doctor
```

## 2) Configure MCP

Use `configs/mcp/codex.json` as the starter template.

Set:

- `${CODECOMPASS_WORKSPACE}` -> absolute path to project
- `${CODECOMPASS_CONFIG}` -> optional explicit config path

The template launches CodeCompass in stdio MCP mode:

```bash
codecompass serve-mcp --workspace <absolute-project-path>
```

## 3) Verify Tool Availability

1. Restart Codex client/session.
2. Confirm tools are visible.
3. Run `health_check` and `search_code` as a smoke test.

## Recommended Workflow

1. Start with `search_code` for broad context.
2. Narrow with `locate_symbol`.
3. Use `find_references`/`get_call_graph` before editing related code.
4. Re-run `sync_repo` after local commits.

## Troubleshooting

### Tool startup timeout

- Ensure `codecompass` binary is discoverable on `PATH`.
- Increase startup timeout in client config if supported.

### Tools missing after config update

- Validate JSON config structure.
- Restart the client.

### Branch mismatch in results

- Use `switch_ref` to align tool queries with your target branch.
