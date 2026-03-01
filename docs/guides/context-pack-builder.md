# Context Pack Builder Guide

`build_context_pack` produces deterministic, sectioned context for coding agents.

## Modes

- `full`: balanced recall across definitions/usages/deps/tests/config/docs
- `edit_minimal` (`aider_minimal` alias): minimal context for focused edits

## Probe behavior

In `full` mode, probe queries are language-aware:

- Rust deps probe uses `use`
- Go deps probe uses `package`
- Python/TypeScript deps probe uses `import`

## Aider integration example

Use a context pack as the prompt context for an Aider edit loop:

1. Fetch context pack from MCP:
   - tool: `build_context_pack`
   - params:
     - `query`: your task (for example `"fix token refresh race condition"`)
     - `mode`: `edit_minimal` for focused patches, `full` for broader refactors
     - `budget_tokens`: start with `4000`
2. Feed ordered sections (`definitions`, `usages`, `deps`, `tests`) into Aider context.
3. Apply edits.
4. Re-run `build_context_pack` with the same query to validate missing-context hints and follow-up queries.

## Suggested defaults

- bugfix in one module: `mode=edit_minimal`, `budget_tokens=2500`
- cross-module refactor: `mode=full`, `budget_tokens=6000`
- slow CI or large monorepo: lower `max_candidates` and tighten section caps
