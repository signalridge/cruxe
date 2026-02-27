## Why

The `001-004` scope is functionally working and tests are passing, but the
current implementation still has structural bottlenecks for ongoing
development:

- duplicated `tools/call` dispatch logic between `stdio` and `http`
- an oversized `tool_calls` hotspot file
- non-unified index subprocess launch paths
- scattered protocol error strings and governance checks

As work moves into `005-009`, these issues would amplify merge conflicts,
regressions, and maintenance cost. This change hardens the architecture now,
before that scale-up.

## What Changes

- Unify MCP tool execution behind one shared dispatch/execution pipeline used by
  both `stdio` and `http`.
- Decompose `tool_calls` into domain modules
  (`query/structure/context/index/health/status`) to reduce conflict surface and
  improve testability.
- Unify index process launching for `index_repo` and `bootstrap_and_index`
  through one launcher abstraction with consistent binary resolution, environment
  propagation, and error semantics.
- Replace ad-hoc protocol error strings with centralized typed mapping aligned
  to `specs/meta/protocol-error-codes.md`.
- Strengthen config type boundaries by normalizing high-traffic enum configs
  (freshness/explainability/semantic mode) at runtime entrypoints.
- Add reusable runtime SQLite connection lifecycle management (long-lived
  manager / lightweight pooling strategy) to reduce repeated open/close churn.
- Split performance verification: move environment-sensitive p95 assertions to a
  repeatable benchmark lane and keep deterministic smoke guards in default test
  lane.
- Clarify health contract boundaries: keep `GET /health` and MCP `health_check`
  as distinct surfaces while sharing core health state assembly.
- Improve degraded-path observability by adding structured logs for DB/index
  open failures and workspace-resolution rejections.
- Complete repository governance automation:
  CI/lint/test/security/pr-title/OpenSpec trace gate plus local pre-commit
  baseline.
- Add readiness guardrails for `005-009` by landing:
  `VcsAdapter` boundary skeleton, unified overlay merge key domain type,
  `symbol_edges` index strategy, typed semantic config substructure, and
  parallel-development guardrail docs.

## Capabilities

### New Capabilities

- `repo-governance-automation`: executable and verifiable governance automation
  for CI, security checks, PR title policy, and OpenSpec trace gate.
- `runtime-connection-lifecycle`: sustainable runtime SQLite connection
  lifecycle management.
- `vcs-core-readiness`: `VcsAdapter` boundary skeleton and overlay merge key
  domain constraints for `005`.
- `graph-index-readiness`: `symbol_edges` query/index strategy groundwork for
  `006/007`.
- `semantic-config-readiness`: typed `semantic_mode/profile` config structure
  with compatibility behavior for `008`.
- `parallel-development-guardrails`: execution-order-based module ownership and
  parallel touchpoint guardrails.

### Modified Capabilities

- `001-core-mvp`: require consistent index execution entrypoints and unified
  error semantics across core runtime paths.
- `002-agent-protocol`: strengthen protocol error-code consistency and
  cross-transport explainability/freshness behavior.
- `003-structure-nav`: improve maintainability through module boundaries without
  changing external tool semantics.
- `004-workspace-transport`: require shared `stdio/http` execution semantics,
  observability consistency, and benchmark/smoke split validation.

## Impact

- Affected code:
  - `crates/cruxe-mcp/src/server.rs`
  - `crates/cruxe-mcp/src/http.rs`
  - `crates/cruxe-mcp/src/server/tool_calls.rs`
  - `crates/cruxe-mcp/src/server/tool_calls/*` (post-split)
  - `crates/cruxe-core/src/config.rs`
  - `crates/cruxe-core/src/types.rs`
  - `crates/cruxe-core/src/error.rs`
  - `crates/cruxe-cli/src/commands/serve_mcp.rs`
  - `crates/cruxe-state/src/schema.rs`
  - `crates/cruxe-state/src/edges.rs`
  - `crates/cruxe-state/src/jobs.rs`
- Affected specs/docs:
  - `specs/001-core-mvp/*`
  - `specs/002-agent-protocol/*`
  - `specs/003-structure-nav/*`
  - `specs/004-workspace-transport/*`
  - `specs/005-vcs-core/*`
  - `specs/006-vcs-ga-tooling/*`
  - `specs/007-call-graph/*`
  - `specs/008-semantic-hybrid/*`
  - `specs/009-distribution/*`
  - `specs/meta/execution-order.md`
  - `specs/meta/protocol-error-codes.md`
  - `specs/meta/repo-maintenance.md`
- Affected workflows/systems:
  - `.github/workflows/*` (new or expanded)
  - `.pre-commit-config.yaml`
- Compatibility:
  - Preserve existing MCP tool schema compatibility by default.
  - Any breaking behavior adjustment must be explicitly marked as **BREAKING** in
    the corresponding spec delta.
