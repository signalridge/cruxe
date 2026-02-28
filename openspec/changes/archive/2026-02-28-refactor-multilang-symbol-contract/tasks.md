## 1. Core types (cruxe-core)

- [x] 1.1 Add `SymbolRole` enum (`Type`, `Callable`, `Value`, `Namespace`, `Alias`) with `Display`/`FromStr`/`Serialize`/`Deserialize` in `cruxe-core/src/types.rs`.
- [x] 1.2 Add `SymbolKind::role() -> SymbolRole` deterministic mapping method.
- [x] 1.3 Remove `Import` from `SymbolKind`. Map `from_str("import")` → `Module` for legacy deserialization.
- [x] 1.4 Update `RankingReasons` struct (types.rs:233):
  - Existing `kind_match: f64` field: keep name for backward compatibility, but populate with `kind_weight + query_intent_boost` instead of hardcoded `0.0`.
  - Add `test_file_penalty: f64` as a new separate field (not folded into `kind_match` — it is conceptually distinct from kind scoring).
- [x] 1.5 Unit tests: role mapping covers all kinds, round-trip serde with legacy `"import"` value, `SymbolRole` display/parse.

## 2. Generic tag mapper (cruxe-indexer)

- [x] 2.1 Create `generic_mapper.rs` module with:
  - `map_tag_kind(tag_kind, has_parent, node_kind) -> Option<SymbolKind>` — shared lookup table with `has_parent` for Function→Method promotion and `node.kind()` fallback for struct/enum/union/type disambiguation (critical: Rust `struct_item`, `union_item`, `enum_item`, `type_item` all arrive as tag kind `"class"` via `@definition.class`).
  - `find_parent_scope(node, source) -> Option<String>` — generic AST parent walking with `is_scope_node` / `is_transparent_node` match sets.
  - `strip_generic_args(name) -> String` — universal `<T>` and `[T]` stripping.
  - `separator_for_language(language) -> &'static str` — `"rust" => "::"`, `_ => "."`.
  - `extract_signature(kind, source, line_range) -> Option<String>` — first-line extraction for Function/Method only (preserves existing tag_extract.rs:89-98 behavior).
- [x] 2.2 Update `tag_extract.rs` to call `generic_mapper` instead of `LanguageEnricher` trait methods. Remove `enricher` parameter from `extract_symbols_via_tags`.
- [x] 2.3 Delete enricher files: `enricher.rs`, `enricher_rust.rs`, `enricher_typescript.rs`, `enricher_python.rs`, `enricher_go.rs`.
- [x] 2.4 Update `languages/mod.rs`: remove `LanguageEnricher` trait, remove enricher dispatch (mod.rs:67-71), simplify `extract_symbols()` to use generic mapper directly.
- [x] 2.5 Tests: generic mapper produces correct `SymbolKind` for all 4 languages' tag kinds; parent scope extracted for nested methods/functions; Go receiver type extracted via `child_by_field_name("type")`; generic args stripped from parent names; `visibility` is `None`; Function→Method promotion works for nested definitions.

## 3. Import path resolution + unresolved unification (cruxe-indexer)

- [x] 3.1 Change `resolve_imports()` to store unresolved imports as `to_symbol_id = NULL, to_name = Some(import_name)` instead of blake3 hash IDs. Remove `unresolved_symbol_stable_id()` function (import_extract.rs:53) and its call sites (import_extract.rs:125-127).
- [x] 3.2 Add `resolve_import_path(importing_file, module_spec, language) -> Option<String>` in `import_extract.rs`.
  - TypeScript: `./foo` → join with importing dir, try `.ts`, `.tsx`, `/index.ts`.
  - Rust: `super::module` → walk parent dirs, try `module.rs` / `module/mod.rs`.
  - Python: `.utils` → relative to package dir, try `utils.py` / `utils/__init__.py`.
  - Go: skip (absolute module paths).
- [x] 3.3 After path resolution, constrain symbol lookup to resolved file via `file_manifest` check + file-specific `symbol_relations` query.
- [x] 3.4 Tests: TS `./services/user` resolves; Rust `super::module::Type` resolves; Python `.utils.helper` resolves; unresolvable path stored with `to_symbol_id = NULL, to_name = import_name`.

## 4. Call edge fix (cruxe-indexer)

- [x] 4.1 In `call_extract.rs`, when `resolve_caller_symbol` returns `None` (line 24), use `source_symbol_id_for_path(path)` as caller ID instead of `continue`. This captures module-scoped calls with `file::<path>` format consistent with import edges.
- [x] 4.2 Tests: module-scoped `const db = createPool()` produces call edge with `file::<path>` caller; function-scoped calls still use function symbol as caller.

## 5. Tantivy schema and search fields (cruxe-state, cruxe-query)

- [x] 5.1 Add `role` as STRING field to `build_symbols_schema()` (tantivy_index.rs:160-213): `builder.add_text_field("role", STRING | STORED);`. Also add `"role"` to `REQUIRED_SYMBOL_FIELDS` array (tantivy_index.rs:19-32) for index compatibility gate.
- [x] 5.2 Derive and populate `role` from `kind.role()` at write time in `writer.rs`: `doc.add_text(f_role, &sym.kind.role().to_string());` (do NOT add role field to SymbolRecord — compute on-the-fly during document construction).
- [x] 5.3 Add field-level boost weights to symbol search QueryParser (search.rs:875). Replace:
  ```rust
  let query_parser = QueryParser::for_index(index, search_fields);
  ```
  with field-specific boosts: `symbol_exact=10.0`, `qualified_name=3.0`, `signature=1.5`, `path=1.0`, `content=0.5`. Use Tantivy's `QueryParser::for_index` + `set_field_boost()` or equivalent API.
- [x] 5.4 Add index compatibility gate: `open_or_create_index` already checks `REQUIRED_SYMBOL_FIELDS` (tantivy_index.rs:62). Adding `"role"` to the array (task 5.1) means old indices without the field will fail to open and require reindex. Log a clear warning message.

## 6. Ranking enhancement (cruxe-query)

- [x] 6.1 Add `kind_weight(kind: &str) -> f64` function in `ranking.rs` with tiered weights: class/interface/trait=2.0, struct/enum=1.8, type_alias/function/method=1.5, constant=1.0, module=0.8, variable=0.5. Unknown kinds return 0.0.
- [x] 6.2 Add `query_intent_boost(query: &str, kind: &str) -> f64`: uppercase-start (no underscores) queries boost type kinds (+1.0); lowercase/underscore queries boost callable kinds (+0.5); ambiguous → 0.0.
- [x] 6.3 Add `test_file_penalty(path: &str) -> f64`: -0.5 for paths matching `_test.`, `.test.`, `.spec.`, `/test/`, `/tests/`, `test_`. Applied at most once regardless of multiple pattern matches.
- [x] 6.4 Integrate all new boost signals into `rerank()` function (ranking.rs:20-69) alongside existing boosts. Specifically:
  - Replace `let kind_match = 0.0_f64;` (ranking.rs:29) with `kind_weight() + query_intent_boost()`.
  - Compute `test_file_penalty()` separately.
  - Add both `kind_match` and `test_file_penalty` to the `boost` sum (ranking.rs:55-56).
  - Update `RankingReasons` construction (ranking.rs:60-68) to populate `kind_match` with computed value and `test_file_penalty` with its value.
  - Also update the second `rerank` function for locate results (ranking.rs:134) which has the same `kind_match = 0.0` pattern.
- [x] 6.5 Add `role` filter to search query builder: `role` only → Tantivy term query on materialized `role` field; `kind` + `role` → intersection (for `locate_symbol`).
- [x] 6.6 Update `explain_ranking.rs` to report actual computed values:
  - `RankingScoringBreakdown.kind_match` (explain_ranking.rs:36) will now receive non-zero values (kind_weight + query_intent_boost) from the ranking layer.
  - `kind_match_reason` logic (explain_ranking.rs:136-137) will produce descriptive reasons like `"kind-specific boost applied (contribution=2.000)"` instead of always `"no kind-specific boost"`.
  - Add `test_file_penalty: f64` and `test_file_penalty_reason: String` fields to `RankingScoringBreakdown` and `RankingScoringDetails` respectively, matching the new field in `RankingReasons`.
  - The `total` formula already sums all breakdown fields; add `test_file_penalty` to the sum.
- [x] 6.7 Tests: kind weights applied correctly; type-named query ranks struct/class higher; test file results penalized; role filter returns cross-language matches; explain_ranking reports non-zero kind_match.
- [x] 6.8 Semantic hit metadata enrich: in `search.rs`, after `semantic_query()` returns (line ~287) and before `blend_hybrid_results()` (line ~293), iterate semantic results and for each result where `kind.is_none()` and `symbol_stable_id.is_some()`, look up the symbol in the Tantivy symbols index to fill `kind`, `name`, `qualified_name`. After `blend_hybrid_results()`, apply `kind_weight()`, `query_intent_boost()`, and `test_file_penalty()` to blended results that were not previously reranked (i.e., `provenance == "semantic"`). This ensures D6 ranking signals apply to semantic-only hits. (~20 lines.)

## 7. find_references improvement (cruxe-query)

- [x] 7.1 Add `unresolved_count` query after existing `query_edge_rows` in `find_references.rs`:
  ```sql
  SELECT COUNT(*) FROM symbol_edges
  WHERE repo = ?1 AND "ref" = ?2
    AND to_symbol_id IS NULL AND to_name LIKE ?3
  ```
  (Works for both unresolved calls and imports after task 3.1 unifies representation.)
- [x] 7.2 Add `unresolved_count: usize` field to `FindReferencesResult` struct (find_references.rs:46-50). Populate from query in 7.1. Always emit (0 when no unresolved edges).
- [x] 7.3 Tests: response includes correct `unresolved_count` for both unresolved calls and imports; zero unresolved is explicit `0`.

## 8. MCP tool updates (cruxe-mcp)

- [x] 8.1 Add `role` parameter to `search_code` tool definition (tools/search_code.rs) with `enum: ["type", "callable", "value", "namespace", "alias"]`.
- [x] 8.2 Add `role` parameter to `locate_symbol` tool definition (tools/locate_symbol.rs) alongside existing `kind`; describe intersection semantics when both provided.
- [x] 8.3 Update `handle_locate_symbol` (tool_calls/query.rs:215) to extract `role` from arguments and pass to query layer. Update `locate_symbol()` (locate.rs:38) and `locate_symbol_vcs_merged()` (locate.rs:168) function signatures to accept `role: Option<&str>`. Implement role filter as Tantivy term query on `role` field; when both `kind` and `role` are provided, add both as `Occur::Must` clauses (intersection).
- [x] 8.4 Update `handle_search_code` (tool_calls/query.rs) to extract `role` from arguments and pass to search query builder. `search_code` supports `role` only — `kind` parameter is not accepted.
- [x] 8.5 Update `find_references` response serialization to include `unresolved_count`.
- [x] 8.6 Update test mocks and assertions:
  - `tool_calls.rs:500-509` (`ranking_payload_basic_uses_compact_fields`): update `RankingReasons` construction to use realistic `kind_match` value instead of hardcoded `0.0`, add `test_file_penalty` field, adjust `final_score` accordingly.
  - `server/tests.rs:3168`: existing assertion `assert!(first.get("kind_match").is_some())` will pass as-is since field name is unchanged; verify assertion still holds after ranking changes.

## 9. Cleanup and migration

- [x] 9.1 Remove `enricher` references from `Cargo.toml` if any module-level declarations exist. Remove enricher module declarations from `languages/mod.rs` (lines 8-12).
- [x] 9.2 Run `cargo test --workspace` — all existing tests pass with changes.
- [x] 9.3 Run `cargo clippy --workspace` — no new warnings.
- [x] 9.4 Update OpenSpec artifacts with implementation evidence.

## Dependency order

```
1 (core types) → 2 (generic mapper) → 3 (import + unresolved) → 4 (call edge fix)
1 (core types) → 5 (tantivy schema) → 6 (ranking) → 8 (MCP tools)
3 (unresolved unification) → 7 (find_references) → 8 (MCP tools)
9 (cleanup) after all above
```

Task 5.3 (field boosts) is independent of the generic mapper and can be implemented in parallel with task group 2.
Task 6.6 (explain_ranking) depends on 6.4 but can be tested independently.
Task 6.8 (semantic hit metadata enrich) depends on 6.1-6.4 for the ranking signal functions and on the Tantivy symbol lookup infrastructure.
Task 8.6 (test mock updates) should be done last within group 8 since it depends on the actual values from ranking changes.

## Implementation evidence (2026-02-28)

- `cargo test --workspace --no-run` ✅
- `cargo clippy --workspace --all-targets -- -D warnings` ✅
- `cargo test -p cruxe-query semantic_backend_error_sets_semantic_fallback_metadata -- --nocapture` ✅
- `cargo test -p cruxe-query semantic_fanout_limits_apply_caps -- --nocapture` ✅
- `cargo test -p cruxe-indexer extract_call_edges_uses_file_source_for_module_scope_calls -- --nocapture` ✅
- `cargo test -p cruxe-indexer resolve_imports_uses_to_name_for_unresolved_target -- --nocapture` ✅
- `cargo test -p cruxe-mcp t403_search_code_exposes_semantic_and_confidence_metadata -- --nocapture` ✅
- `openspec validate refactor-multilang-symbol-contract` ✅
