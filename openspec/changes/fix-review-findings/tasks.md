## 1. Dead code removal and protocol correctness (phase 1)

- [x] 1.1 Delete `crates/codecompass-core/src/vcs_adapter.rs` and remove `pub mod vcs_adapter;` from `crates/codecompass-core/src/lib.rs:10`. Evidence: zero external callers confirmed by workspace-wide grep.
- [x] 1.2 Remove dead error types from `crates/codecompass-core/src/error.rs`: aggregate `Error` enum (lines 4-28), `Result<T>` alias (line 342), `IndexError` (lines 274-292), `McpError` (lines 319-331), `QueryError` (lines 307-316). **Preserve**: `StateError`, `ConfigError`, `ParseError`, `VcsError`, `WorkspaceError`, `ProtocolErrorCode` (all actively imported across multiple crates).
- [x] 1.3 Remove `pub type DefaultDiffEntry = DiffEntry;` from `crates/codecompass-vcs/src/adapter.rs:38`. Evidence: zero references outside declaration.
- [x] 1.4 Fix `tool_compatibility_error` in `crates/codecompass-mcp/src/server/tool_calls.rs:320-322`: emit `ProtocolErrorCode::IndexNotReady` for `SchemaStatus::NotIndexed` (currently emits `IndexIncompatible`). Keep `IndexIncompatible` for `ReindexRequired`/`CorruptManifest`.
- [x] 1.5 Add explicit `StateError::ProjectNotFound` arm in `map_state_error` at `crates/codecompass-mcp/src/server/tool_calls/shared.rs:420` mapping to `ProtocolErrorCode::ProjectNotFound` instead of falling through to `InternalError`.
- [x] 1.6 Run `cargo check --workspace && cargo test --workspace` to confirm no breakage from dead code removal and error-code fixes.

## 2. MCP tool schema fixes (phase 1)

- [x] 2.1 Add `ref` parameter to `suggest_followup_queries` tool schema in `crates/codecompass-mcp/src/tools/suggest_followup_queries.rs`. Handler already reads `arguments.get("ref")`.
- [x] 2.2 Add `"minimum": 0.0, "maximum": 1.0` to `semantic_ratio` and `confidence_threshold` in `crates/codecompass-mcp/src/tools/search_code.rs:51-57`.
- [x] 2.3 Add `"enum": ["imports", "calls", "implements", "extends", "references"]` to `kind` parameter in `crates/codecompass-mcp/src/tools/find_references.rs:23-26`.

## 3. Redundancy cleanup and overlay merge fix (phase 2)

- [x] 3.1 Replace `needless_collect` in `crates/codecompass-state/src/tantivy_index.rs:136-140`: change `let missing_fields: Vec<&str> = ...collect(); if missing_fields.is_empty()` to `if !required_fields.iter().any(|name| schema.get_field(name).is_err())`. The collected Vec is only checked with `.is_empty()` — never iterated, joined, or used in the error message.
- [x] 3.2 Remove redundant `.clone()` in `crates/codecompass-vcs/src/diff.rs:43` (`DiffEntry::renamed`): `new_path` is an owned `String`; the `.clone()` before moving into `Self { path: new_path }` is unnecessary. Reorder to use `new_path` directly in the `path` field and pass the original to `Renamed { old_path }`.
- [x] 3.3 Fix overlay merge asymmetry in `crates/codecompass-query/src/overlay_merge.rs`: add `overlay_keys` pre-computation and tombstone re-provision check to `merged_locate` (lines 121-127) consistent with `merged_search` (lines 87-95). Add test case to verify re-provision behavior.
- [x] 3.4 Perform evidence-backed dead-code pass for touched modules (`tantivy_index.rs`, `diff.rs`, `overlay_merge.rs`); record no-op evidence when nothing additional is safe to delete.

## 4. Maintainability hygiene (phase 2)

- [x] 4.1 Narrow `cleanup_stale_staging()` to test-only scope in `crates/codecompass-indexer/src/staging.rs:114` (`#[cfg(test)] fn`, stricter than `pub(crate)`). Only referenced in its own test module; no external callers.
- [x] 4.2 Move `serde_json` from `[dependencies]` to `[dev-dependencies]` in `crates/codecompass-cli/Cargo.toml:24`. Confirmed: no `use serde_json` in any `src/` file; only used in `tests/integration_test.rs`.
- [x] 4.3 Add V12 schema migration to remove duplicate `worktree_leases` index in `crates/codecompass-state/src/schema.rs`: `DROP INDEX IF EXISTS idx_worktree_leases_status_last_used`. Keep `idx_worktree_leases_status` (spec-aligned name) and `idx_worktree_leases_status_updated` (distinct columns). Bump `CURRENT_SCHEMA_VERSION` to 12.

## 5. Benchmark/governance harness alignment (phase 3)

- [x] 5.1 Extend `scripts/benchmarks/run_mcp_benchmarks.sh` to execute semantic phase benchmark entrypoints (directly or via `benchmarks/semantic/run_semantic_benchmarks.sh`) used by semantic acceptance requirements.
- [x] 5.2 Keep smoke-vs-benchmark layering intact by ensuring default fast test path remains unchanged while benchmark harness executes runtime-sensitive checks.
- [x] 5.3 Validate benchmark harness script output and command coverage against current governance/testing docs.

## 6. Optional-feature preflight reproducibility (phase 3)

- [x] 6.1 Add explicit `protoc` preflight guidance for optional all-features/lancedb verification paths in repository docs.
- [x] 6.2 Document deterministic verification command set for optional feature lane (including all-features clippy/test expectations).

## 7. Spec and documentation consistency (phase 4)

- [x] 7.1 Fix spec 005 `branch_tombstones` heading: change "Unchanged from 001-core-mvp" to "Extended from 001-core-mvp" in `specs/005-vcs-core/data-model.md:113`.
- [x] 7.2 Fix stale phase reference in `specs/001-core-mvp/contracts/mcp-tools.md:52`: change "planned for Phase 4" to reference spec 004 (Phase 1.5b, already implemented).
- [x] 7.3 Move `watch daemon lifecycle` and `structural path boost` rows in `specs/meta/testing-strategy.md` to a "Future/Backlog" section (no owning FR, both unscheduled in roadmap).
- [x] 7.4 Refresh `specs/meta/design.md` §9.0.3 canonical DDL to match current production schema (V11): update `symbol_edges` (nullable `to_symbol_id`, `to_name`, `source_file`, `source_line`, unique index), `index_jobs` (progress_token, files_scanned/indexed/symbols_extracted), `symbol_relations.content`. Add `semantic_vectors` and `semantic_vector_meta` table DDL.
- [x] 7.5 Add crates.io distribution cross-reference note in spec 009 (reference spec 001 US1 `cargo install` requirement).
- [x] 7.6 Align `semantic_fallback` metadata-table consistency in spec 008 contracts.
- [x] 7.7 Add `FR` alpha-suffix convention note to `specs/meta/INDEX.md` (optional, low priority).
- [x] 7.8 Add brief rationale note to spec 008 `Depends On: 007-call-graph` clarifying this is a sequential-only dependency with no functional FR dependency (optional, low priority).
- [x] 7.9 Align `health_check` default-workspace semantics across spec and implementation: reconcile `specs/002-agent-protocol/contracts/mcp-tools.md` ("all registered projects") with `specs/004-workspace-transport/contracts/mcp-tools.md` + tool schema/handler ("server default project"), and update the chosen source of truth.

## 8. Query-tuning config externalization (phase 4)

- [x] 8.1 Externalize confirmed hardcoded query-tuning constants to `SearchConfig` fields with current values as backward-compatible defaults: confidence weights (`confidence.rs:27`: 0.55/0.30/0.15), rerank boosts (`rerank.rs:80,84`: 0.75/2.5), fanout multipliers (`search.rs:267,286-287`: 2x/4x/3x).
- [x] 8.2 Add tests confirming default config values produce identical behavior to current hardcoded values.

## 9. Semantic runtime spec formalization (phase 4)

- [x] 9.1 Verify existing fail-soft boundaries in `embedding.rs:207-254` match the spec delta scenarios (init failure → `warn!` + `None` cache; embed failure → `.ok().and_then()` + `deterministic_embedding()`). Record evidence: if behavior matches scenarios, mark as verified with evidence pointer. If gap found, create follow-up task.
- [x] 9.2 Decide whether FFI-level `catch_unwind` for ONNX C-library panics is warranted. If yes, create scoped follow-up task with explicit risk/benefit. If no, record rationale.

## 10. VCS migration debt disposition (phase 4)

- [x] 10.1 Document explicit keep rationale for `codecompass-core::vcs` free functions: thin wrappers (3-5 lines), 4 active callers, no bugs, adapter threading cost outweighs dedup benefit. Follow-up trigger: `codecompass-vcs` adds capabilities (caching, pooling) that free functions cannot provide.

## 11. Identity convention decision (deferred)

- [x] 11.1 Decide `semantic_vectors.project_id` vs `repo` naming: verify whether callers of `upsert_vector`/`query_nearest` pass the same value for `project_id` as other tables pass for `repo`. If values are identical at runtime, plan V13 migration with `ALTER TABLE ... RENAME COLUMN`. If different, document why and close as intentional.

## 12. Verification and evidence capture

- [x] 12.1 Run formatting and lint gates (`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`) after all code changes.
- [x] 12.2 Run deterministic test verification (`CODECOMPASS_ENABLE_FASTEMBED_RUNTIME=0 cargo test --workspace`) and confirm all 497+ tests pass.
- [x] 12.3 Run benchmark harness evidence command and capture semantic benchmark coverage in logs/output summary.
- [x] 12.4 Re-run `openspec validate fix-review-findings` and record command evidence in final summary.
- [x] 12.5 Record explicit disposition for not-confirmed findings (`L2`: documented in INDEX.md; `L6`: intentional template/guide scope separation) so closure is auditable.
