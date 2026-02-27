## 1. Core Metadata Contract Alignment (001 + 003)

- [x] 1.1 Update `IndexingStatus` and `ResultCompleteness` in `crates/cruxe-core/src/types.rs` to canonical enum sets required by specs 001-003.
- [x] 1.2 Add legacy alias compatibility for `indexing_status` deserialization (`idle` -> `ready`, `partial_available` -> `ready`) and corresponding unit tests. `not_indexed` and `failed` are new variants with no legacy alias.
- [x] 1.3 Align metadata builder methods in `crates/cruxe-mcp/src/protocol.rs` (`new`, `not_indexed`, `syncing`, `reindex_required`, `corrupt_manifest`) to emit semantically correct canonical `indexing_status` values per the builder mapping table in design.md Decision 2.
- [x] 1.4 Ensure `get_code_context`, `get_symbol_hierarchy`, and `find_related_symbols` emit canonical metadata and set `result_completeness: "truncated"` when truncation occurs.

## 2. Explainability and Config Migration (002)

- [x] 2.1 Add `search.ranking_explain_level` (`off|basic|full`) to `crates/cruxe-core/src/config.rs` and `configs/default.toml`, with normalization logic and environment-variable overrides.
- [x] 2.2 Implement compatibility precedence logic: request arg > config default > legacy `debug.ranking_reasons` fallback.
- [x] 2.3 Extend MCP tool schemas in `crates/cruxe-mcp/src/tools/search_code.rs` and `crates/cruxe-mcp/src/tools/locate_symbol.rs` to accept `compact` and `ranking_explain_level`.
- [x] 2.4 Implement `basic` vs `full` ranking reason serialization paths in query/runtime assembly and keep `off` zero-payload behavior.
- [x] 2.5 Migrate `cruxe-cli` `serve-mcp` entrypoint: remove legacy `enable_ranking_reasons: bool` runtime toggle path and align to `ranking_explain_level` config/env propagation.

## 3. Query Payload Optimization Guarantees (FR-105b/FR-105c)

- [x] 3.1 Implement near-duplicate suppression in `search_code`/`locate_symbol` response assembly with deterministic identity keys and include `suppressed_duplicate_count` metadata.
- [x] 3.2 Implement hard payload safety limit enforcement for query tools, including deterministic truncation behavior and `safety_limit_applied` metadata.
- [x] 3.3 Ensure truncation path sets `result_completeness: "truncated"` and always returns deterministic `suggested_next_actions` instead of hard failures.
- [x] 3.4 Keep `compact` behavior serialization-only (after retrieval/ranking), preserving ordering and stable identifiers while removing heavy optional fields.

## 4. Verification and Regression Coverage

- [x] 4.1 Add/update unit tests for enum serde, legacy alias compatibility, and ranking explain config parsing in `cruxe-core`.
- [x] 4.2 Add/update MCP integration tests for `ranking_explain_level` (`off/basic/full`), `compact` payload shaping, dedup metadata, and truncation metadata.
- [x] 4.3 Add/update structure-nav tool tests to verify canonical metadata + non-`compact` schema boundary for 003 tools.
- [x] 4.4 Run `cargo fmt --check`, `cargo clippy --workspace`, and `cargo test --workspace`; fix regressions and record outcomes in the change notes.
