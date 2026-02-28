# MCP Tool Schema Reference

This document is the human-readable companion to `configs/mcp/tool-schemas.json`.
The JSON file is the canonical machine-readable source.

- Generated from: MCP `tools/list` response
- Generator script: `scripts/generate_mcp_tool_schemas.sh`
- Current tool count: 19

## Regenerate

```bash
./scripts/generate_mcp_tool_schemas.sh . configs/mcp/tool-schemas.json
```

## Validation

```bash
jq empty configs/mcp/tool-schemas.json
jq -r '.tools[].name' configs/mcp/tool-schemas.json
```

## Tool Index

| Tool | Required Input Fields | Purpose |
| --- | --- | --- |
| `index_repo` | none | Trigger full or incremental indexing. |
| `sync_repo` | none | Trigger incremental sync since last indexed state. |
| `search_code` | `query` | Search symbols/snippets/files with intent classification. |
| `locate_symbol` | `name` | Locate symbol definitions with file:line output. |
| `get_file_outline` | `path` | Return symbol outline for one file. |
| `get_call_graph` | `symbol_name` | Return callers/callees graph with bounded depth. |
| `compare_symbol_between_commits` | `symbol_name`, `base_ref`, `head_ref` | Compare one symbol between two refs. |
| `get_symbol_hierarchy` | `symbol_name` | Return ancestor/descendant symbol hierarchy. |
| `find_related_symbols` | `symbol_name` | Find nearby symbols in file/module/package scope. |
| `get_code_context` | `query` | Return token-budgeted context blocks. |
| `build_context_pack` | `query` | Build deterministic sectioned context packs with provenance and diagnostics. |
| `suggest_followup_queries` | `previous_query`, `previous_results` | Suggest next tool calls for weak/empty results. |
| `health_check` | none | Check operational status and warm/index state. |
| `index_status` | none | Return indexing status and recent jobs. |
| `diff_context` | none | Summarize symbol-level changes across refs. |
| `find_references` | `symbol_name` | Return references from relation graph edges. |
| `explain_ranking` | `query`, `result_path`, `result_line_start` | Explain ranking contribution for one search result. |
| `list_refs` | none | List indexed refs and branch metadata. |
| `switch_ref` | `ref` | Switch default ref for current MCP session. |

## Common Optional Fields

Most query/navigation tools also accept these optional fields:

- `workspace`: absolute workspace path override
- `ref`: branch/ref scope override
- `limit`: result cap
- `language`: language filter (when applicable)
- `detail_level`: response verbosity (`location`, `signature`, `context`) for supported tools
- `freshness_policy`: strictness of stale-index handling (`strict`, `balanced`, `best_effort`) for supported tools

## Example Calls

### `search_code`

```json
{
  "name": "search_code",
  "arguments": {
    "query": "validate_token",
    "limit": 10,
    "detail_level": "signature"
  }
}
```

### `locate_symbol`

```json
{
  "name": "locate_symbol",
  "arguments": {
    "name": "AuthHandler",
    "workspace": "/abs/path/to/repo"
  }
}
```

### `switch_ref`

```json
{
  "name": "switch_ref",
  "arguments": {
    "ref": "feat/new-branch"
  }
}
```

### `build_context_pack`

```json
{
  "name": "build_context_pack",
  "arguments": {
    "query": "validate_token",
    "budget_tokens": 400,
    "mode": "edit_minimal",
    "section_caps": {
      "definitions": 6,
      "usages": 4,
      "deps": 2
    }
  }
}
```

Notes:
- `budget_tokens` accepts `1..=200000`.
- Response section keys are canonicalized as `definitions`, `usages`, `deps`, `tests`, `config`, `docs`.
- Metadata includes `section_aliases` for Continue-style naming (`key_usages` -> `usages`, `dependencies` -> `deps`).
- Mode alias: `aider_minimal` is accepted as an alias for `edit_minimal`.
- Token estimates use `cruxe_core::tokens::estimate_tokens` with a minimum of 8 tokens per selected item.
- Metadata includes `budget_utilization_ratio`, and underfilled packs include guidance in `missing_context_hints`.

## Version Alignment Rule

When MCP tool schemas change:

1. Regenerate `configs/mcp/tool-schemas.json`.
2. Update this reference file.
3. Verify agent templates in `configs/mcp/*.json` still match the released tool surface.
4. Mention schema changes in release notes.
