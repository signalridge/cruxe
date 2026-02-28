# artifact-build-pipeline Specification

## Purpose
TBD - created by archiving change tree-sitter-tags-migration. Update Purpose after archive.
## Requirements
### Requirement: Consolidated artifact builder
The system SHALL provide a `build_source_artifacts()` function in `cruxe-indexer::prepare` that encapsulates the full per-file processing pipeline: parse → extract symbols → build snippets → extract call edges → extract imports.

#### Scenario: Full artifact construction
- **WHEN** `build_source_artifacts` is called with source content, language, and file path
- **THEN** it SHALL return a `SourceArtifacts` struct containing symbols, snippets, call edges, raw imports, and a `parse_error` field

#### Scenario: Parse failure handling
- **WHEN** the source file fails to parse (e.g., severely malformed syntax)
- **THEN** `build_source_artifacts` SHALL return a `SourceArtifacts` with empty symbols and snippets, and populate the `parse_error` field

#### Scenario: Partial parse quality signal handling
- **WHEN** language parsing succeeds but `tree-sitter-tags` reports recoverable parse errors during tag generation
- **THEN** `build_source_artifacts` SHALL continue extraction and populate `parse_error` with a partial-extraction warning message

### Requirement: Injectable parser for testing
The system SHALL provide a `build_source_artifacts_with_parser()` variant that accepts an injectable parser function, enabling unit tests to supply mock parse results.

#### Scenario: Test with mock parser
- **WHEN** `build_source_artifacts_with_parser` is called with a custom parser function
- **THEN** it SHALL use the provided parser instead of `parser::parse_file()`

### Requirement: Standardized file record construction
The system SHALL provide a `build_file_record()` function that constructs a `FileRecord` from file path, content hash, language, and metadata in a consistent format.

#### Scenario: File record fields populated
- **WHEN** `build_file_record` is called with file path, content, and language
- **THEN** the resulting `FileRecord` SHALL contain correct `path`, `content_hash`, `language`, and `size_bytes`

### Requirement: Caller deduplication
Both `index.rs` (full reindex) and `sync_incremental.rs` (incremental sync) SHALL use `build_source_artifacts` or `build_source_artifacts_with_parser` for their per-file processing, eliminating duplicated orchestration logic.

#### Scenario: Full reindex uses prepare module
- **WHEN** the `index` command processes source files
- **THEN** it SHALL delegate per-file artifact construction to `prepare::build_source_artifacts()`

#### Scenario: Incremental sync uses prepare module
- **WHEN** the incremental sync pipeline processes changed files
- **THEN** it SHALL delegate per-file artifact construction to `prepare::build_source_artifacts_with_parser()`

### Requirement: VCS ref resolution helpers
The system SHALL provide `detect_default_ref(repo_root, fallback)` and `resolve_effective_ref(repo_root, explicit_ref, fallback)` in `cruxe-core::vcs` to standardize branch/ref resolution across CLI and MCP commands that follow the standard explicit → HEAD → fallback precedence.

#### Scenario: Explicit ref takes precedence
- **WHEN** `resolve_effective_ref` is called with `explicit_ref = Some("feature-branch")`
- **THEN** it SHALL return `"feature-branch"` regardless of HEAD or fallback

#### Scenario: HEAD detection fallback
- **WHEN** `resolve_effective_ref` is called with `explicit_ref = None` in a git repository
- **THEN** it SHALL detect the HEAD branch and return it

#### Scenario: Final fallback
- **WHEN** `resolve_effective_ref` is called with `explicit_ref = None` outside a git repository
- **THEN** it SHALL return the provided fallback value

#### Exception: MCP server query-path ref resolution
- **WHEN** the MCP server resolves a ref for query operations (search, find-references)
- **THEN** it MAY use its own `resolve_ref()` with an extended cascade (explicit → session override → HEAD → project default → `REF_LIVE`) that exceeds the standard helper's scope
- **NOTE** This is acceptable because the session-override step is MCP-specific state not available in `cruxe-core`

### Requirement: Scanner glob matching via GlobSet
The scanner SHALL use `globset::GlobSet` for file pattern matching with path separator normalization, replacing the hand-rolled `matches_simple_glob` function.

#### Scenario: Cross-platform path matching
- **WHEN** a file path with backslash separators is checked against ignore patterns
- **THEN** the scanner SHALL normalize separators to `/` before matching against the `GlobSet`

