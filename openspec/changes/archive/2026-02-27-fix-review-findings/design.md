## Context

The review surfaced concrete implementation gaps across protocol correctness,
dead code, governance evidence, and code quality hygiene:

1. Protocol error-code mappings are wrong: `SchemaStatus::NotIndexed` emits `IndexIncompatible` instead of `IndexNotReady` (in `tool_compatibility_error`, `tool_calls.rs:320-322`), and `StateError::ProjectNotFound` falls through to `InternalError` in `map_state_error` (`shared.rs:420-424`).
2. Dead code: `cruxe-core::vcs_adapter` module has zero external callers; aggregate `Error`/`Result` type and `IndexError`/`McpError`/`QueryError` sub-types are never imported outside `error.rs`; `DefaultDiffEntry` type alias is unreferenced.
3. MCP tool schemas have gaps: `suggest_followup_queries` accepts `ref` in handler but schema omits it; `search_code.semantic_ratio`/`confidence_threshold` lack numeric bounds; `find_references.kind` has no enum constraint.
4. Overlay merge asymmetry: `merged_search` has tombstone re-provision check via `overlay_keys`; `merged_locate` does not, causing potential false suppression.
5. Duplicate `worktree_leases` indexes: `idx_worktree_leases_status_last_used` and `idx_worktree_leases_status` are identical (`status, last_used_at`).
6. Semantic phase-8 benchmark requirements are represented in tests/docs but not fully wired into the standard benchmark harness path.
7. Optional all-features verification (lancedb path) requires `protoc`, but preflight guidance is not explicit enough for reproducible local/CI verification.
8. A small set of low-risk redundancies (`needless_collect` in `tantivy_index.rs:136-140`, redundant `.clone()` in `DiffEntry::renamed` at `diff.rs:43`) introduces maintenance noise.

**Important clarification on semantic runtime fail-soft:**
The original proposal stated that semantic runtime "can panic when ONNX runtime dylibs are unavailable." Re-verification against `embedding.rs:207-254` shows that **both boundaries already have Err→fallback handling**:
- `TextEmbedding::try_new` failure: handled via `match Ok/Err` with `warn!` + fallback to `None` (lines 207-225)
- `runtime.embed` failure: handled via `.lock().ok().and_then(|r| r.embed(...).ok())` with fallback to `deterministic_embedding()` (lines 230-254)

The Rust-level error paths are covered. The only uncovered risk would be an FFI panic from the ONNX C library that bypasses Rust's `Result` (which `.ok()` does NOT catch). If FFI-level `catch_unwind` protection is desired, it should be scoped as a separate, explicitly justified decision.

Constraints:
- Must preserve external API behavior and protocol semantics (except correcting wrong error codes).
- Must remain aligned with existing Rust-first/embedded/fail-soft design constraints from `specs/008-semantic-hybrid`.
- Must keep deterministic verification commands for CI and local workflows.

Stakeholders:
- Agent developers (correct protocol error codes and complete tool schemas)
- Runtime maintainers (stability and fallback behavior documentation)
- Governance/CI maintainers (benchmark evidence and reproducibility)
- Contributors/operators who rely on deterministic validation commands

## External Review Findings Triage (2026-02-27)

An external full-repo review list was re-validated against current code/spec
state. We classify each item as:
- **Confirmed**: directly reproducible in current repo state
- **Partial**: claim direction is valid, but severity/scope needs nuance
- **Not confirmed**: evidence does not support the claim as a defect

### Critical/High Findings

| ID | Finding | Triage | Notes |
|---|---|---|---|
| C1 | `cruxe-core::vcs_adapter` dead module | Confirmed | `cruxe-core/src/vcs_adapter.rs` has no workspace callers beyond self-tests. `pub trait VcsAdapter`, `pub struct GitVcsAdapter`, `pub fn default_vcs_adapter()` — all zero external imports. Superseded by `cruxe-vcs` crate (spec 005/006). |
| C2 | Aggregate `Error`/`Result` and specific sub-errors unused | Confirmed | Dead types: aggregate `Error` enum (lines 4-28), `Result<T>` alias (line 342), `IndexError` (line 274), `McpError` (line 319), `QueryError` (line 307). **Live types to preserve**: `StateError`, `ConfigError`, `ParseError`, `VcsError`, `WorkspaceError`, `ProtocolErrorCode` — all actively imported across multiple crates. |
| C3 | `SchemaStatus::NotIndexed` mapped to `IndexIncompatible` instead of `IndexNotReady` | Confirmed | `tool_compatibility_error` in `tool_calls.rs:320-322` emits `ProtocolErrorCode::IndexIncompatible` for all non-`ProjectNotFound` cases including `NotIndexed`. Per spec, `IndexNotReady` is the correct code for "no index available." |
| C4 | Spec 009 missing explicit crates.io FR | Partial | Constitution requires `cargo install` path; spec 001 US1 presupposes it. Spec 009 (distribution) has no explicit FR for crates.io publishing. Resolution: add cross-reference note in spec 009, not a new FR (since spec 001 already establishes the requirement). |
| H1 | Canonical DDL docs stale vs runtime schema | Confirmed | `specs/meta/design.md` and `specs/001-core-mvp/data-model.md` lag current `schema.rs`/vector schema fields. 10+ columns/indexes diverge. |
| H2 | `semantic_vectors` uses `project_id` while most tables use `repo` | Confirmed | `vector_index.rs` uses `project_id`; all other tables use `repo`. Cross-table identity naming is inconsistent. |
| H3 | Duplicate `worktree_leases` indexes | Confirmed | `idx_worktree_leases_status` (`status, last_used_at`) and `idx_worktree_leases_status_last_used` (`status, last_used_at`) are **identical**. `idx_worktree_leases_status_updated` (`status, updated_at`) is distinct and should be kept. Remove `idx_worktree_leases_status_last_used` (migration artifact from V7). |
| H4 | `suggest_followup_queries` accepts `ref` in handler but schema omits it | Confirmed | Tool schema in `tools/suggest_followup_queries.rs` lacks `ref`; handler reads `arguments.get("ref")`. |
| H5 | Spec 005 `branch_tombstones` heading says "Unchanged" while adding new fields | Confirmed | Heading/content contradiction in `specs/005-vcs-core/data-model.md:113`. Heading says "Unchanged from 001-core-mvp" but body adds `tombstone_type` and `created_at`. |
| H6 | Testing strategy references unscheduled backlog topics | Confirmed | `watch daemon lifecycle` (line 48) / `structural path boost` (line 25) rows in `meta/testing-strategy.md` have no corresponding FR in owning specs and are marked unscheduled in roadmap backlog. |

### Medium/Low Findings

| ID | Finding | Triage | Notes |
|---|---|---|---|
| M1 | `cleanup_stale_staging()` dead public API | Confirmed | Only referenced in its own test module. Should be `pub(crate)` not `pub`. |
| M2 | `DefaultDiffEntry` alias unused | Confirmed | `adapter.rs:38` — zero references outside declaration. |
| M3 | `serde_json` in CLI runtime deps but used only in tests | Confirmed | `serde_json` in `cruxe-cli/Cargo.toml:24` `[dependencies]` but no `use serde_json` in any `src/` file. Only in `tests/integration_test.rs`. Note: `blake3` is correctly in `[dependencies]` (used in `commands/index.rs`). |
| M4 | Query tuning magic numbers not config-exposed | Confirmed | Hardcoded weights/boosts/fanouts in `confidence.rs:27` (0.55/0.30/0.15), `rerank.rs:80,84` (0.75/2.5), `search.rs:267,286-287` (2x/4x/3x fanout multipliers). |
| M5 | `search_code` schema missing numeric bounds for ratio/threshold | Confirmed | `semantic_ratio` and `confidence_threshold` are `"type": "number"` with no `minimum`/`maximum`. Spec says 0.0-1.0 and handler validates at runtime. |
| M6 | `find_references.kind` missing enum constraint | Confirmed | `kind` parameter is `"type": "string"` with no enum despite having exactly 5 valid values. |
| M7 | `health_check` default-workspace semantics inconsistent across specs | Confirmed | Spec 002 says "all registered projects"; spec 004/impl use default workspace. |
| M8 | Overlay merge asymmetry (search vs locate tombstone re-provision) | Confirmed | `merged_search` (lines 87-95) computes `overlay_keys` and preserves base results at tombstoned paths if overlay re-provides the key. `merged_locate` (lines 122-125) has no such check — always suppresses base at tombstoned paths. |
| M9 | `StateError::ProjectNotFound` not mapped in `map_state_error` | Confirmed | Falls through to catch-all `InternalError` at `shared.rs:420-424`, even though `ProtocolErrorCode::ProjectNotFound` exists and is used at direct emission sites. |
| M10 | Stale phase reference in 001 MCP contract | Confirmed | `specs/001-core-mvp/contracts/mcp-tools.md:52` says "planned for Phase 4" for multi-workspace routing. Phase 4 is now spec 009 (distribution). Multi-workspace is Phase 1.5b/spec 004 (already implemented). |
| L1 | `cruxe-core::vcs` free functions duplicate VCS crate capabilities | Partial | Active callers exist in 4 crates (`cli`, `query`, `mcp`, `indexer`). Migration debt, not dead code. Free functions are thin (3-5 lines), no bugs. Recommend: document keep rationale, defer migration. |
| L2 | FR numbering offset inconsistency | Not confirmed | Numbering strategy documented in `specs/meta/INDEX.md:94-96`. Intentional. |
| L3 | Alpha-suffixed FRs undocumented | Partial | Suffix usage exists (`FR-105a/b/c`, `FR-115a`); no explicit convention section. Low-risk doc hygiene. |
| L4 | `semantic_fallback` metadata documentation drift | Partial | Mentioned in spec edge cases; not consistently listed in contract metadata table. |
| L5 | Spec 008 dependency on 007 appears weak | Partial | Sequential-only dependency. Could confuse reviewers. |
| L6 | Spec 009 agent coverage mismatch (Copilot guide vs template) | Not confirmed | FR-806 and FR-808 deliberately separate templates from guides. Copilot is guides-only because MCP support is conditional. Intentional. |

### Scope decision for this change

This change adopts a **single-spec consolidated lane**:

- protocol/schema correctness and dead-code removal as phase-1 priority (highest blast radius reduction)
- redundancy cleanup, overlay merge fix, and schema hygiene as phase-2
- benchmark/governance wiring and documentation as phase-3
- spec/doc consistency updates as phase-4
- avoid creating a separate follow-up change for confirmed items unless new scope is discovered during implementation

This preserves one governed context while keeping execution ordered by risk and
blast radius.

## Goals / Non-Goals

**Goals:**
- Fix protocol error-code mappings so agents receive semantically correct error codes.
- Remove confirmed dead code and unused types to reduce maintenance surface.
- Complete MCP tool JSON schemas so agents get correct parameter introspection.
- Fix overlay merge asymmetry for consistent tombstone behavior.
- Keep semantic benchmark gates executable and auditable via the existing benchmark harness workflow.
- Make optional all-features verification reproducible by documenting required preflight dependencies.
- Remove review-confirmed low-risk redundancies without altering business behavior.
- Formalize existing semantic runtime fail-soft behavior as spec scenarios (documenting, not re-implementing).

**Non-Goals:**
- No new semantic retrieval algorithm changes.
- No protocol schema redesign.
- No FFI-level `catch_unwind` for ONNX runtime (unless explicitly scoped as a follow-up decision).
- No broad refactor of module boundaries or visibility unless required by behavior/correctness.
- No migration of `cruxe-core::vcs` free functions (keep rationale documented, defer to follow-up).

## Decisions

### 1) Semantic runtime fail-soft: document existing, do not re-implement
- Decision: the spec delta documents existing fail-soft boundaries in `embedding.rs` as formal scenarios. No code changes needed for `TextEmbedding::try_new` or `runtime.embed` — both already handle `Err` with deterministic fallback. If FFI-level `catch_unwind` for ONNX C-library panics is desired, scope it as a separate decision with explicit risk/benefit analysis.
- Rationale: re-verification confirmed both init and embed paths have `match Err` / `.ok().and_then()` guardrails. Adding redundant error handling would add code with no behavioral change.
- Evidence: `embedding.rs:207-225` (`try_new` match), `embedding.rs:230-254` (`.ok().and_then()` + `deterministic_embedding()` fallback).

### 2) Keep benchmark governance in one canonical harness lane
- Decision: extend `scripts/benchmarks/run_mcp_benchmarks.sh` to include semantic phase-8 benchmark entrypoints, with controlled env defaults to keep runtime deterministic.
- Rationale: existing governance docs already designate this script as the benchmark harness entrypoint; semantic gates should not live off-path.
- Alternatives considered:
  - Separate semantic-only script: rejected (fragmented governance/evidence path).
  - CI-only hidden invocation: rejected (local reproducibility suffers).

### 3) Explicit optional-feature preflight guidance
- Decision: document `protoc` preflight for all-features/lancedb verification and keep base-path CI unchanged.
- Rationale: avoids false negatives and improves operator reproducibility without forcing protoc into default lane.
- Alternatives considered:
  - Make protoc mandatory everywhere: rejected (increases baseline friction for non-lancedb path).

### 4) Targeted redundancy cleanup with strict scope lock
- Decision: apply only review-confirmed local cleanups where behavior is provably unchanged and covered by tests/clippy.
- Confirmed cleanup targets:
  - `tantivy_index.rs:136-140`: `needless_collect` — collected `Vec` is only checked with `.is_empty()`, never iterated or joined. Safe to replace with `.any()`.
  - `diff.rs:43`: `new_path.clone()` — `new_path` is moved into `Self { path: new_path }` but `.clone()` is called before the move for the `Renamed` variant. Reorder to use `new_path` directly in `path` field.
- Alternatives considered:
  - Wide "style sweep": rejected (high churn, low value).
  - Visibility tightening in command modules: deferred.

### 5) Dead-code cleanup must be evidence-backed
- Decision: require an explicit dead-code pass for touched modules and allow removals only when references are provably absent (compiler/lint/symbol search evidence).
- Confirmed dead code to remove:
  - `crates/cruxe-core/src/vcs_adapter.rs` — entire file + `pub mod vcs_adapter;` from `lib.rs:10`. Zero external callers confirmed by workspace-wide grep.
  - `crates/cruxe-core/src/error.rs` — aggregate `Error` enum (lines 4-28), `Result<T>` alias (line 342), `IndexError` (lines 274-292), `McpError` (lines 319-331), `QueryError` (lines 307-316). Zero external imports confirmed. **Preserve**: `StateError`, `ConfigError`, `ParseError`, `VcsError`, `WorkspaceError`, `ProtocolErrorCode` (all actively used).
  - `crates/cruxe-vcs/src/adapter.rs:38` — `pub type DefaultDiffEntry = DiffEntry;`. Zero references.
- Rationale: satisfies review expectations while preventing accidental deletion of live extension points.

### 6) Consolidate validated external findings into one change
- Decision: implement validated external-review findings in this same change via phased task groups, instead of opening a separate follow-up change.
- Rationale: user requested one-spec governance lane; this keeps traceability, validation evidence, and rollout communication in one place.

### 7) VCS free-function migration debt: document and defer
- Decision: keep `cruxe-core::vcs` free functions (`detect_head_branch`, `is_git_repo`, `detect_head_commit`) as-is. Document keep rationale; do not consolidate into `cruxe-vcs` adapter threading.
- Rationale: free functions are thin (3-5 lines each), have no bugs, and 4 crates actively call them. Cost of threading a `Git2VcsAdapter` instance through all callers outweighs dedup benefit. Follow-up trigger: if `cruxe-vcs` adds functionality that free functions cannot provide (e.g., caching, pooling).
- Alternatives considered:
  - Full migration to `cruxe-vcs` adapter: rejected (high churn, zero correctness gain).

### 8) `semantic_vectors.project_id` naming: values align with `repo`, migration deferred
- Decision: runtime usage currently passes the same logical project identity through both `repo` (other tables) and `project_id` (semantic vectors), but we defer column rename in this change and record a V13 migration plan.
- Rationale: call sites constructing `VectorRecord` and semantic query scopes derive `project_id` from the same project identifier that populates `repo` elsewhere (`embed_writer`, `search`, `vcs_e2e` fixtures). Renaming now would be schema-wide churn without correctness gain.
- Follow-up: when schema churn is otherwise required, run V13 `ALTER TABLE ... RENAME COLUMN` migration for `semantic_vectors`/`semantic_vector_meta` and align naming end-to-end.

## Risks / Trade-offs

- [Risk] Dead-code cleanup can remove intentional extension points.
  → Mitigation: the dead type list above is exhaustively verified by workspace-wide grep. Record "no confirmed dead code" when applicable.

- [Risk] Error-code change (`IndexIncompatible` → `IndexNotReady` for `NotIndexed` state) is a protocol-visible change.
  → Mitigation: this is a bugfix (code was wrong per spec). Agents that handle `index_incompatible` for "not indexed" will need to update, but the correct behavior is semantically clearer.

- [Risk] Adding semantic benchmark invocations increases benchmark runtime cost.
  → Mitigation: keep heavy checks in explicit benchmark harness lane (not default fast PR unit path), preserve smoke-vs-benchmark split.

- [Risk] Duplicate index removal requires careful migration.
  → Mitigation: V12 migration with `DROP INDEX IF EXISTS idx_worktree_leases_status_last_used`. Idempotent; the remaining `idx_worktree_leases_status` covers the same columns.

## Migration Plan

1. Remove confirmed dead code (`vcs_adapter.rs`, dead error types, `DefaultDiffEntry`) and run compile/test to confirm no breakage.
2. Fix protocol error-code mappings (`IndexNotReady` for `NotIndexed`, `ProjectNotFound` in `map_state_error`) and update MCP tool schemas (add `ref`, enum constraints, numeric bounds).
3. Fix overlay merge asymmetry in `merged_locate`.
4. Apply review-confirmed redundancy cleanups (`tantivy_index.rs`, `diff.rs`) and run clippy/tests.
5. Implement maintainability hygiene: `cleanup_stale_staging` → `pub(crate)`, `serde_json` → `[dev-dependencies]`.
6. Add V12 migration to remove duplicate `worktree_leases` index.
7. Extend benchmark harness script to include semantic phase-8 benchmark entrypoints.
8. Update docs with all-features preflight (`protoc`) guidance.
9. Implement spec/doc consistency fixes (DDL refresh, heading correction, phase reference cleanup, testing-strategy backlog split).
10. Externalize hardcoded query-tuning constants to config defaults.
11. Verify with deterministic command set:
    - `cargo fmt --all --check`
    - `cargo clippy --workspace -- -D warnings`
    - `CRUXE_ENABLE_FASTEMBED_RUNTIME=0 cargo test --workspace`
    - benchmark harness script run (including semantic entries)

Rollback strategy:
- All changes are additive/isolated and can be reverted per-file if regressions surface.
- Error-code fix is a one-line enum variant swap; fully reversible.
- Schema migration V12 is a `DROP INDEX IF EXISTS`; no data loss possible.

## Open Questions

- Should semantic phase-8 benchmark assertions remain `#[ignore]` and be harness-only, or should a reduced subset run in scheduled CI automatically?
- Should we add a dedicated CI job for `--all-features` with protoc preinstalled, or keep it as documented/manual verification only?
