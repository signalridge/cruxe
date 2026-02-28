## Context

Cruxe uses tree-sitter-tags plus per-language enrichers (Rust/TypeScript/Python/Go) to extract symbols. Codebase audit and competitive analysis reveal the enricher architecture delivers marginal search quality gain while consuming significant maintenance budget. The real quality gaps are in ranking signals and reference resolution.

Current pipeline:
```
tags.scm → tree-sitter-tags → enricher (150 lines/language) → ExtractedSymbol → Tantivy + SQLite
```

Proposed pipeline:
```
tags.scm → tree-sitter-tags → generic_mapper (shared) → ExtractedSymbol → Tantivy + SQLite
                                                                            ↑
                                                              import path resolver (per-language)
```

Measured impact of current enricher outputs on search:

| Enricher output | Used in ranking? | Used in filtering? | Ranking weight |
|----------------|-----------------|-------------------|---------------|
| `map_kind` → `kind` | No (`kind_match = 0.0`, ranking.rs:29) | Yes (`locate_symbol` only) | 0.0 |
| `find_parent_scope` → `qualified_name` | Yes (ranking.rs:42) | No | 2.0 |
| `extract_visibility` → `visibility` | No | No | 0.0 |
| signature (hardcoded in tag_extract.rs:89-98) | Indirect (BM25 via search field, search.rs:853) | No | BM25 only (no explicit boost) |

Per-language enricher investment that affects search: parent_scope → qualified_name (weight 2.0). Signature participates in BM25 via search field inclusion but receives no explicit ranking boost. Visibility is never used.

### Competitive analysis

No successful open-source code search tool writes per-language enricher code:

| Tool | Per-language code | Extraction strategy | Ranking strategy |
|------|------------------|--------------------|-----------------|
| Aider | 4-50 lines `.scm` | Flat def/ref tags, zero enrichment | PageRank on file graph |
| Continue | 20-40 lines `.scm` | Pre-body capture concatenation as signature | Vector + FTS5 |
| Zoekt | 0 (uses ctags) | ctags symbol sections | BM25 + per-language kind weights × 100 |
| stack-graphs | 800+ lines `.tsg` | Full scope graph from tree-sitter | Path finding (abandoned by GitHub) |

Key insight from Zoekt: **symbol kind scoring is a major ranking signal** — Zoekt's `scoreSymbolKind` (contentprovider.go) applies a generic tier (Class=10, Struct=9.5, Enum=9, Interface=8, Function/Method=7, Field=5.5, Constant=5, Variable=4) multiplied by `scoreKindMatch=100`, making kind weight a ~1000-point signal in a ~9000-point total score. Zoekt further provides per-language overrides for 10 languages (Java, Go, Python, C++, Kotlin, Scala, Ruby, PHP, GraphQL, Markdown). Cruxe adopts the tiered approach but uses a single generic table — per-language tuning is a future optimization, not a prerequisite for improvement over the current `kind_match=0.0`.

### Zoekt ranking architecture deep-dive

Source: `sourcegraph/zoekt` `index/score.go` + `index/contentprovider.go` (verified against commit `c747a3bc`).

Zoekt has two scoring paths — classic (additive per-line) and BM25 (term-frequency). Both feed into file-level scoring.

**Classic scoring path** — per-line/chunk additive signals:

| Signal | Value | Description |
|--------|-------|-------------|
| `scoreSymbol` | 7000 | Match text overlaps a ctags symbol definition section |
| `scorePartialSymbol` | 4000 | Partial overlap with symbol section |
| `scoreBase` | 7000 | Exact match on filename basename (after last `/`) |
| `scorePartialBase` | 4000 | Partial basename match |
| `scoreWordMatch` | 500 | Full word boundary match (both sides) |
| `scorePartialWordMatch` | 50 | One-sided word boundary match |
| `scoreKindMatch` × factor | 100-1000 | Symbol kind weight (factor 1-10, per language) |
| `scoreFactorAtomMatch` | 400 | Multi-clause query bonus (file-level) |

Total score budget: ~9000 per line/chunk. Symbol definition overlap (7000) and basename match (7000) are the dominant signals. Kind weight (max 1000) is a significant tiebreaker among symbol matches.

**BM25 scoring path** — term-frequency with boosts:

| Signal | Value | Description |
|--------|-------|-------------|
| `importantTermBoost` | 5× TF | Symbol or filename match counts as 5 term hits |
| `lowPriorityFilePenalty` | /5 TF | Test/vendor files (via `go-enry`) get 1/5 term frequency |
| `boostScore` | variable | External multiplier from match tree |
| BM25 params | k=1.2, b=0.75 | Standard Lucene defaults |

**File-level and tiebreaker signals:**

| Signal | Description |
|--------|-------------|
| `scoreRepoRankFactor` × 100 | Repository rank (0-65535) |
| `scoreFileOrderFactor` × 10 | Document order within shard |
| `scoreLineOrderFactor` × 1.0 | Line ordering within file |
| `boostNovelExtension` | Promotes file with novel extension to 3rd position for diversity |

**Go-specific scoring (in `scoreSymbolKind`):**
- Exported symbols (uppercase first char): `factor += 0.5`
- Test files (`_test.go`): `factor *= 0.8`

**Zoekt signal → cruxe adoption mapping:**

| Zoekt signal | Zoekt value | cruxe adoption | cruxe value | Rationale |
|-------------|------------|---------------|------------|-----------|
| `scoreSymbol` | 7000 (78% of max) | Already exists as `definition_boost` | 1.0 (11% of max) | Different architecture: cruxe separates symbols/snippets/files via RRF weights (1.0/0.5/0.3) before reranking. Combined with RRF, definition signal is adequately represented. |
| `scoreBase` | 7000 (78% of max) | Already exists as `exact_match_boost` | 5.0 (56% of max) | cruxe matches on symbol name, not filename basename. Proportionally similar to Zoekt. |
| `scoreKindMatch` | 100-1000 (11% of max) | **NEW — this change** | 0.5-2.0 (22% of max) | Currently 0.0 (dead). Proportionally larger relative impact than Zoekt. |
| `scoreWordMatch` | 500 | Not adopted (BM25 implicit) | — | Word boundary matching handled by Tantivy BM25 at retrieval time. |
| `scoreFactorAtomMatch` | 400 | Not adopted (BM25 implicit) | — | Multi-term matching handled by BM25 term frequency. |
| `importantTermBoost` | 5× TF | Approximated by field boost weights | 10.0/3.0/1.5/1.0/0.5 | Field-level boost weights (D7) achieve similar effect: symbol name matches weighted higher than body content. |
| `lowPriorityFilePenalty` | /5 TF | **NEW — this change** (test penalty) | -0.5 | Zoekt is much more aggressive (/5 = 80% reduction). cruxe uses conservative -0.5 additive penalty. |
| Go exported boost | +0.5 factor | Partially covered by query-intent boost | 0.0-1.0 | Query-intent boost is query-driven (uppercase query → type boost), not language-driven (Go export → boost). Different mechanism, related intent. |
| `boostNovelExtension` | display-level | Not adopted (deferred) | — | Result diversity is a display-layer concern, not a ranking signal. |
| `scoreRepoRankFactor` | 100 | Not applicable | — | Cruxe is single-repo per index. |

Project constraints:
- Local-first indexing and querying.
- No mandatory external indexer/service/runtime installation.
- Compatibility with existing MCP/query surfaces and stable-ID workflows.
- Contract design must not block future SCIP data source integration.

## Goals / Non-Goals

**Goals:**

1. Replace per-language enrichers with a generic tag→symbol mapper.
2. Add `SymbolRole` for cross-language semantic grouping.
3. Fix import path resolution for relative paths.
4. Capture module-scoped call edges.
5. Surface unresolved reference counts.
6. Implement Zoekt-inspired tiered kind ranking and add `role` filter to MCP tools.
7. Add Tantivy field-level boost weights for search quality.
8. Enable zero-cost language expansion (register grammar only).

**Non-Goals:**

1. Quality/provenance metadata for enrichment fields.
2. Capability-tier language metadata registry.
3. Visibility vocabulary normalization.
4. SCIP/LSP integration (future work).
5. Full cross-file semantic resolution (type inference, macro expansion).
6. Full scope graph construction (stack-graphs approach — proven uneconomical).

## Decisions

### D1. Generic tag→symbol mapper replaces per-language enrichers

#### Decision

Replace `LanguageEnricher` trait and 4 implementations with a single `GenericTagMapper` module. The mapper provides:

1. **Kind mapping** — shared lookup table from tag kind strings to `SymbolKind`:
```rust
fn map_tag_kind(tag_kind: &str, has_parent: bool, node_kind: Option<&str>) -> Option<SymbolKind> {
    match tag_kind {
        "function" => if has_parent { Some(SymbolKind::Method) } else { Some(SymbolKind::Function) },
        "method"   => Some(SymbolKind::Method),
        "class"    => match node_kind {
            Some("enum_item" | "enum_declaration") => Some(SymbolKind::Enum),
            Some("type_item" | "type_alias_declaration") => Some(SymbolKind::TypeAlias),
            Some("struct_item" | "union_item") => Some(SymbolKind::Struct),
            _ => Some(SymbolKind::Class),
        },
        "interface" => Some(SymbolKind::Interface),
        "module"    => Some(SymbolKind::Module),
        "constant"  => Some(SymbolKind::Constant),
        "variable"  => Some(SymbolKind::Variable),
        _           => None,
    }
}
```

The `has_parent` parameter preserves Function→Method promotion for nested definitions (Python and TypeScript enrichers currently use this). The `node_kind` fallback handles struct/enum/union/type disambiguation already used by enrichers — critical because Rust's `tags.scm` maps `struct_item`, `enum_item`, `union_item`, and `type_item` all to the `"class"` tag kind via `@definition.class`.

2. **Parent scope** — generic AST parent walking with generic argument stripping:
```rust
fn find_parent_scope(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut current = node.parent()?;
    loop {
        match current.kind() {
            k if is_scope_node(k) => {
                let raw = current.child_by_field_name("name")
                    .or_else(|| current.child_by_field_name("type"))
                    .map(|n| node_text(n, source).to_string())?;
                return Some(strip_generic_args(&raw));
            }
            k if is_transparent_node(k) => { current = current.parent()?; }
            _ => return None,
        }
    }
}

/// Strip generic arguments universally: `Foo<T>` → `Foo`, `Bar[T]` → `Bar`
fn strip_generic_args(name: &str) -> String {
    // Handle Rust <T>, Go [T], and nested cases like Foo<Bar<T>>
    let mut result = String::new();
    let mut depth = 0;
    for ch in name.chars() {
        match ch {
            '<' | '[' => depth += 1,
            '>' | ']' => { depth -= 1; }
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}
```

Where `is_scope_node` matches: `impl_item`, `trait_item`, `struct_item`, `enum_item`, `mod_item`, `class_definition`, `class_declaration`, `interface_declaration`, `internal_module`, `method_declaration` (Go receiver), `type_declaration`, `abstract_class_declaration`.

Where `is_transparent_node` matches: `declaration_list`, `class_body`, `object_type`, `statement_block`, `block`, `decorated_definition`.

Generic argument stripping resolves Open Question 1 — stripping universally is safe because `qualified_name` is used for human display and ranking boost, not for identity (that's `symbol_stable_id`). This replaces Rust-specific `normalize_rust_parent` and Go-specific `normalize_go_receiver`.

3. **Separator** — derived from language:
```rust
fn separator_for_language(language: &str) -> &'static str {
    match language {
        "rust" => "::",
        _ => ".",
    }
}
```

4. **Signature** — language-agnostic extraction using first line of definition:
```rust
fn extract_signature(kind: SymbolKind, source: &str, tag_line_range: Range<usize>) -> Option<String> {
    if matches!(kind, SymbolKind::Function | SymbolKind::Method) {
        source.get(tag_line_range).map(|s| s.trim().to_string())
    } else {
        None
    }
}
```

This preserves the existing tag_extract.rs:89-98 behavior without per-language code. Signature participates in BM25 scoring as one of the 5 symbol search fields (search.rs:853) but receives no explicit ranking boost. The generic mapper preserves the same extraction logic, so `symbol_stable_id` (which includes signature as input) remains stable.

5. **Visibility** — not extracted. Returns `None`. Zero search impact confirmed by audit: `visibility` is stored in Tantivy but never queried, filtered, or ranked across all 18 MCP tools.

#### What is lost

| Capability | Current enricher | Generic mapper | Search impact |
|-----------|-----------------|---------------|--------------|
| Rust struct vs union distinction | `union_item` → `Struct` | Same via node_kind fallback | None (both map to `Type` role) |
| Rust `pub(crate)` visibility text | Extracted | `None` | None (visibility unused in search) |
| Go capitalization-based visibility | Convention inference | `None` | None |
| Python `_`/`__` visibility convention | Convention inference | `None` | None |
| TS `export` visibility | AST check | `None` | None |
| Rust `impl<T> Foo<T>` generic stripping | `normalize_rust_parent` | Universal `strip_generic_args` | None — generic approach covers all languages |
| Go method receiver type extraction | `method_declaration` receiver parsing | Generic parent walker with `"type"` field fallback | None — `child_by_field_name("type")` handles Go receivers |
| Python dunder visibility (`__init__` = public) | Convention check | `None` | None |
| TS `const` vs `let` variable distinction | `is_const_declaration()` check | Both map to `Variable` | Low — enricher maps `const` to `Constant`, generic maps to `Variable`. Tag-level fix: custom query already tags const as `@definition.constant` in tag_registry.rs |

#### Rationale

650 lines of enricher code → ~120 lines of generic mapper. New language cost drops from ~150 lines to ~10 lines (register grammar). No successful code search tool (Aider, Continue, Zoekt) writes per-language symbol *extraction* code — they use tree-sitter/ctags generically. (Zoekt does maintain per-language *ranking weight* tables, but these are pure data, not extraction logic.) The only enricher output with measurable search impact (`qualified_name`, weight 2.0) is preserved by the generic parent walker.

#### Alternatives considered

- Keep enrichers but add default `None`. Rejected: still requires per-language maintenance, still invites complexity growth.
- Remove parent scope walking entirely. Rejected: `qualified_name` boost (2.0) is meaningful.
- Use tree-sitter-graph (.tsg) files. Rejected: 5-10x more verbose than Rust code for same capability (Python TSG = 800+ lines vs enricher = 154 lines). Only worthwhile for 50+ language community contributions.

---

### D2. Add SymbolRole with deterministic kind→role mapping

#### Decision

Add `SymbolRole` enum to `cruxe-core`:

```rust
pub enum SymbolRole { Type, Callable, Value, Namespace, Alias }
```

Mapping:
- `Function`, `Method` → `Callable`
- `Struct`, `Class`, `Enum`, `Trait`, `Interface` → `Type`
- `Constant`, `Variable` → `Value`
- `Module` → `Namespace`
- `TypeAlias` → `Alias`

`Import` removed from `SymbolKind` (dead value — `grep SymbolKind::Import` returns zero uses in enrichers/extractors). `from_str("import")` maps to `Module` for legacy deserialization.

`role` materialized as Tantivy STRING field in symbols index, derived from `kind.role()` at write time (`writer.rs`). The `role` field is NOT added to `SymbolRecord` — it is computed on-the-fly during Tantivy document construction to minimize churn across the ~44 `SymbolRecord` construction sites in the codebase.

Query filter semantics:
- `kind` only → exact-kind filtering (existing behavior, available on `locate_symbol`).
- `role` only → matches all kinds mapping to that role (available on both `search_code` and `locate_symbol`).
- Both → intersection (available on `locate_symbol` which already has `kind` parameter).

Note: `search_code` gains `role` filter only (not `kind`). For `search_code`, role-level filtering is the right abstraction — agents searching across languages should use semantic roles, not language-specific kinds. The `kind` filter remains exclusive to `locate_symbol` where precise symbol lookup is the intent.

#### Rationale

AI agent searching "find all type definitions" across a Rust+Python repo currently requires knowing to search for both `struct` and `class`. SymbolRole enables `role=Type` which matches both. This aligns with SCIP's Descriptor.Suffix concept (Namespace, Type, Term, Method) which groups fine-grained kinds into coarse categories.

---

### D3. Fix import path resolution

#### Decision

Add a `resolve_import_path` function in `import_extract.rs` that runs before the existing `resolve_target_symbol_stable_id` lookup.

**Phase 1: Relative paths** (no config file parsing)

```rust
fn resolve_import_path(
    importing_file: &str,   // e.g., "src/api/handler.ts"
    module_spec: &str,      // e.g., "./services/user"
    language: &str,
) -> Option<String> {
    // Returns resolved relative path: "src/services/user.ts"
}
```

Per-language rules:

| Language | Input | Resolution |
|----------|-------|-----------|
| TypeScript | `./services/user` | Join with importing dir, try `.ts`, `.tsx`, `/index.ts` |
| Rust | `super::utils` | Walk module tree: `super` = parent dir, then `utils.rs` or `utils/mod.rs` |
| Python | `.utils` | Relative to `__init__.py` package: same dir `utils.py` |
| Go | N/A for relative (Go uses absolute module paths) | Skip in Phase 1 |

After resolving to a file path, look up the file in `file_manifest` to confirm existence, then query `symbol_relations` for that specific file:

```sql
SELECT symbol_stable_id FROM symbol_relations
WHERE repo = ?1 AND "ref" = ?2 AND path = ?3 AND name = ?4
LIMIT 1
```

This is more precise than the current qualified_name lookup (import_extract.rs:85-101) because it constrains to the resolved file.

**Phase 2: Config-based aliases** (follow-up, separate change)

- TS: parse `tsconfig.json` `compilerOptions.paths` for `@alias/*` → `src/*` mappings.
- Go: parse `go.mod` for module path prefix.

#### Rationale

Import resolution is the #1 quality gap in `find_references`. Current two-tier lookup (qualified_name → name fallback) fails for module-aliased paths. Constraining lookup to resolved file path eliminates false positives from name collisions across files.

---

### D4. Fix module-scoped call edges

#### Decision

In `call_extract.rs`, when `resolve_caller_symbol` (line 118-124) returns `None`, use `source_symbol_id_for_path(path)` (`import_extract.rs:18-20`) as the caller instead of `continue`.

Change from:
```rust
let Some(caller) = resolve_caller_symbol(symbols, site.line) else {
    continue;  // Module-scope calls silently dropped
};
```

To:
```rust
let caller_id = match resolve_caller_symbol(symbols, site.line) {
    Some(caller) => caller.symbol_stable_id.clone(),
    None => source_symbol_id_for_path(path),  // file::<path>
};
```

This is consistent with import edges, which already use `file::<path>` as `from_symbol_id`.

#### Rationale

Module-level initialization (`const db = createPool()`, `static INSTANCE: Lazy<Pool>`) represents real dependencies. These call sites are common in Go (package-level `init`), TypeScript (module-level constants), and Rust (`lazy_static`/`OnceLock` initialization).

---

### D5. Surface unresolved references

#### Decision

Add `unresolved_count` to `find_references` response.

**Historical unresolved representation was inconsistent across edge types:**
- **Call edges**: unresolved targets stored as `to_symbol_id = NULL, to_name = Some(callee_name)` (call_extract.rs:34-35).
- **Import edges (before this refactor)**: unresolved targets used a synthetic `blake3("unresolved:" + qualified_name)` ID.
- **Final aligned state (after this refactor)**: both edge types use `to_symbol_id = NULL` with `to_name` populated.

**Step 1: Unify unresolved representation.**

Change `resolve_imports()` to use `to_symbol_id = NULL, to_name = Some(import_name)` for unresolved imports, consistent with call edges. This replaces the current blake3 hash approach. The hash IDs are never matched by `find_references` today anyway, so this is a behavior-preserving change for query consumers.

**Step 2: Count unresolved edges.**

After the existing `query_edge_rows` (find_references.rs:241-274), run a count query:

```sql
SELECT COUNT(*) FROM symbol_edges
WHERE repo = ?1 AND "ref" = ?2
  AND to_symbol_id IS NULL
  AND to_name LIKE ?3
```

Where `?3` is the target symbol name (with `%` suffix for prefix matching). This now correctly counts both unresolved calls AND unresolved imports.

MCP response shape addition:
```json
{
  "references": [...],
  "total": 5,
  "unresolved_count": 3
}
```

#### Rationale

Users currently see "0 references" when the actual situation is "0 resolved + 5 unresolved." The count gives AI agents a signal to fall back to text search for unresolved references. Unifying the NULL representation is a prerequisite — without it, unresolved imports are invisible to the count query.

---

### D6. Ranking: Zoekt-inspired tiered kind scoring

#### Decision

Replace the current dead `kind_match = 0.0` placeholder with a tiered kind scoring system inspired by Zoekt's empirically-tuned weights.

**Symbol kind weight table** (applied as additive boost in `ranking.rs`):

```rust
fn kind_weight(kind: &str) -> f32 {
    match kind {
        "class" | "interface" | "trait" => 2.0,
        "struct" | "enum"              => 1.8,
        "type_alias"                   => 1.5,
        "function" | "method"          => 1.5,
        "constant"                     => 1.0,
        "module"                       => 0.8,
        "variable"                     => 0.5,
        _                              => 0.0,
    }
}
```

Rationale for weight scale: Zoekt uses a multiplicative architecture — kind factor (1-10) × `scoreKindMatch` (100) = up to 1000 points applied at match-time per line/chunk. Cruxe uses an additive reranking architecture — boosts applied post-retrieval on top of RRF+BM25 scores. The existing boosts are: exact_match=5.0, qualified_name=2.0, definition=1.0, path_affinity=1.0. Kind weights scaled to 0.5-2.0 range to be meaningful without dominating exact_match. Zoekt's per-language overrides (e.g., Go exported symbol +0.5, Go `_test.go` ×0.8) are not adopted in Phase 1 — a single generic table provides the 80/20 improvement.

**Query-intent kind boost** (additional, stacks with base kind weight):

```rust
fn query_intent_boost(query: &str, kind: &str) -> f32 {
    let is_type_query = query.chars().next().map_or(false, |c| c.is_uppercase())
                        && !query.contains('_');
    let is_fn_query = query.chars().next().map_or(false, |c| c.is_lowercase())
                      || query.contains('_');

    if is_type_query && is_type_kind(kind) { 1.0 }
    else if is_fn_query && is_callable_kind(kind) { 0.5 }
    else { 0.0 }
}
```

**Test file penalty** (applied during reranking):

```rust
fn test_file_penalty(path: &str) -> f32 {
    if is_test_file(path) { -0.5 } else { 0.0 }
}

fn is_test_file(path: &str) -> bool {
    let p = path.to_lowercase();
    let filename = p.rsplit('/').next().unwrap_or(&p);
    p.contains("_test.") || p.contains(".test.") || p.contains(".spec.")
    || p.contains("/test/") || p.contains("/tests/") || filename.starts_with("test_")
}
```

**Combined ranking formula** (replaces ranking.rs:14-57):

```rust
let total_boost = exact_match_boost        // 5.0 (unchanged)
    + qualified_name_boost                  // 2.0 (unchanged)
    + kind_weight(result.kind)              // 0.5-2.0 (NEW)
    + query_intent_boost(query, result.kind)// 0.0-1.0 (NEW)
    + definition_boost                      // 1.0 (unchanged)
    + path_affinity                         // 1.0 (unchanged)
    + test_file_penalty(result.path);       // -0.5 or 0.0 (NEW)
```

**Semantic hit metadata enrichment** (enables D6 signals for semantic results):

Currently `semantic_query()` (hybrid.rs:73-93) returns results with `symbol_stable_id` but `kind=None, name=None, qualified_name=None`. Lexical `rerank()` runs before `blend_hybrid_results()` (search.rs:255-293), so semantic-only hits never receive kind/test ranking signals.

Fix: after `semantic_query()` returns, enrich semantic results by looking up each `symbol_stable_id` in the Tantivy symbols index to fill `kind`, `name`, `qualified_name`. After `blend_hybrid_results()`, apply `kind_weight()`, `query_intent_boost()`, and `test_file_penalty()` to results with `provenance == "semantic"`. This is ~20 lines and ensures D6 signals apply uniformly regardless of retrieval path.

```rust
// Post-semantic enrichment (in search.rs, after semantic_query)
for result in &mut semantic_results {
    if result.kind.is_none() {
        if let Some(ref sid) = result.symbol_stable_id {
            if let Some((kind, name, qn)) = lookup_symbol_metadata(index, sid) {
                result.kind = Some(kind);
                result.name = Some(name);
                result.qualified_name = qn;
            }
        }
    }
}
```

**Add `role` filter to `search_code` tool** (currently only `locate_symbol` has `kind` filter):

```json
{
  "name": "role",
  "description": "Semantic role filter: type, callable, value, namespace, alias",
  "type": "string",
  "enum": ["type", "callable", "value", "namespace", "alias"]
}
```

#### Rationale

Zoekt (Google/Sourcegraph) uses kind weights as a major code search ranking signal (`scoreSymbolKind` in `contentprovider.go`). Their generic tier: Class=10, Struct=9.5, Enum=9, Interface/Type=8, Function/Method=7, Field=5.5, Constant=5, Variable=4 — multiplied by `scoreKindMatch=100` for ~1000-point impact. Zoekt further provides per-language overrides (10 languages) and Go-specific boosts (exported symbol +0.5). Cruxe's current ranking treats all kinds equally (`kind_match = 0.0`), missing this signal entirely.

The test file penalty is inspired by two Zoekt mechanisms: (1) Go-specific `factor *= 0.8` for `_test.go` in classic scoring, and (2) generic `lowPriorityFilePenalty = /5` term frequency reduction for test/vendor files (via `go-enry`) in BM25 scoring. Cruxe uses a simpler additive `-0.5` penalty applied uniformly across languages — less aggressive than Zoekt's approach, but sufficient for the reranking context.

---

### D7. Tantivy field-level boost weights

#### Decision

Add field-level boost weights to the Tantivy QueryParser for symbol search (search.rs:875).

Currently all 5 symbol search fields (`symbol_exact`, `qualified_name`, `signature`, `content`, `path`) have equal BM25 contribution. A match in body text ranks the same as a match in the symbol name. Add differentiated boosts:

```rust
let query_parser = QueryParser::for_index(&index, vec![
    (symbol_exact_field, 10.0),    // Exact name match — highest priority
    (qualified_name_field, 3.0),   // Namespace-qualified match
    (signature_field, 1.5),        // Signature text match
    (path_field, 1.0),            // Path component match
    (content_field, 0.5),         // Body content — lowest priority
]);
```

Note: snippet search fields (`content`, `path`, `imports`) and file search fields (`path`, `filename`, `content_head`) already include all relevant fields (search.rs:860,864). Only field boost weights are missing.

#### Rationale

Field-level boost weights are the standard approach in text search (Elasticsearch, Solr, Zoekt all use them). A match in `symbol_exact` is fundamentally more relevant than a match in `content` (body text). Without boosts, a function named `config` in one file ranks equally with a variable that mentions `config` in its body — this produces noisy results.

---

### D8. Zero-cost language expansion

#### Decision

With the generic mapper (D1), adding a new language requires only:

1. Add `tree-sitter-{lang}` dependency to `Cargo.toml`.
2. Register grammar in `parser.rs` language dispatch.
3. Add language to `INDEXABLE_SOURCE_LANGUAGES` in `cruxe-core::languages`.
4. Add extension mapping in `detect_language_from_extension`.
5. If the language's `tags.scm` needs custom query additions, add to `tag_registry.rs` `custom_query_extra`.

No enricher code. No per-language mapper. No tests beyond verifying tag extraction works.

Priority languages for expansion (based on tree-sitter grammar + tags.scm availability):
Java, C, C++, Ruby, Kotlin, Swift, C#, PHP.

This is not in scope for this change but the generic mapper makes it possible.

---

### D9. Migration and compatibility

#### Decision

- Full reindex required (role field addition, field boost changes, unresolved import representation change, potential qualified_name changes from generic mapper).
- Index compatibility gate: if symbols index lacks `role` field, require reindex.
- `Import` removed from `SymbolKind`; `from_str("import")` → `Module` for old serialized data.
- `symbol_stable_id` computation unchanged — `kind` input stays as-is, `role` is not an input.
- Unresolved import edges migrate from blake3 hash IDs to `NULL` + `to_name` (consistent with call edges). Old hash IDs cleared by reindex.

## Risks / Trade-offs

- **[Risk] Generic parent walker less accurate than Rust-specific `normalize_rust_parent`**
  -> **Mitigation:** Universal `strip_generic_args` handles both Rust `<T>` and Go `[T]`. Same result as language-specific normalizers.

- **[Risk] Go method receiver requires `"type"` field fallback in generic walker**
  -> **Mitigation:** Already handled by `child_by_field_name("type")` in generic walker. Confirmed by AST inspection: Go `method_declaration` uses `"type"` field for receiver.

- **[Risk] Import resolution Phase 1 only covers relative paths**
  -> **Mitigation:** Phase 1 covers ~60% of import failures. Phase 2 (tsconfig, go.mod) is additive follow-up in separate change.

- **[Risk] Kind weight values may need tuning**
  -> **Mitigation:** Tier ordering derived from Zoekt's generic tier (Class=10 > Struct=9.5 > Function=7 > Variable=4), scaled to cruxe's additive boost range (2.0 > 1.8 > 1.5 > 0.5). Conservative scale prevents dominating exact_match (5.0). Per-language overrides (Zoekt has 10) deferred to future tuning. Can be adjusted without schema changes.

- **[Risk] Test file penalty may penalize legitimate test searches**
  -> **Mitigation:** Penalty is small (-0.5) — a test file with an exact name match (5.0 + 2.0 + 1.5 + ... = ~9.5) still ranks highly. The penalty only affects tiebreaking between otherwise-equal results.

- **[Risk] Reindex required**
  -> **Mitigation:** Explicit compatibility gate with clear error message.

## Migration Plan

1. **Core types** — Add `SymbolRole`, remove `Import` from `SymbolKind`.
2. **Generic mapper** — Implement `GenericTagMapper`, delete enricher files.
3. **Import resolution** — Phase 1 relative paths in `import_extract.rs`.
4. **Call edge fix** — Module-scoped caller fallback in `call_extract.rs`.
5. **Tantivy schema** — Add `role` field, add field boosts.
6. **Ranking** — Implement tiered kind scoring, test file penalty.
7. **MCP tools** — Add `role` filter to `search_code` and `locate_symbol`.
8. **find_references** — Add `unresolved_count` query and response field.
9. **Migration gate** — Index compatibility check, reindex requirement.

### Rollback strategy

- Generic mapper → restore enricher files (git revert).
- Import resolution → edges fall back to unresolved (current behavior).
- Kind scoring → set weights to 0.0 (reverts to current behavior).
- Test file penalty → remove penalty function.
- Field boosts → remove boost parameters from QueryParser (reverts to equal weighting).
- Unresolved import representation → restore blake3 hash approach (git revert).
- role filter → remove from tool definitions.
- unresolved_count → remove from response shape.
- All changes are independently revertable.

## Resolved Questions

1. **Generic arg stripping:** Universal heuristic — strip `<...>` and `[...]` from all languages. Safe because qualified_name is used for display/ranking, not identity. Implemented in `strip_generic_args`.

2. **Import resolution Phase 2:** Separate follow-up change. Phase 1 (relative paths) is self-contained and covers ~60% of failures.

3. **SymbolKind expansion:** Deferred. Current 11 kinds (minus Import) sufficient for all 4 languages. Expansion (Constructor, Field, Macro) can be added when new languages need them, with no breaking changes.
