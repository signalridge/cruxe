## Why

Recent spec-vs-code review found protocol correctness, dead code, and governance gaps:

- confirmed dead code modules (`vcs_adapter`) and unused error types (`Error` aggregate, `IndexError`, `McpError`, `QueryError`) carry maintenance cost and mislead contributors
- protocol error-code mappings emit wrong codes for `NotIndexed`/`ProjectNotFound` states, preventing agents from distinguishing failure modes
- MCP tool JSON schemas are missing declared parameters (`suggest_followup_queries.ref`), enum constraints (`find_references.kind`), and numeric bounds (`semantic_ratio`, `confidence_threshold`)
- stale spec DDL docs, duplicate database indexes (`worktree_leases`), and cross-spec heading contradictions reduce traceability
- overlay merge has asymmetric tombstone re-provision logic between search and locate paths
- semantic benchmark evidence exists, but canonical harness wiring is incomplete; optional all-features (`lancedb`) verification is not reproducible without explicit `protoc` preflight guidance
- a small set of review-confirmed redundancy items (`needless_collect`, redundant clone) should be cleaned up with evidence-backed scope

Note: semantic runtime fail-soft boundaries (`TextEmbedding::try_new` Err→fallback, `runtime.embed` via `.ok().and_then()`) are **already implemented** in `embedding.rs:207-254`. This change documents those existing safeguards in the spec delta rather than re-implementing them. If FFI-level `catch_unwind` for ONNX C-library panics is desired, that would be a separate scope decision.

## What Changes

- Resolve confirmed protocol/schema/tooling drifts: fix error-code mapping (`IndexNotReady` for `SchemaStatus::NotIndexed`, `ProjectNotFound` in `map_state_error`), add missing MCP tool schema parameters/constraints, remove duplicate database indexes, and update stale spec/DDL docs.
- Remove confirmed dead code: `cruxe-core::vcs_adapter` module (zero external callers), unused aggregate `Error`/`Result` type and dead sub-error types (`IndexError`, `McpError`, `QueryError`), `DefaultDiffEntry` type alias.
- Remove review-confirmed low-risk redundancies (`needless_collect` in `tantivy_index.rs`, redundant `.clone()` in `DiffEntry::renamed`) and perform evidence-backed dead-code pass for touched modules.
- Fix overlay merge asymmetry: add tombstone re-provision check to `merged_locate` consistent with `merged_search`.
- Extend benchmark/governance wiring so semantic phase-8 benchmark signals run through the standard benchmark harness path.
- Document optional all-features (`lancedb`) preflight requirements (notably `protoc`) and deterministic verification commands.
- Consolidate validated cross-spec/code findings into phased implementation tasks (single-spec execution lane).
- Externalize review-confirmed hardcoded query-tuning constants into config with backward-compatible defaults.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `semantic-config-readiness`: formalize existing runtime fail-soft behavior as spec scenarios; document that `embedding.rs` already handles `try_new` failure and `embed` invocation failure with deterministic fallback.
- `repo-governance-automation`: expand governance to include executable semantic benchmark harness coverage, explicit optional-feature preflight guidance, and evidence-backed maintainability cleanup expectations.

## Impact

- Affected code:
  - `crates/cruxe-core/src/error.rs` (remove dead types), `crates/cruxe-core/src/vcs_adapter.rs` (delete), `crates/cruxe-core/src/lib.rs` (remove module decl)
  - `crates/cruxe-state/src/tantivy_index.rs` (needless_collect), `crates/cruxe-state/src/schema.rs` (V12 migration for duplicate index)
  - `crates/cruxe-vcs/src/diff.rs` (redundant clone), `crates/cruxe-vcs/src/adapter.rs` (remove `DefaultDiffEntry`)
  - `crates/cruxe-mcp/src/server/tool_calls.rs` (error-code fix), `crates/cruxe-mcp/src/server/tool_calls/shared.rs` (`ProjectNotFound` mapping)
  - MCP tool schemas in `crates/cruxe-mcp/src/tools/{suggest_followup_queries,search_code,find_references}.rs`
  - `crates/cruxe-query/src/overlay_merge.rs` (tombstone re-provision parity)
  - `crates/cruxe-indexer/src/staging.rs` (visibility reduction)
  - `crates/cruxe-cli/Cargo.toml` (`serde_json` to dev-deps)
  - `scripts/benchmarks/*` and CI workflow files
- Affected docs/spec artifacts:
  - OpenSpec delta files for modified capabilities
  - benchmark and optional-feature preflight documentation
  - spec consistency updates across `specs/meta/*`, `specs/001-core-mvp/*`, `specs/005-vcs-core/*`, `specs/008-semantic-hybrid/*`, `specs/009-distribution/*`
- Runtime/API impact:
  - protocol error codes change for `NotIndexed` state (from `index_incompatible` to `index_not_ready`) — agents will see correct error codes
  - no other breaking API changes
