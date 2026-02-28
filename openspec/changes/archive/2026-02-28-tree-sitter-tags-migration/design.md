## Context

Cruxe is a Rust-powered code search and navigation tool that uses tree-sitter for parsing source code in four languages (Rust, TypeScript, Python, Go). The current symbol extraction pipeline uses per-language recursive AST walkers that each independently traverse tree-sitter parse trees and construct `ExtractedSymbol` values. This approach has led to ~600+ lines of duplicated extraction logic, inconsistent handling of language features, and fragile coupling to tree-sitter grammar internals.

The tree-sitter ecosystem provides a `tree-sitter-tags` crate (v0.24) that leverages `tags.scm` query files shipped with each grammar. These queries define `@definition.*` and `@reference.*` captures that identify symbols declaratively rather than through procedural AST walking. Grammar authors maintain these queries alongside the grammar itself, meaning improvements and new syntax coverage arrive automatically with grammar updates.

The crate's `TagsContext` and `TagsConfiguration` types do not implement `Send`/`Sync`, which constrains how they can be used in async/threaded contexts. Additionally, tags alone cannot provide all the metadata Cruxe needs (symbol kind disambiguation, visibility, qualified names with parent scopes), requiring a supplementary enrichment layer.

Beyond symbol extraction, the codebase has accumulated duplicated patterns in per-file artifact orchestration (across `index.rs` and `sync_incremental.rs`), language metadata (scattered across four modules), and VCS ref resolution (five call sites with slight variations).

## Goals / Non-Goals

**Goals:**
- Replace per-language recursive AST walkers with a unified `tree-sitter-tags` extraction pipeline
- Preserve the externally consumed `ExtractedSymbol` semantic fields: name, qualified_name, kind, visibility, signature, and line range
- Maintain call-site extraction and import extraction unchanged (these remain in per-language modules)
- Centralize language metadata into a single authoritative module
- Consolidate per-file artifact build orchestration into a testable, injectable pipeline
- Standardize VCS ref resolution helpers across all CLI and MCP commands
- Define explicit contract alignment for extension mapping and `FileRecord` field names in OpenSpec docs
- Add parity-oriented verification constraints for symbol extraction migration quality
- Zero breaking changes to public APIs, MCP tool contracts, or index schema

**Non-Goals:**
- Adding new languages (the migration covers the same four: Rust, TypeScript, Python, Go)
- Migrating call-site extraction or import extraction to tags (these use different tree-sitter patterns not well-served by `tags.scm`)
- Changing the Tantivy index schema or SQLite schema
- Performance optimization (correctness and maintainability are the primary drivers)
- Migrating to tree-sitter's `@reference.*` captures for find-references (current AST-based approach remains)

## Decisions

### D1: Use `tree-sitter-tags` crate as the extraction backend

**Choice**: Replace all per-language `extract()` functions with `tree-sitter-tags` 0.24 `TagsContext::generate_tags()`.

**Rationale**: The tags crate provides a declarative, query-based approach to symbol discovery that leverages upstream-maintained `tags.scm` files. This eliminates ~600 lines of hand-rolled AST walking and automatically benefits from grammar updates. The alternative — continuing to maintain per-language walkers — scales poorly as languages are added and requires deep grammar knowledge for each update.

**Alternative considered**: Using raw tree-sitter queries (`Query::new`) directly without the tags crate. This would give more control but would require reimplementing the tag generation logic that `tree-sitter-tags` already provides, including pattern deduplication and UTF-8 range handling.

### D2: Thread-local storage for TagsConfiguration and TagsContext

**Choice**: Store `TagsConfiguration` map and `TagsContext` in `thread_local!` with `RefCell`, exposed via a `with_tags(|configs, ctx| ...)` closure API.

**Rationale**: `TagsConfiguration` in tree-sitter-tags 0.24 does not implement `Send` or `Sync` (it holds raw pointers internally). Thread-local storage avoids `unsafe` wrappers while supporting both the synchronous indexing path and tokio-based MCP server (each tokio worker thread gets its own instance). The accessor closure pattern ensures borrows are scoped and prevents accidental leaks of non-`Send` references.

**Alternative considered**: Wrapping in `Mutex<Option<...>>` behind a global `OnceLock`. This would work but adds contention on a shared mutex for a resource that has no cross-thread sharing requirement. Thread-local is simpler and lock-free.

### D3: LanguageEnricher trait for per-language metadata

**Choice**: Define a `LanguageEnricher` trait with five methods: `language`, `map_kind`, `extract_visibility`, `find_parent_scope`, and `separator`.

**Rationale**: Tags provide symbol name, range, and a coarse kind string (e.g., `"class"`, `"function"`). But Cruxe needs finer-grained kinds (e.g., distinguishing Rust `struct` vs `enum` vs `type_alias` — all tagged as `@definition.class`), language-specific visibility rules (Rust's `pub`, Go's capitalization, Python's underscore convention), and qualified name construction with parent scope resolution. The enricher trait isolates these three concerns into focused, per-language implementations (70-110 lines each) while the generic extraction loop handles the common path.

**Alternative considered**: Embedding all language logic directly in `tag_extract.rs` via match arms. This would work for four languages but becomes unwieldy at scale and mixes concerns that evolve independently.

### D4: Custom tags.scm query additions per language

**Choice**: Append custom query patterns to the built-in `TAGS_QUERY` for languages where upstream coverage is insufficient (Rust: `const_item`/`static_item`; TypeScript: most declarations since upstream targets `.d.ts` files).

**Rationale**: Upstream `tags.scm` files are maintained for general use and may not cover all constructs Cruxe needs. Rather than forking the queries entirely, appending targeted additions preserves upstream improvements while filling gaps. The registry clearly separates base queries from custom additions.

**Alternative considered**: Forking complete `tags.scm` files into the repository. This provides full control but creates a maintenance burden to keep forked queries in sync with grammar updates.

### D5: Consolidated prepare module for artifact orchestration

**Choice**: Create `cruxe-indexer::prepare` with `build_source_artifacts()` and `build_file_record()` that encapsulate the full per-file pipeline: parse → extract symbols → build snippets → extract call edges → extract imports.

**Rationale**: This sequence was duplicated between `index.rs` (full reindex) and `sync_incremental.rs` (incremental sync) with slight variations. Consolidation ensures both paths produce identical artifacts and provides a `build_source_artifacts_with_parser()` variant for injecting test parsers. The `SourceArtifacts` struct bundles all per-file outputs as a single return value.

**Alternative considered**: Keeping the orchestration inline in each caller. Acceptable for two callers, but any future caller (e.g., a watch-mode reindex) would need to duplicate again.

### D6: Centralized language metadata in cruxe-core

**Choice**: Create `cruxe-core::languages` with canonical arrays and lookup functions (`is_indexable_source_language`, `detect_language_from_extension`, etc.).

**Rationale**: Language lists and extension mappings were hardcoded in four separate locations (`scanner.rs`, `parser.rs`, `config.rs`, `semantic_advisor.rs`) with drift risk. A single module in the core crate ensures consistency and provides a natural extension point for adding languages.

**Known limitation**: Three dispatch points remain that must be updated in parallel when adding a language: `parser.rs::get_language()` (maps string → tree-sitter grammar), `tag_registry.rs::build_configs()` (maps string → `TagsConfiguration`), and `languages/mod.rs::extract_symbols()` (maps string → enricher). These cannot be unified because each maps to a different type. The centralized `INDEXABLE_SOURCE_LANGUAGES` constant serves as the canonical check, but implementors must update all three dispatch sites.

### D7: GlobSet for scanner pattern matching

**Choice**: Replace `matches_simple_glob()` with `globset::GlobSet` initialized via `OnceLock`.

**Rationale**: The hand-rolled glob function only supported `*` wildcards and didn't handle path separator normalization. `globset` provides correct, battle-tested glob matching with lazy initialization. Path separators are normalized (`\` → `/`) before matching for cross-platform correctness.

### D8: Explicit core-vs-adjacent scope and parity validation

**Choice**: Treat symbol extraction migration + language/artifact contract unification as the **core scope**, and treat VCS helper + scanner glob cleanup as **adjacent scope**. Require parity-oriented validation for the core scope.

**Rationale**: This change bundles several related cleanups. Explicit scope classification keeps review and rollback decisions clear: core acceptance is tied to extraction correctness and artifact contract coherence; adjacent refactors can be independently deferred if regressions appear. Parity checks reduce the risk of silent extraction drift during migration.

**Validation constraints introduced**:
- Extension mapping contract is explicit: `ts/tsx -> typescript`, `js/jsx -> javascript`
- `FileRecord` contract fields in spec language use canonical names (`path`, `size_bytes`)
- Symbol extraction scenarios require stable semantic fields (`name`, `qualified_name`, `kind`, `visibility`, `signature`, `line_start`, `line_end`) rather than undocumented internal offsets

## Risks / Trade-offs

**[Risk: tags.scm upstream regressions]** → Grammar updates could change tag patterns in ways that affect extraction. Mitigation: the enricher layer validates tag kinds before mapping, returning `None` for unrecognized kinds which are filtered out. Integration tests against known source files will catch regressions.

**[Risk: Thread-local memory usage]** → Each thread maintains its own `TagsConfiguration` instances for all four languages. Mitigation: these are lightweight (query bytecode + grammar reference) and the thread count is bounded by tokio's worker pool (typically CPU count).

**[Risk: TypeScript tag coverage gaps]** → TypeScript's upstream `tags.scm` is minimal (targeting `.d.ts`), requiring extensive custom additions. Mitigation: the custom queries are well-scoped to declaration patterns and are tested against representative TypeScript samples.

**[Risk: Double-parse for enrichment]** → The extraction pipeline parses each file twice: once via `tree-sitter-tags` internally and once via `parser::parse_file()` for enricher AST node access. Mitigation: parse times for individual files are typically sub-millisecond; correctness and separation of concerns outweigh the duplication. A future optimization could expose the internal parse tree from the tags crate. **Follow-up**: Track as an optimization candidate; if a future `tree-sitter-tags` release exposes its internal parse tree, the enricher layer should be updated to consume it directly and eliminate the second parse.

**[Trade-off: Enricher complexity vs tags simplicity]** → The enricher layer adds ~370 lines of per-language code. This is less than the ~600 lines removed from the old walkers, but the total system (tags infra + enrichers) is architecturally more layered. The trade-off favors maintainability: the enricher code is focused on three specific concerns rather than mixing traversal, extraction, and metadata in a single recursive function.

## Verification Strategy

- **Contract coherence checks (spec-level)**:
  - `language-metadata-registry` reflects canonical extension mappings and semantic-language rules without conflicting labels
  - `artifact-build-pipeline` uses current `FileRecord` field names and `SourceArtifacts` parse-error semantics
  - `tag-based-symbol-extraction` trait and scenario fields match implementation-facing contracts
- **Behavioral parity checks (migration-level)**:
  - Index and search smoke test on representative repositories across Rust/TypeScript/Python/Go
  - Symbol output spot-checks for kind/visibility/qualified-name correctness on language fixtures
  - Adjacent refactors (VCS/glob) validated separately from core extraction parity

## Migration Plan

This is an internal refactor with no schema changes or API changes. Migration is atomic:

1. Add `tree-sitter-tags` and `globset` dependencies
2. Create new modules: `tag_registry`, `tag_extract`, `enricher*`, `prepare`, `languages`
3. Rewire `languages/mod.rs::extract_symbols()` to dispatch through the tags pipeline
4. Remove old `extract()` functions from per-language modules (retaining call-site and import extraction)
5. Update all callers to use consolidated helpers (`prepare::build_source_artifacts`, `vcs::resolve_effective_ref`, `languages::*`)
6. Verify: `cargo build --workspace`, `cargo test --workspace`, manual index + search on a test repository

**Rollback**: Revert the commit. No data migration or schema changes to undo.

## Open Questions

- **Q1**: Should the double-parse (tags + enricher) be optimized in this change, or deferred? **Recommendation**: Defer — the performance impact is negligible for the file sizes Cruxe processes, and the tags crate doesn't expose its internal parse tree.
- **Q2**: Should JavaScript be added as a fifth language given its similarity to TypeScript? **Recommendation**: Out of scope for this change; track as a follow-up if needed.
- **Q3**: Should per-language `extract_call_sites()` be consolidated in a follow-up? The four implementations share an identical structural pattern (recursive walk → parse call node → normalize target → confidence check), differing only in node kind names and minor normalization rules. After this migration, call-site extraction is the primary source of remaining per-language duplication (~160 lines each). **Recommendation**: Track as a follow-up; the differences are small but language-specific enough to warrant a separate change.
