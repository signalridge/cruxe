# Parallel Development Guardrails

> Cross-team collaboration guardrails for concurrent work on specs 005-009.

## High-Conflict Paths

| Path | Risk | Guardrail |
|---|---|---|
| `crates/cruxe-mcp/src/server.rs` | transport/runtime coupling | Single-owner per PR for dispatcher/runtime edits |
| `crates/cruxe-mcp/src/server/tool_calls/**` | tool contract drift | Domain module ownership (`query/structure/context/index/health/status`) |
| `crates/cruxe-state/src/schema.rs` | migration conflicts | One migration author at a time; append-only migration policy |
| `crates/cruxe-core/src/config.rs` | compatibility regressions | Typed normalization changes require config regression tests |
| `specs/**` + `openspec/**` | traceability drift | Code changes must include matching spec/change updates |

## Suggested Module Owners

| Module Area | Primary Owner Role | Review Requirement |
|---|---|---|
| MCP transport/runtime | Runtime maintainer | 1 reviewer from protocol/contracts |
| Query/ranking logic | Retrieval maintainer | 1 reviewer from search relevance |
| SQLite schema/state | Data/state maintainer | 1 reviewer from migration/governance |
| Config + compatibility | Core platform maintainer | 1 reviewer from release/distribution |
| Governance workflows | DevEx maintainer | 1 reviewer from security/release |

## Approved Parallel Touchpoints

The following workstreams can proceed in parallel with low conflict risk:

1. `cruxe-query/**` relevance tuning + `specs/008-*` updates.
2. `cruxe-state/src/edges.rs` traversal/index tests + `specs/006-007`.
3. CI/security workflow updates under `.github/**`.
4. Docs-only updates in `specs/meta/**` (except migration-critical docs).

## Change Boundaries

- Keep one PR focused on one primary module group.
- Avoid touching both `server.rs` and `schema.rs` unless absolutely necessary.
- If a PR modifies shared contract fields (`metadata`, `error.code`), include:
  - transport parity test update,
  - OpenSpec artifact update,
  - contract doc delta.

## Review Checklist (Parallel Safety)

Before merge, confirm:

1. No unresolved overlap with active PRs on high-conflict paths.
2. OpenSpec task/spec trace is present.
3. Migration changes are append-only and idempotent.
4. Transport parity tests still pass for modified tools.
