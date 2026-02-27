## Context

`001-004` is working and tested, but the implementation still has structural
friction for continued delivery:

1. `stdio` and `http` maintain separate `tools/call` dispatch flows.
2. `crates/cruxe-mcp/src/server/tool_calls.rs` was a conflict hotspot.
3. `index_repo` and `bootstrap_and_index` used partially divergent index launch
   logic.
4. Protocol error codes were hard-coded in multiple locations.
5. Critical enum-like config values (freshness/explainability) were still
   string-heavy in runtime paths.
6. Some p95 performance checks were in standard test lanes and caused CI noise.
7. Governance requirements from `specs/meta/repo-maintenance.md` were only
   partially automated.
8. SQLite connections were repeatedly opened in request paths.
9. `005-009` prerequisite abstractions (`VcsAdapter` boundary, overlay merge
   key domain type, `symbol_edges` indexing strategy, typed semantic config,
   parallel guardrails) were not yet codified as enforceable constraints.

These issues do not immediately break functionality, but they increase merge
conflicts, regressions, and maintenance cost as `005-009` work scales up.

## Goals / Non-Goals

**Goals**

- Build one shared MCP execution path for `stdio/http` to eliminate duplicate
  behavior branches.
- Split `tool_calls` into domain modules that support parallel ownership.
- Unify index process launch semantics (binary resolution, env propagation,
  errors).
- Centralize typed protocol error mapping aligned to
  `specs/meta/protocol-error-codes.md`.
- Normalize critical config paths with typed behavior while keeping legacy
  compatibility.
- Add extensible SQLite connection lifecycle management.
- Separate benchmark gates from deterministic smoke checks.
- Land governance automation (CI/security/PR title/OpenSpec trace gate/pre-commit).
- Add `005-009` readiness abstractions and guardrails without shipping full
  `005-009` feature behavior.

**Non-Goals**

- Do not implement full end-to-end `005-009` feature behavior in this change.
- Do not rename released MCP tools or break core request semantics.
- Do not add new external runtime services/dependencies.
- Do not perform destructive historical migrations.

## Decisions

### Decision 1: Unify MCP execution pipeline (transport-agnostic dispatch)

Create a shared execution path (dispatch + runtime context). `stdio/http`
remain transport adapters responsible only for decode/encode and transport
surface handling.

**Rationale**
- Implement tool behavior once.
- Eliminate cross-transport behavior drift.

**Alternatives considered**
- Keep dual paths and add tests only (insufficient long-term).
- Proxy HTTP to stdio subprocess (adds process/error complexity).

### Decision 2: Split `tool_calls` by domain with stable public entrypoint

Split into `query/structure/context/index/health/status/shared`, while keeping
stable external entrypoints.

**Rationale**
- Reduce merge conflicts and review burden.
- Enable focused tests and ownership.

**Alternatives considered**
- One file per tool (too fragmented for shared logic).
- Keep monolith with comments (does not solve hotspot risk).

### Decision 3: Introduce unified `IndexProcessLauncher`

Route both explicit indexing (`index_repo`) and bootstrap indexing through one
launcher abstraction with deterministic binary/env/argument/error behavior.

**Rationale**
- Prevent launch behavior drift.
- Make launch behavior independently testable.

**Alternatives considered**
- Continue patching each callsite independently (keeps duplication).

### Decision 4: Centralize typed protocol error mapping

Introduce `ProtocolErrorCode` (or equivalent typed representation) and map
payload errors from this typed source; avoid adding new raw string literals.

**Rationale**
- One source of truth for protocol codes.
- Better consistency with `specs/meta/protocol-error-codes.md`.

### Decision 5: Typed normalization for critical config with compatibility

Normalize `freshness_policy`, `ranking_explain_level`, and semantic config at
config load/entry points while accepting legacy values.

**Rationale**
- Reduce runtime string branching.
- Provide stable ground for `008` expansion.

### Decision 6: Performance verification split (benchmark gate + smoke guard)

Move environment-sensitive p95 assertions to benchmark harnesses; retain
deterministic smoke assertions in regular tests.

**Rationale**
- Reduce CI flakiness.
- Keep regression signal quality.

### Decision 7: Governance automation as baseline

Automate and enforce:
- CI workflow (fmt/clippy/test/build)
- security workflow (secret + dependency baseline + SARIF support)
- PR title policy
- OpenSpec trace gate
- local pre-commit baseline

**Rationale**
- Convert policy docs into executable governance.

### Decision 8: Runtime SQLite connection lifecycle management

Introduce `ConnectionManager` (lightweight manager abstraction) for lazy open,
reuse, and invalidate/reopen behavior across transport paths.

**Rationale**
- Reduce repeated open/close churn.
- Improve degraded-path recovery behavior consistency.

### Decision 9: `005-009` readiness foundations (without full feature delivery)

Land prerequisite abstractions:
- `005`: `VcsAdapter` trait boundary and overlay merge key domain type
- `006/007`: `symbol_edges` index strategy (`from/to + edge_type`)
- `008`: typed semantic config substructure with compatibility normalization
- `009`: minimal release governance baseline via automation

**Rationale**
- Shift expensive refactoring left.
- Lower integration risk in upcoming specs.

### Decision 10: Add parallel development guardrails to execution order

Document:
- module owners and review boundaries
- high-conflict paths
- approved parallel touchpoint matrix
- change-scope constraints for multi-stream delivery

**Rationale**
- Improve stability for multi-developer parallel work.

### Decision 11: Shared health core with explicit surface boundaries

Treat `GET /health` (HTTP readiness probe) and MCP `health_check` as distinct
contracts that share core health assembly but retain surface-specific fields.

Implementation pattern:
- shared health-core payload builder
- surface adapters:
  - `GET /health`: HTTP contract fields only
  - `health_check`: MCP metadata/grammar/tool-contract fields
- shared helpers for schema-status mapping and
  `interrupted_recovery_report` payload construction

**Rationale**
- Reduce duplicate logic while preserving intentional contract differences.

### Decision 12: No silent degrade on critical runtime paths

For degraded-but-continue paths (DB/index open failure, workspace resolution
rejection):
- return contract-compatible degraded responses
- emit structured `tracing::warn!` logs with relevant dimensions
- avoid bare `.ok()` in critical paths unless logging already happened

**Rationale**
- Production diagnosis needs observable breadcrumbs.

### Decision 13: Gradual typed payload migration for complex responses

For complex responses (`health_check`, `index_status`, `GET /health`), migrate
incrementally from handwritten `json!` to typed `#[derive(Serialize)]` payloads.

**Rationale**
- Reduce field drift risk.
- Improve reviewability and change safety.

### Decision 14: Define transport extension seam for WebSocket/SSE (doc-first)

Do not implement new transports now, but enforce extension seam constraints:
- `DispatchRuntime`: transport-agnostic runtime state
- `TransportExecutionContext`: transport-side variance
- `execute_transport_request`: single JSON-RPC execution entrypoint
- transport layers handle decode/encode/network only; no inlined tool-logic
  branches

**Rationale**
- Prevent future reintroduction of duplicated dispatch logic.

## Risks / Trade-offs

- **Risk**: module split introduces subtle behavior differences  
  **Mitigation**: lock behavior with parity/regression tests before and after split.
- **Risk**: typed error mapping changes snapshots  
  **Mitigation**: compatibility map first, then update snapshots deliberately.
- **Risk**: unified launcher changes edge-case behavior  
  **Mitigation**: preserve env override and add launcher parity tests.
- **Risk**: benchmark harness adds process complexity  
  **Mitigation**: start with minimal harness and fixed fixtures.
- **Risk**: governance rollout increases early PR failures  
  **Mitigation**: provide local parity commands and clear remediation.
- **Risk**: connection manager adds state complexity  
  **Mitigation**: keep v1 lightweight and validate reconnect semantics in tests.
- **Risk**: readiness foundations blur into feature scope  
  **Mitigation**: explicit acceptance criteria: foundation only, not full features.
- **Risk**: guardrails are documented but inconsistently applied  
  **Mitigation**: reference guardrails in PR/review governance docs and checks.

## Migration Plan

1. **Phase A - Baseline safety**
   - Add and freeze parity/regression tests (dispatch, error code, launcher,
     health/status).
2. **Phase B - Execution unification**
   - Move `stdio/http` to shared transport-agnostic execution path.
3. **Phase C - Module decomposition**
   - Split `tool_calls` by domain while preserving entrypoint behavior.
4. **Phase D - Error/config normalization**
   - Add typed protocol error mapping and typed config normalization.
5. **Phase E - Performance lane split**
   - Move strict p95 assertions to benchmark harness; keep smoke guards.
6. **Phase F - Governance rollout**
   - Land CI/security/PR-title/trace-gate/pre-commit and maintenance docs.
7. **Phase G - Connection lifecycle**
   - Integrate `ConnectionManager` for stdio/http runtime paths.
8. **Phase H - `005-009` readiness**
   - Land readiness abstractions and parallel guardrail docs.

**Rollback strategy**

- Keep changes phase-scoped to support phase-level rollback.
- Preserve a controlled fallback seam if shared dispatch causes regressions.
- Keep compatibility mapping in error/config layers to avoid unreadable config
  or payload rollback hazards.

## Open Questions

1. Should `ProtocolErrorCode` live in `cruxe-core` or
   `cruxe-mcp`?  
   **Recommendation:** `cruxe-core` for cross-transport reuse.
2. Should benchmarking start with custom harness or `criterion`?  
   **Recommendation:** start with minimal harness, adopt `criterion` when needed.
3. Should OpenSpec trace gate be required immediately or staged?  
   **Recommendation:** stage if needed operationally, but keep enforcement path explicit.
4. Should v1 connection lifecycle remain one-connection-per-db-path with
   reopen-on-failure?  
   **Recommendation:** yes, keep it lightweight first.
5. Should `VcsAdapter` start as trait + minimal implementation or full `git2`
   behavior now?  
   **Recommendation:** trait + minimal implementation first.

## Validation Log (2026-02-25)

- `cargo fmt --all --check` ✅
- `cargo clippy --workspace -- -D warnings` ✅
- `cargo test --workspace` ✅
- `openspec validate harden-001-004-maintainability --type change --strict` ✅
