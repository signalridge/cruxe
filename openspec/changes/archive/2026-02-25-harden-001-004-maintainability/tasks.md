## 1. Baseline Lock and Refactor Safety Net

- [x] 1.1 Add transport-parity regression tests in `crates/cruxe-mcp/src/server/tests.rs` and `crates/cruxe-mcp/src/http.rs` to lock equivalent stdio/http semantics for representative success/error paths.
- [x] 1.2 Add launcher-behavior regression tests in `crates/cruxe-mcp/src/server/tests.rs` for binary resolution order and env propagation (`CRUXE_INDEX_BIN`, `CRUXE_PROJECT_ID`, `CRUXE_STORAGE_DATA_DIR`, `CRUXE_JOB_ID`).
- [x] 1.3 Add compatibility snapshot tests for canonical protocol error envelope and canonical metadata enums in `crates/cruxe-mcp/src/server/tests.rs`.

## 2. Transport-Agnostic Tool Execution Pipeline (004)

- [x] 2.1 Extract shared request execution pipeline in `crates/cruxe-mcp/src/server.rs` (or new shared module) to own workspace resolution, runtime/schema checks, and tool dispatch.
- [x] 2.2 Refactor stdio path in `crates/cruxe-mcp/src/server.rs` to use the shared execution pipeline with transport-specific decode/encode only.
- [x] 2.3 Refactor HTTP JSON-RPC path in `crates/cruxe-mcp/src/http.rs` to use the same shared execution pipeline with transport-specific decode/encode only.
- [x] 2.4 Verify and update parity tests ensuring equivalent semantic outputs across transports for success, validation errors, and compatibility failures.
- [x] 2.5 Extract shared health-core assembler used by `GET /health` and `health_check`, while preserving endpoint-specific contract envelopes.
- [x] 2.6 Extract shared helper(s) for `SchemaStatus -> contract string` mapping and `interrupted_recovery_report` payload construction; remove duplicated inline `match/json` blocks.
- [x] 2.7 Add explicit contract tests that lock both shared fields and intentional surface differences (`GET /health` vs `health_check`) to prevent false-positive regressions.
- [x] 2.8 Document and enforce a transport extension seam (trait/adapter-level design contract) for future WebSocket/SSE transports without duplicating dispatch/router/health logic.

## 3. Handler Decomposition and Module Boundaries (003)

- [x] 3.1 Split `crates/cruxe-mcp/src/server/tool_calls.rs` into domain modules (`query`, `structure`, `context`, `index`, `health`, `status`, `shared`) while preserving public entrypoint behavior.
- [x] 3.2 Move shared helper logic (metadata build, freshness enforcement, serialization filters, dedup/limit helpers) into stable internal shared modules and remove duplicate logic.
- [x] 3.3 Preserve and verify 003 tool semantic stability (`get_symbol_hierarchy`, `find_related_symbols`, `get_code_context`) through focused regression tests.
- [x] 3.4 Continue decomposition from the existing `query_tools` split and complete migration of `health/index/status/structure/context` handlers out of the monolithic dispatcher file.
- [x] 3.5 Introduce typed response payload structs (starting with `health_check`/`index_status`) and replace high-risk hand-written `json!` assembly incrementally.

## 4. Unified Index Process Launcher (001/004)

- [x] 4.1 Introduce shared index launcher abstraction in `crates/cruxe-mcp/src/server.rs` (or dedicated module) and migrate `index_repo`/`sync_repo` subprocess start to use it.
- [x] 4.2 Migrate `bootstrap_and_index` in `crates/cruxe-mcp/src/server.rs` to the same launcher implementation.
- [x] 4.3 Ensure all launcher call sites use deterministic binary resolution order and required env propagation with consistent error mapping.
- [x] 4.4 Add integration tests for both explicit index and auto-bootstrap paths to confirm launcher parity.

## 5. Canonical Protocol Error Mapping (002)

- [x] 5.1 Add centralized typed protocol error code mapping in `crates/cruxe-core/src/error.rs` (or equivalent shared location) aligned with `specs/meta/protocol-error-codes.md`.
- [x] 5.2 Replace ad-hoc string error codes in `crates/cruxe-mcp/src/server.rs`, `crates/cruxe-mcp/src/server/tool_calls.rs`, and `crates/cruxe-mcp/src/http.rs` with the centralized mapping.
- [x] 5.3 Add test coverage to guarantee stdio and HTTP return canonical, transport-consistent `error.code` for equivalent failure classes.

## 6. Config Normalization and Legacy Compatibility (002)

- [x] 6.1 Add typed normalization helpers for explainability/freshness config in `crates/cruxe-core/src/config.rs` and keep canonical runtime values.
- [x] 6.2 Ensure legacy `debug.ranking_reasons` compatibility maps deterministically to canonical `ranking_explain_level` behavior.
- [x] 6.3 Add/expand config-loading tests in `crates/cruxe-core/src/config.rs` for invalid values fallback and legacy compatibility behavior.

## 7. Performance Verification Layer Split (004)

- [x] 7.1 Define benchmark harness entrypoints and fixture policy for p95 verification in repository benchmark tooling (new benchmark module/path), referencing `specs/meta/benchmark-targets.md`.
- [x] 7.2 Move environment-sensitive p95 assertions out of flaky integration assertions while retaining deterministic smoke guards in `crates/cruxe-mcp/src/server/tests.rs`, `crates/cruxe-mcp/src/http.rs`, and `crates/cruxe-mcp/src/workspace_router.rs`.
- [x] 7.3 Document benchmark invocation and acceptance thresholds in `specs/meta/testing-strategy.md` and/or developer docs.

## 8. Repository Governance Automation (new capability)

- [x] 8.1 Add CI workflow in `.github/workflows/ci.yml` with required checks for `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace`.
- [x] 8.2 Add security workflow in `.github/workflows/security.yml` with baseline secret detection and dependency/security scanning.
- [x] 8.3 Add PR title policy workflow in `.github/workflows/pr-title.yml` for repository title rules.
- [x] 8.4 Add OpenSpec trace gate workflow/script (`.github/workflows/openspec-trace-gate.yml`, `.github/scripts/check_openspec_trace_gate.sh`) consistent with repository policy.
- [x] 8.5 Add `.pre-commit-config.yaml` baseline hooks aligned with `specs/meta/repo-maintenance.md` and document local usage.

## 9. Spec/Doc Synchronization and Final Validation

- [x] 9.1 Update impacted design/task documents in `specs/001-core-mvp/`, `specs/002-agent-protocol/`, `specs/003-structure-nav/`, `specs/004-workspace-transport/`, `specs/005-vcs-core/`, `specs/006-vcs-ga-tooling/`, `specs/007-call-graph/`, `specs/008-semantic-hybrid/`, `specs/009-distribution/`, and `specs/meta/` to match final implemented behavior.
- [x] 9.2 Run full validation (`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`) and record results in change notes.
- [x] 9.3 Run OpenSpec validation for this change and resolve any schema/artifact issues before implementation apply.

## 10. Runtime Connection Lifecycle Management (P1)

- [x] 10.1 Introduce a lightweight runtime SQLite `ConnectionManager` (or equivalent) in `crates/cruxe-mcp/src/server.rs` and/or `crates/cruxe-mcp/src/http.rs` for lazy open, reuse, and reconnect behavior.
- [x] 10.2 Refactor stdio and HTTP request paths to consume the same connection lifecycle abstraction instead of repeated per-request open calls.
- [x] 10.3 Add regression tests for connection failure/reopen semantics and ensure protocol error behavior remains stable.

## 11. VCS Core Readiness Foundations (005 prework)

- [x] 11.1 Create `VcsAdapter` trait boundary skeleton (and minimal implementation wiring) in a dedicated VCS-facing module/crate boundary to satisfy FR-410 direction without implementing full 005 behavior.
- [x] 11.2 Define and adopt a unified overlay merge key domain type in shared types (eliminating ad-hoc tuple/string concatenation across merge paths).
- [x] 11.3 Add unit tests that lock merge key equality and ordering semantics for base/overlay merge scenarios.

## 12. Graph Query Readiness Foundations (006/007 prework)

- [x] 12.1 Add/verify SQLite index strategy for `symbol_edges` hot queries in `crates/cruxe-state/src/schema.rs` and related query modules (`from/to + edge_type` combinations).
- [x] 12.2 Add query-shape regression tests in `crates/cruxe-state/src/edges.rs` for forward and reverse lookups to validate index-backed access patterns.
- [x] 12.3 Document `symbol_edges` query/index expectations in corresponding spec docs for 006/007 readiness.

## 13. Semantic Config Typed Substructure Foundations (008 prework)

- [x] 13.1 Extend `crates/cruxe-core/src/config.rs` with typed config substructures for semantic feature gates (`semantic_mode`, profile selection, profile-specific overrides) while preserving legacy compatibility.
- [x] 13.2 Add normalization and compatibility tests for semantic config parsing (canonical values, invalid fallback, legacy mapping behavior).
- [x] 13.3 Update `specs/008-semantic-hybrid/*` docs to align semantic config contracts with typed runtime structure decisions.

## 14. Parallel Development Guardrails and Release Baseline (009 prework)

- [x] 14.1 Add a parallel-development guardrail section/document rooted in `specs/meta/execution-order.md` covering module owners, high-conflict paths, and approved parallel touchpoints.
- [x] 14.2 Integrate guardrail references into PR/review workflow docs so multi-stream implementation follows the same boundaries.
- [x] 14.3 Verify the repo-level automation baseline (CI/security/policy) is sufficient as minimal 009-ready release governance and document remaining gaps explicitly.

## 15. Audit Reconciliation and Observability Hardening

- [x] 15.1 Add HTTP transport regression tests for malformed JSON and unknown JSON-RPC method paths, validating canonical error envelopes.
- [x] 15.2 Add `/health` cache behavior tests for TTL hit and TTL expiry (`HEALTH_CACHE_TTL`) to lock caching semantics.
- [x] 15.3 Add workspace-router integration test(s) that cover LRU eviction side effects, including index data directory cleanup and safety-path guard behavior.
- [x] 15.4 Add structured warning logs for degraded-but-continued paths (DB/index open failure, workspace resolution rejection) and verify with targeted tests or log assertions.
- [x] 15.5 Add concurrent workspace-discovery integration test(s) to validate end-to-end `claim_bootstrap_indexing` behavior under multi-request contention.
