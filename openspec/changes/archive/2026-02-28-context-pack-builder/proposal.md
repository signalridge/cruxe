## Why

Search hit lists are useful for experts, but AI coding agents need a compact, structured, and budget-aware context bundle that can be sent directly to a model with provenance and next-step guidance.

## What Changes

1. Add a new MCP capability to build context packs from a query + token budget.
2. Assemble packs as structured sections (`definitions`, `key_usages`, `dependencies`, `tests`, `config`, `docs`).
3. Add provenance envelope for every selected snippet (`ref`, `path`, `range`, `content_hash`, `selection_reason`).
4. Add pack diagnostics (`token_budget_used`, `dropped_candidates`, `suggested_next_queries`).

## Capabilities

### New Capabilities
- `context-pack-builder`: deterministic, token-budgeted context packaging for AI agent workflows.

### Modified Capabilities
- `002-agent-protocol`: adds `build_context_pack` tool contract and response metadata semantics.

## Impact

- Affected crates: `cruxe-query`, `cruxe-mcp`, `cruxe-core`, optionally `cruxe-state` for snippet provenance lookups.
- API impact: introduces a new MCP tool and additive metadata fields.
- Product impact: upgrades Cruxe from retrieval engine to context orchestration layer for agents.
- Performance impact: adds pack assembly stage with bounded budget controls.
