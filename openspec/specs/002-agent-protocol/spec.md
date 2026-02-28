## Purpose
Define agent-facing query response controls for explainability, compact payload
shaping, dedup observability, and graceful hard-limit truncation behavior.
## Requirements
### Requirement: Explainability control via ranking_explain_level
`search_code` and `locate_symbol` MUST support explainability control using
`ranking_explain_level` with values `off | basic | full`.

Precedence MUST be:

1. request argument `ranking_explain_level`
2. config default `search.ranking_explain_level`
3. legacy compatibility fallback from `debug.ranking_reasons`

Legacy fallback mapping MUST be deterministic:

- `debug.ranking_reasons = true` -> `full`
- `debug.ranking_reasons = false` -> `off`

Behavior:

- `off`: `ranking_reasons` MUST be omitted
- `basic`: `ranking_reasons` MUST include compact normalized factors only
- `full`: `ranking_reasons` MUST include full deterministic scoring breakdown

#### Scenario: Off mode omits ranking reasons
- **WHEN** `ranking_explain_level` resolves to `off`
- **THEN** response metadata MUST NOT include `ranking_reasons`

#### Scenario: Basic mode emits compact factors
- **WHEN** `ranking_explain_level` resolves to `basic`
- **THEN** response metadata MUST include `ranking_reasons` with compact normalized fields:
  `result_index`, `exact_match`, `path_boost`, `definition_boost`, `semantic_similarity`, `final_score`

#### Scenario: Full mode emits complete factors
- **WHEN** `ranking_explain_level` resolves to `full`
- **THEN** response metadata MUST include per-result full scoring fields:
  `result_index`, `exact_match_boost`, `qualified_name_boost`, `path_affinity`,
  `definition_boost`, `kind_match`, `bm25_score`, `final_score`

### Requirement: Compact response shaping is serialization-only
`compact` MUST be applied at serialization stage after retrieval and ranking.
`compact` MUST preserve identity/location/score fields and MUST drop large optional payload fields.

#### Scenario: Compact preserves ranking order and identifiers
- **WHEN** a query is executed with `compact: true`
- **THEN** result ordering and stable identifiers MUST match non-compact output for the same query

#### Scenario: Compact suppresses heavy optional fields
- **WHEN** a query is executed with `compact: true`
- **THEN** large optional fields such as body previews MUST be omitted from serialized results

### Requirement: Near-duplicate suppression with explicit metadata
Query response assembly for `search_code` and `locate_symbol` MUST deduplicate
near-identical hits by symbol/file-region identity before final output and MUST
expose suppression count via `suppressed_duplicate_count`.

#### Scenario: Duplicate-heavy result set is deduplicated
- **WHEN** retrieval returns repeated hits for the same symbol/file region
- **THEN** final emitted results MUST keep only one representative per identity key

#### Scenario: Suppression count is observable
- **WHEN** one or more duplicates are removed
- **THEN** metadata MUST include `suppressed_duplicate_count` greater than zero

### Requirement: Hard payload safety limits use graceful truncation
Query tools MUST enforce a hard payload safety limit and MUST degrade
gracefully instead of failing.

When the safety limit is hit:

- runtime MUST emit deterministic prefix results
- `result_completeness` MUST be `truncated`
- `safety_limit_applied` MUST be `true` (field in `metadata`, omitted when `false`)
- `suggested_next_actions` MUST provide deterministic follow-up guidance

#### Scenario: Safety limit triggers truncation contract
- **WHEN** serialized response size exceeds the configured hard limit
- **THEN** response metadata MUST set `result_completeness: "truncated"` and `safety_limit_applied: true`

#### Scenario: Safety limit does not hard-fail requests
- **WHEN** hard payload limit is reached
- **THEN** the tool MUST return a valid response with deterministic `suggested_next_actions`

### Requirement: Protocol error registry conformance is transport-consistent
All MCP tool failures MUST emit the canonical error envelope and canonical error
codes defined by `specs/meta/protocol-error-codes.md`.

For equivalent failure conditions, stdio and HTTP transports MUST emit the same:
- `error.code`
- error semantics (same failure class and remediation meaning)

Transport-specific wrappers MAY differ only in transport envelope details and
MUST NOT introduce new protocol-level error codes.

#### Scenario: Equivalent invalid input maps to same protocol error code
- **WHEN** the same invalid tool input is submitted via stdio and HTTP
- **THEN** both responses MUST emit the same canonical `error.code` from the registry

#### Scenario: Equivalent compatibility failure maps to same protocol error code
- **WHEN** a tool request hits index compatibility failure (`not_indexed`, `reindex_required`, or `corrupt_manifest`)
- **THEN** both transports MUST emit canonical compatibility error codes and remediation-oriented error data

### Requirement: Explainability and freshness config normalization is canonical
Runtime config loading MUST normalize explainability and freshness settings to
canonical runtime enums while preserving legacy compatibility inputs.

Normalization requirements:
- `search.ranking_explain_level` MUST resolve to `off | basic | full`
- legacy `debug.ranking_reasons=true` MUST deterministically map to `full`
- invalid config values MUST fall back to canonical defaults (not arbitrary runtime branches)

#### Scenario: Legacy debug flag resolves to canonical explainability mode
- **WHEN** config provides `debug.ranking_reasons=true` and no explicit `search.ranking_explain_level`
- **THEN** runtime explainability mode MUST resolve to canonical `full`

#### Scenario: Invalid explainability config falls back safely
- **WHEN** config contains non-canonical explainability value
- **THEN** runtime MUST normalize to canonical default behavior without crashing and without emitting non-canonical modes

### Requirement: Semantic runtime metadata must be explicit and additive
`search_code` protocol metadata MUST expose semantic runtime state as explicit additive fields, preserving backward compatibility with existing clients.

Required semantic metadata set for this change:
- `semantic_mode` (existing)
- `semantic_enabled` (existing)
- `semantic_ratio_used` (existing)
- `semantic_triggered` (existing)
- `semantic_skipped_reason` (existing)
- `semantic_fallback` (existing)
- `semantic_degraded` (**new**, additive)
- `semantic_limit_used` (**new**, additive)
- `lexical_fanout_used` (**new**, additive)
- `semantic_fanout_used` (**new**, additive)
- `semantic_budget_exhausted` (**new**, additive)

Additive compatibility rules:
- Existing fields MUST keep current semantics.
- New fields MUST be optional/omittable in serialized output when unavailable.
- Clients that ignore unknown fields MUST continue to function unchanged.

#### Scenario: Degraded semantic fallback emits normalized metadata
- **WHEN** semantic execution falls back due to backend failure
- **THEN** metadata MUST include `semantic_fallback=true` and `semantic_degraded=true`
- **AND** MUST include deterministic `semantic_skipped_reason`

#### Scenario: Budget metadata reflects effective runtime values
- **WHEN** hybrid semantic search executes with configured multipliers
- **THEN** metadata MUST include `semantic_limit_used`, `lexical_fanout_used`, and `semantic_fanout_used`
- **AND** these values MUST reflect post-floor/post-cap effective values

#### Scenario: Legacy clients remain compatible
- **WHEN** a client only reads legacy semantic metadata fields
- **THEN** the response MUST remain parseable and semantically valid without requiring new-field handling

### Requirement: Ranking signal composition and precedence
The ranking system MUST compose scoring signals additively into a single reranking boost. The boost is added to the base BM25 score from Tantivy retrieval.

Signal inventory (existing + new):

| Signal | Value | Status | Condition |
|--------|-------|--------|-----------|
| `exact_match_boost` | 5.0 | existing | Exact symbol name match (case-insensitive) |
| `qualified_name_boost` | 2.0 | existing | Query substring found in qualified_name |
| `kind_weight` | 0.5-2.0 | implemented | Tiered by symbol kind (see below) |
| `query_intent_boost` | 0.0-1.0 | implemented | Query naming convention matches kind category |
| `definition_boost` | 1.0 | existing | Result is a symbol definition (not snippet/file) |
| `path_affinity` | 1.0 | existing | Query substring found in file path |
| `test_file_penalty` | -0.5 or 0.0 | implemented | Result from a test file |

Formula:
```
total_boost = exact_match_boost + qualified_name_boost + kind_weight
            + query_intent_boost + definition_boost + path_affinity
            + test_file_penalty
result.score = bm25_score + total_boost
```

Maximum possible boost: 5.0 + 2.0 + 2.0 + 1.0 + 1.0 + 1.0 + 0.0 = **12.0** (for an exact-name class match in a non-test file with qualifying path and qualified_name).

Minimum possible boost: 0.0 + 0.0 + 0.5 + 0.0 + 0.0 + 0.0 + (-0.5) = **0.0** (variable in a test file with no name/path match).

Design note: Zoekt uses a match-time additive architecture with a ~9000-point budget where `scoreSymbol=7000` and `scoreBase=7000` are dominant. Cruxe uses a post-retrieval additive reranking architecture where `exact_match=5.0` is dominant. The new kind_weight (max 2.0) is proportionally larger relative to the budget than Zoekt's kind scoring (max 1000 in ~9000), which is intentional — cruxe has fewer signals so each carries more relative weight.

#### Scenario: Signals compose additively without interaction effects
- **WHEN** a symbol result matches on exact name, has a qualifying kind, and is from a test file
- **THEN** the total boost MUST be the arithmetic sum of all individual signal values (e.g., 5.0 + 0.0 + 2.0 + 1.0 + 1.0 + 0.0 + (-0.5) = 8.5)

#### Scenario: Zero boost signals do not penalize
- **WHEN** a result has no exact match, no qualified_name match, no path affinity, and kind `variable`
- **THEN** the total boost MUST be exactly `kind_weight(variable)` = 0.5 (the only non-zero signal), not negative

#### Scenario: Ranking explains all signal components
- **WHEN** ranking explanation is requested (via `explain_ranking`)
- **THEN** the breakdown MUST include `kind_match` with the actual computed `kind_weight + query_intent_boost` value

#### Scenario: Semantic-only results receive ranking signals after metadata enrichment
- **WHEN** a semantic-only search result (no lexical match) has `symbol_stable_id` but `kind=None`
- **THEN** the system MUST enrich the result by looking up `kind`, `name`, `qualified_name` from the Tantivy symbols index using the `symbol_stable_id`
- **AND** after enrichment, `kind_weight`, `query_intent_boost`, and `test_file_penalty` signals MUST be applied to the enriched result
- **AND** a semantic-only result that cannot be enriched (symbol not found in index) MUST receive `kind_weight=0.0` and `query_intent_boost=0.0` (no penalty, no boost)

### Requirement: Role-aware symbol filtering
Query tools that support symbol-type filtering MUST support role-aware filtering in addition to exact kind filtering.

The `role` parameter accepts: `type`, `callable`, `value`, `namespace`, `alias`.

Filter semantics per tool:
- `search_code`: supports `role` filter only (not `kind`). Role-level filtering is the appropriate abstraction for cross-language search.
- `locate_symbol`: supports both `kind` (existing) and `role` (new). When both provided, intersection semantics apply.

Deterministic behavior:
- only `kind` provided: exact-kind filtering (legacy-compatible, `locate_symbol` only)
- only `role` provided: role-based filtering (matches all kinds mapping to the role)
- both provided: intersection semantics (`locate_symbol` only)
- neither provided: no symbol-type filter

#### Scenario: Kind-only filtering remains exact
- **WHEN** `locate_symbol` is called with `kind: "struct"` only
- **THEN** the runtime MUST return only `SymbolKind::Struct` symbols, not other kinds in the `Type` role

#### Scenario: Role-only filtering matches language-specific kinds
- **WHEN** `search_code` is called with `role: "type"` across a mixed Rust+Python repository
- **THEN** the runtime MUST return Rust `Struct`, `Enum`, `Trait`, `Interface` and Python `Class` symbols — all kinds mapping to the `Type` role

#### Scenario: Combined kind and role uses intersection
- **WHEN** `locate_symbol` is called with `kind: "struct"` and `role: "type"`
- **THEN** the runtime MUST return only `Struct` symbols (intersection of kind=struct and role=type)

#### Scenario: Combined kind and role with empty intersection
- **WHEN** `locate_symbol` is called with `kind: "function"` and `role: "type"`
- **THEN** the runtime MUST return zero results (Function maps to Callable role, not Type)

#### Scenario: search_code role filter is symbol-channel only
- **WHEN** `search_code` is called with `role: "type"`
- **THEN** the runtime MUST return only symbol results that map to `Type`
- **AND** snippet/file channels MUST be omitted for that query because they do not carry `role` semantics

#### Scenario: search_code rejects kind parameter
- **WHEN** `search_code` is called with `kind: "struct"`
- **THEN** the tool MUST reject the request or ignore the `kind` parameter (only `role` is supported on `search_code`)

### Requirement: find_references surfaces unresolved reference count
The `find_references` response MUST include an `unresolved_count` field indicating the number of reference edges where the target symbol could not be resolved but the name matches. This includes both unresolved call edges and unresolved import edges (both stored as `to_symbol_id = NULL, to_name = target_name`).

The count query:
```sql
SELECT COUNT(*) FROM symbol_edges
WHERE repo = ?1 AND "ref" = ?2
  AND to_symbol_id IS NULL
  AND (
       to_name = ?3
       OR to_name LIKE ?4 ESCAPE '\'
       OR to_name LIKE ?5 ESCAPE '\'
  )
```

#### Scenario: Unresolved count is reported
- **WHEN** `find_references` for symbol `createPool` finds 3 resolved edges and 5 unresolved edges (2 unresolved calls + 3 unresolved imports) matching the target name
- **THEN** the response MUST include `"unresolved_count": 5` alongside the 3 resolved references

#### Scenario: Zero unresolved count is explicit
- **WHEN** all reference edges for a symbol are fully resolved
- **THEN** the response MUST include `"unresolved_count": 0` (not omitted, not null)

#### Scenario: Unresolved count spans edge types
- **WHEN** a symbol has 2 unresolved call edges and 1 unresolved import edge matching by name
- **THEN** `unresolved_count` MUST be 3 (sum across both edge types)

### Requirement: Tiered symbol kind scoring in ranking
The ranking system MUST apply differentiated boost scores based on symbol kind. Implemented via `kind_weight()` at `ranking.rs:5-15`.

Kind weight tiers (derived from Zoekt's `scoreSymbolKind` generic tier ordering: Class=10 > Struct=9.5 > Enum=9 > Interface=8 > Function/Method=7 > Field=5.5 > Constant=5 > Variable=4, scaled to cruxe's 0.5-2.0 additive boost range):

| Tier | Weight | Kinds | Zoekt generic factor |
|------|--------|-------|---------------------|
| High | 2.0 | class, interface, trait | 10, 8, — |
| Medium-high | 1.8 | struct, enum | 9.5, 9 |
| Medium | 1.5 | type_alias, function, method | —, 7, 7 |
| Low-medium | 1.0 | constant | 5 |
| Low-medium | 0.8 | module | — |
| Low | 0.5 | variable | 4 |

Design notes:
- Zoekt applies `factor × scoreKindMatch(100)` at match-time per line/chunk. Cruxe applies kind_weight as an additive reranking boost.
- Zoekt maintains per-language override tables for 10 languages. Cruxe uses a single generic table in Phase 1.
- Tier ordering matches Zoekt: type definitions > callables > values. The absolute values are scaled to cruxe's boost budget where `exact_match=5.0` is dominant.

#### Scenario: Type definitions rank above variables for same-name match
- **WHEN** a search for `config` matches both a class `Config` (kind_weight=2.0) and a variable `config` (kind_weight=0.5)
- **THEN** the class MUST receive a 1.5-point higher kind boost than the variable

#### Scenario: Kind weights do not override exact match
- **WHEN** a variable `UserService` has an exact name match (exact_match=5.0, kind_weight=0.5, total=5.5+) and a class `UserServiceFactory` has only a partial match (exact_match=0.0, kind_weight=2.0, total=2.0+)
- **THEN** the variable MUST still rank higher because exact_match (5.0) dominates kind_weight (2.0)

#### Scenario: Struct and enum rank between class and function
- **WHEN** search results include a class (2.0), a struct (1.8), and a function (1.5) with otherwise equal scores
- **THEN** the ranking order MUST be: class > struct > function

#### Scenario: Module receives lower weight than constant
- **WHEN** search results include a module `utils` (0.8) and a constant `UTILS` (1.0) with otherwise equal scores
- **THEN** the constant MUST rank higher than the module

#### Scenario: Unknown kind receives zero weight
- **WHEN** a symbol has a kind not in the weight table
- **THEN** the kind_weight MUST be 0.0 (no boost, no penalty)

### Requirement: Query-intent kind boost
The ranking system MUST apply an additional boost when the query's naming convention matches the symbol's kind category. This stacks additively with the base kind weight.

Intent detection rules:
- **Type query**: first character is uppercase AND no underscores (e.g., `UserService`, `Config`, `HttpClient`)
- **Callable query**: first character is lowercase OR contains underscores (e.g., `validate_token`, `getUser`, `parse_config`)
- **Ambiguous query**: does not clearly match either pattern → no intent boost

Boost values:
- Type query + type kind (class, interface, trait, struct, enum, type_alias): +1.0
- Callable query + callable kind (function, method): +0.5
- No match: +0.0

#### Scenario: Uppercase query boosts type symbols
- **WHEN** a search for `UserService` matches both a class `UserService` and a function `userService`
- **THEN** the class MUST receive +1.0 intent boost; the function MUST receive +0.0

#### Scenario: Lowercase/underscore query boosts callable symbols
- **WHEN** a search for `validate_token` matches both a function `validate_token` and a class `ValidateToken`
- **THEN** the function MUST receive +0.5 intent boost; the class MUST receive +0.0

#### Scenario: Mixed-case query with underscores treated as callable
- **WHEN** a search for `User_service` (uppercase start but contains underscore)
- **THEN** the intent detector MUST classify this as callable query (underscore presence overrides uppercase start)

#### Scenario: Single-character query receives no intent boost
- **WHEN** a search query is a single character (e.g., `A` or `x`)
- **THEN** intent detection MUST still apply based on case (e.g., `A` → type query, `x` → callable query)

#### Scenario: Intent boost stacks with kind weight
- **WHEN** a class `UserService` matches a type-intent query
- **THEN** total kind-related boost MUST be kind_weight(2.0) + intent_boost(1.0) = 3.0

### Requirement: Test file ranking penalty
The ranking system MUST apply a small negative boost to results from test files.

Test file detection patterns (applied to the full relative path, case-insensitive):
- `_test.` (Go convention: `handler_test.go`)
- `.test.` (JS convention: `handler.test.ts`)
- `.spec.` (Angular/Jest convention: `handler.spec.ts`)
- `/test/` (directory convention)
- `/tests/` (directory convention)
- `test_` (Python convention: `test_handler.py`)

Penalty value: -0.5 (additive).

Design note: Zoekt uses two mechanisms — Go-specific `factor *= 0.8` (multiplicative, 20% reduction on kind score only) and BM25-path `lowPriorityFilePenalty = /5` (80% reduction on term frequency, very aggressive). Cruxe uses a conservative additive -0.5 that applies uniformly across all languages, affecting only the reranking stage.

#### Scenario: Test file result penalized in ranking
- **WHEN** two results have equivalent base scores and kind weights, but one is from `handler_test.go` and the other from `handler.go`
- **THEN** `handler.go` MUST rank higher by exactly 0.5 points

#### Scenario: High-relevance test result still ranks well
- **WHEN** a test file result has an exact name match (5.0) and is a class (2.0), giving total boost = 5.0 + 2.0 + 1.0 + ... - 0.5 ≈ 9.5
- **THEN** the test file penalty (-0.5) MUST NOT prevent it from ranking above a non-test variable with partial match (total boost ≈ 0.5)

#### Scenario: Multiple test path patterns detected
- **WHEN** a file path is `src/tests/handler_test.go` (matches both `/tests/` and `_test.`)
- **THEN** the penalty MUST be applied once (-0.5), not multiplied per pattern match

#### Scenario: Non-test file with "test" in name not penalized
- **WHEN** a file path is `src/test_utils.go` (matches `test_` at path component level)
- **THEN** the file MUST be penalized (the pattern matches the path string)
- **BUT WHEN** a file path is `src/attestation.go` (contains "test" but matches none of the patterns)
- **THEN** the file MUST NOT be penalized

### Requirement: Search field boost weights
Symbol search queries MUST apply differentiated field boost weights to the Tantivy QueryParser rather than equal weighting across all searched fields.

Field boost values (applied at BM25 retrieval time in Tantivy QueryParser, `search.rs:976-991`):

| Field | Boost | Rationale |
|-------|-------|-----------|
| `symbol_exact` | 10.0 | Exact symbol name — highest signal. Analogous to Zoekt's `scoreSymbol=7000`. |
| `qualified_name` | 3.0 | Namespace-qualified match. Unique to cruxe (Zoekt uses ctags, no qualified names). |
| `signature` | 1.5 | Signature text contains function/method parameter types. |
| `path` | 1.0 | File path component match. |
| `content` | 0.5 | Body text match — lowest signal. Analogous to Zoekt's content without symbol overlap. |

Design note: Zoekt achieves field differentiation via two mechanisms — (1) `importantTermBoost=5×` TF multiplier for symbol/filename matches in BM25 path, and (2) `scoreSymbol=7000` vs `scoreWordMatch=500` (14:1 ratio) in classic path. Cruxe uses Tantivy's native QueryParser field boost weights, achieving a 20:1 ratio (`symbol_exact=10.0` vs `content=0.5`).

Note: snippet search fields (`content`, `path`, `imports`) and file search fields (`path`, `filename`, `content_head`) include all relevant fields (`search.rs:960,964`).

#### Scenario: Name match outranks body match
- **WHEN** a query `parse` matches symbol `parse_config` in its `symbol_exact` field (boost 10.0) and matches another symbol's `content` field where `parse` appears in the function body (boost 0.5)
- **THEN** the name-matched symbol MUST receive a 20× higher BM25 contribution from this term

#### Scenario: Qualified name provides intermediate signal
- **WHEN** a query `Config` matches `app::config::Config` in `qualified_name` (boost 3.0) and another result's `content` field (boost 0.5)
- **THEN** the qualified_name match MUST receive 6× higher BM25 contribution

#### Scenario: Field boosts compose with reranking signals
- **WHEN** Tantivy returns results with field-boosted BM25 scores
- **THEN** the reranking layer MUST apply kind_weight, exact_match, and other boost signals on top of the field-boosted BM25 base scores (the two layers are independent and additive)

