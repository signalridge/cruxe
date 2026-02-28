## Why

Cruxe's symbol extraction pipeline carries per-language enricher complexity (650 lines across 4 languages) that delivers marginal search quality improvement. Meanwhile, the highest-impact gaps — ranking signals, reference resolution, and search field utilization — receive no optimization. This change rebalances investment toward measurable search quality gains.

Key findings from codebase audit and competitive analysis:

- **`visibility`** is indexed but never used in search, filtering, or ranking across all 18 MCP tools. The 4 enrichers invest ~120 lines producing this field.
- **`signature`** participates in BM25 scoring as one of 5 symbol search fields (search.rs:853) but receives no explicit ranking boost or field weight differentiation.
- **`qualified_name`** contributes a ranking boost of 2.0 (`ranking.rs:42`). This is the only enricher output that measurably affects search relevance beyond BM25.
- **`kind_match`** is a reserved placeholder at 0.0 (`ranking.rs:29`). Zoekt (Google/Sourcegraph code search) uses tiered kind weights as a major ranking signal: generic tier Class=10, Struct=9.5, Function/Method=7, Variable=4, multiplied by `scoreKindMatch=100` for ~1000-point impact in a ~9000-point total score (`contentprovider.go:scoreSymbolKind`). Zoekt further provides per-language override tables for 10 languages.
- **All 5 symbol search fields have equal BM25 weight** — a match in body text ranks the same as a match in symbol name. No field-level boost weights applied.
- **Import path resolution** in `import_extract.rs:79-123` uses two-tier lookup (qualified_name → name fallback) but has no module path alias handling. Module-aliased imports produce `unresolved_symbol_stable_id` blake3 hashes (import_extract.rs:125-127) that never match real symbols and are invisible to `find_references`.
- **Module-scoped calls** (`const x = foo()`) are dropped at `call_extract.rs:24-26` when `resolve_caller_symbol` returns `None`.
- **Unresolved edges** (where `to_symbol_id IS NULL`) are never returned by `find_references.rs:241-274`.
- No successful code search tool (Aider, Continue, Zoekt) writes per-language symbol *extraction* code. They use tree-sitter/ctags generically for extraction and delegate deeper semantic enrichment to external analyzers. (Zoekt does maintain per-language *ranking weight* tables in `scoreSymbolKind`, but this is ~200 lines of pure data in one function, not extraction logic.)

## What Changes

**1. Replace per-language enrichers with generic tag mapper**
- Replace 4 enrichers (650 lines) with a single generic mapper (~120 lines).
- `map_kind` becomes a shared lookup table with `has_parent` for Function→Method promotion and `node.kind()` for struct/enum/type disambiguation.
- `qualified_name` constructed via generic parent-scope walking with universal generic argument stripping.
- New language = register grammar + tags.scm. Zero custom code.
- SymbolRole added for cross-language semantic grouping.

**2. Fix import path resolution**
- Phase 1: Relative path resolution (TS `./foo`, Rust `super::`, Python `.module`).
- Resolved paths fed into file-constrained `symbol_relations` lookup (more precise than current qualified_name matching).
- Phase 2 (separate change): Config-based alias resolution (TS `tsconfig.json` paths, Go `go.mod`).

**3. Fix call edge gaps**
- Module-scoped calls use `file::<path>` pseudo-symbol as caller (matches import edge format).
- `find_references` returns `unresolved_count` for edges with `to_symbol_id IS NULL`.

**4. Ranking: Zoekt-inspired tiered kind scoring**
- Replace dead `kind_match = 0.0` with tiered kind weight table (class=2.0, function=1.5, variable=0.5).
- Add query-intent boost: uppercase queries boost type symbols, lowercase/underscore boost callables.
- Add test file penalty (-0.5).
- Enrich semantic-only results with symbol metadata from Tantivy before applying ranking signals.
- Add `role` filter to `search_code` and `locate_symbol` MCP tools.

**5. Tantivy field-level boost weights**
- Add field-level boost weights to symbol QueryParser: symbol_exact=10.0 > qualified_name=3.0 > signature=1.5 > path=1.0 > content=0.5.

**6. Unresolved import representation unification**
- Change unresolved import edges from blake3 hash IDs to `NULL` + `to_name` (consistent with call edges).
- Enables unified `unresolved_count` query across both edge types.

## Capabilities

### New Capabilities
- `generic-tag-mapper`: Language-agnostic tag→symbol mapping replacing per-language enrichers.
- `symbol-role-layer`: Cross-language `SymbolRole` with deterministic kind→role mapping and role-aware query filtering.
- `import-path-resolution`: Relative path resolution for import edge accuracy.

### Modified Capabilities
- `tag-based-symbol-extraction`: Unified extraction through generic mapper; enricher trait removed.
- `002-agent-protocol`: Tiered kind scoring, query-intent boost, test file penalty, field boost weights; `find_references` includes `unresolved_count`; `search_code` supports `role` filter; `locate_symbol` supports `role` filter (in addition to existing `kind`).

## Impact

- Affected crates: `cruxe-core` (types, SymbolRole), `cruxe-indexer` (languages/, import_extract, call_extract), `cruxe-query` (find_references, ranking, role filtering, field boosts), `cruxe-mcp` (tool parameters, response schema), `cruxe-state` (Tantivy schema).
- API impact: additive — `role` filter, `unresolved_count`, ranking changes are new; existing `kind` filter unchanged.
- Data/index impact: full reindex required (role field addition, field boost changes, unresolved import representation change).
- Net code delta: ~+450 lines (generic mapper, import resolution, ranking, field boosts, unresolved unification) / ~-650 lines (enrichers) = **net reduction ~200 lines**.
