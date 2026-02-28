## 1. Dependencies and Foundation

- [x] 1.1 Add `tree-sitter-tags = "0.24"` to `cruxe-indexer/Cargo.toml`
- [x] 1.2 Add `globset = "0.4"` to workspace `Cargo.toml` and `cruxe-indexer/Cargo.toml`
- [x] 1.3 Verify workspace builds with new dependencies (`cargo check --workspace`)

## 2. Language Metadata Registry (cruxe-core)

- [x] 2.1 Create `cruxe-core/src/languages.rs` with `INDEXABLE_SOURCE_LANGUAGES` constant, `is_indexable_source_language()`, `supported_indexable_languages()`, `is_semantic_code_language()`, and `detect_language_from_extension()`
- [x] 2.2 Re-export `languages` module from `cruxe-core/src/lib.rs`
- [x] 2.3 Update `cruxe-core/src/config.rs` — `default_languages()` delegates to `languages::supported_indexable_languages()`
- [x] 2.4 Update `cruxe-indexer/src/parser.rs` — `is_language_supported()` and `supported_languages()` delegate to `cruxe_core::languages`
- [x] 2.5 Update `cruxe-indexer/src/scanner.rs` — `detect_language()` delegates to `cruxe_core::languages::detect_language_from_extension()`
- [x] 2.6 Update `cruxe-query/src/semantic_advisor.rs` — use `languages::is_semantic_code_language()` instead of inline `matches!()`

## 3. VCS Ref Resolution Helpers (cruxe-core)

- [x] 3.1 Add `detect_default_ref(repo_root, fallback)` and `resolve_effective_ref(repo_root, explicit_ref, fallback)` to `cruxe-core/src/vcs.rs`
- [x] 3.2 Update `cruxe-cli/src/commands/index.rs` to use `vcs::resolve_effective_ref()`
- [x] 3.3 Update `cruxe-cli/src/commands/init.rs` to use `vcs::is_git_repo()` and `vcs::detect_default_ref()`
- [x] 3.4 Update `cruxe-cli/src/commands/search.rs` to use `vcs::resolve_effective_ref()`
- [x] 3.5 Update `cruxe-cli/src/commands/state_import.rs` to use `vcs::detect_default_ref()`
- [x] 3.6 Update `cruxe-mcp/src/server.rs` to use `vcs::is_git_repo()` and `vcs::detect_default_ref()`
- [x] 3.7 Update `cruxe-query/src/find_references.rs` to use `vcs::is_git_repo()`

## 4. Tag Registry and Enricher Infrastructure (historical; later superseded by generic mapper refactor)

- [x] 4.1 Create `cruxe-indexer/src/languages/enricher.rs` — define `LanguageEnricher` trait with `language()`, `separator()`, `map_kind()`, `extract_visibility()`, `find_parent_scope()`; add `node_text()` helper
- [x] 4.2 Create `cruxe-indexer/src/languages/enricher_rust.rs` — implement legacy `RustEnricher` with struct/enum/trait kind disambiguation, `impl_item` parent walking, `visibility_modifier` detection
- [x] 4.3 Create `cruxe-indexer/src/languages/enricher_typescript.rs` — implement `TypeScriptEnricher` with class/enum/type discrimination, const detection, export/accessibility visibility
- [x] 4.4 Create `cruxe-indexer/src/languages/enricher_python.rs` — implement `PythonEnricher` with function/method distinction, class parent walking, underscore visibility
- [x] 4.5 Create `cruxe-indexer/src/languages/enricher_go.rs` — implement `GoEnricher` with struct/interface type disambiguation, receiver-based parent scope, capitalization visibility
- [x] 4.6 Create `cruxe-indexer/src/languages/tag_registry.rs` — thread-local `TagsConfiguration` map and `TagsContext`, `with_tags()` closure API, custom query additions for Rust (`const_item`, `static_item`) and TypeScript (declarations)
- [x] 4.7 Create `cruxe-indexer/src/languages/tag_extract.rs` — `extract_symbols_via_tags()` function: generate tags, filter to definitions, use caller-provided tree for enrichment, map to `ExtractedSymbol` via enricher
- [x] 4.8 Register new modules in `cruxe-indexer/src/languages/mod.rs`

## 5. Rewire Symbol Extraction Pipeline

- [x] 5.1 Update `cruxe-indexer/src/languages/mod.rs` — rewrite `extract_symbols()` to dispatch through enricher + tags pipeline instead of per-language `extract()` functions
- [x] 5.2 Remove `pub fn extract()` and all its helper functions from `cruxe-indexer/src/languages/rust.rs` (retain `extract_call_sites`, `extract_imports`, and their helpers)
- [x] 5.3 Remove `pub fn extract()` and helpers from `cruxe-indexer/src/languages/typescript.rs`
- [x] 5.4 Remove `pub fn extract()` and helpers from `cruxe-indexer/src/languages/python.rs`
- [x] 5.5 Remove `pub fn extract()` and helpers from `cruxe-indexer/src/languages/go.rs`

## 6. Artifact Build Pipeline Consolidation

- [x] 6.1 Create `cruxe-indexer/src/prepare.rs` — `SourceArtifacts` struct, `ArtifactBuildInput` struct, `build_source_artifacts()`, `build_source_artifacts_with_parser()`, `build_file_record()`
- [x] 6.2 Register `prepare` module in `cruxe-indexer/src/lib.rs`
- [x] 6.3 Update `cruxe-cli/src/commands/index.rs` — replace inline artifact orchestration with `prepare::build_source_artifacts()`
- [x] 6.4 Update `cruxe-indexer/src/sync_incremental.rs` — replace inline artifact orchestration with `prepare::build_source_artifacts_with_parser()`

## 7. Scanner Improvements

- [x] 7.1 Replace `matches_simple_glob()` in `scanner.rs` with `globset::GlobSet` behind `OnceLock`
- [x] 7.2 Add path separator normalization (`\` → `/`) before pattern matching

## 8. Verification

- [x] 8.1 `cargo build --workspace` passes without errors
- [x] 8.2 `cargo test --workspace` passes — all existing tests green
- [x] 8.3 `cargo clippy --workspace` passes without warnings
- [x] 8.4 Manual smoke test: `cruxe index` on a test repository, then `cruxe search` to verify symbol results match expected output
- [x] 8.5 Contract coherence check: OpenSpec language mapping docs match canonical behavior (`ts/tsx -> typescript`, `js/jsx -> javascript`)
- [x] 8.6 Contract coherence check: OpenSpec `artifact-build-pipeline` fields match canonical structs (`SourceArtifacts.parse_error`, `FileRecord.path`, `FileRecord.size_bytes`)
- [x] 8.7 Symbol parity spot-check: validate kind/visibility/qualified-name extraction on representative Rust/TypeScript/Python/Go fixtures after migration
- [x] 8.8 Go parity regression check: verify top-level `const`/`var` declarations are emitted as `Constant`/`Variable` symbols in the tag pipeline
- [x] 8.9 TypeScript compatibility check: verify legacy `var` declarations are emitted as `Variable` symbols
- [x] 8.10 Signature compatibility check: verify only callable symbols emit `signature` values
- [x] 8.11 Hygiene check: deduplicate language-local `node_text()` helpers used by call/import extraction
- [x] 8.12 Grammar registry check: parser and tag registry share a single language grammar source
- [x] 8.13 Quality-signal check: propagate `tree-sitter-tags` parse-error signal into artifact `parse_error` warnings
- [x] 8.14 TypeScript scope parity check: verify namespace-contained declarations receive namespace parent scope
