## Why

The current symbol extraction pipeline uses hand-rolled recursive AST walkers per language (~600+ lines of duplicated logic across four modules), each reimplementing tree traversal, kind mapping, visibility detection, and parent scope resolution independently. This duplicated approach is fragile to grammar updates, inconsistent across languages, and ignores tree-sitter's built-in `tags.scm` query infrastructure that grammar authors already maintain. Consolidating onto `tree-sitter-tags` reduces maintenance surface, gains upstream query improvements automatically, and enables cleaner separation between generic extraction and language-specific enrichment.

## What Changes

### Core migration scope (must-pass acceptance)
- Replace all per-language `extract()` symbol extraction functions with a unified `tree-sitter-tags`-based pipeline
- Introduce a `LanguageEnricher` trait for language-specific concerns (kind disambiguation, visibility, parent scope) that tags alone cannot provide
- Add a tag registry with thread-local `TagsConfiguration` and `TagsContext` management, including per-language custom query additions where upstream coverage is insufficient
- Consolidate duplicated per-file artifact orchestration (parse → extract → snippet → call-edges → file-record) into a single `prepare` module
- Centralize language metadata (supported languages, extension mapping, semantic classification) into `cruxe-core::languages`, replacing scattered hardcoded lists

### Adjacent refactors in same change (non-blocking for core migration)
- Standardize VCS ref resolution helpers (`detect_default_ref`, `resolve_effective_ref`) to replace five duplicated patterns across CLI and MCP commands
- Replace hand-rolled glob matching in the scanner with `globset::GlobSet` for correctness and cross-platform path normalization
- Add `tree-sitter-tags = "0.24"` and `globset = "0.4"` as new dependencies

## Capabilities

### New Capabilities
- `tag-based-symbol-extraction`: Unified symbol extraction pipeline using `tree-sitter-tags` with per-language enricher traits for kind disambiguation, visibility detection, and parent scope resolution
- `language-metadata-registry`: Centralized language metadata (supported languages, file extensions, semantic classification) as single source of truth in `cruxe-core`
- `artifact-build-pipeline`: Consolidated per-file artifact construction (symbols, snippets, call edges, imports, file records) with injectable parser for testability

### Modified Capabilities
<!-- No existing specs govern symbol extraction mechanics, language parsing strategy, or per-file build orchestration. VCS and scanner changes are internal implementation details that don't alter spec-level behavior. -->

## Scope Boundary and Acceptance

- **Core migration acceptance** is satisfied only when:
  - symbols for Rust/TypeScript/Python/Go are produced by the tag-based pipeline with enricher disambiguation; and
  - `prepare` and `cruxe-core::languages` become the canonical paths for artifact orchestration and language metadata.
- **Adjacent refactors** (VCS helper standardization, scanner glob engine swap) are included for cleanup and consistency, but MAY be deferred or reverted independently if they introduce regressions unrelated to core symbol extraction behavior.

## Impact

- **cruxe-indexer**: Major restructure — per-language modules retain only call-site and import extraction; new modules: `tag_extract`, `tag_registry`, `enricher*`, `prepare`
- **cruxe-core**: New `languages` module re-exported from crate root; new VCS helpers in `vcs.rs`
- **cruxe-cli**: All index/init/search/state-import commands updated to use consolidated helpers
- **cruxe-mcp**: Server updated to use VCS helpers
- **cruxe-query**: `find_references` and `semantic_advisor` updated to use core language utilities
- **Dependencies**: `tree-sitter-tags 0.24` (new, core to migration), `globset 0.4` (new, scanner improvement)
- **Breaking**: None — all changes are internal implementation; public APIs and index schema unchanged
