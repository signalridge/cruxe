# language-metadata-registry Specification

## Purpose
Define one authoritative language registry for indexable languages, extension
mapping, and parser metadata usage across indexing and query subsystems.
## Requirements
### Requirement: Canonical indexable language list
The system SHALL define a single authoritative constant `INDEXABLE_SOURCE_LANGUAGES` in `cruxe-core::languages` containing the canonical set of supported languages for indexing.

#### Scenario: All consumers reference canonical list
- **WHEN** any module needs to enumerate supported languages (parser, scanner, config defaults, semantic advisor)
- **THEN** it SHALL use functions from `cruxe-core::languages` rather than hardcoded inline lists

#### Scenario: Language list contents
- **WHEN** `INDEXABLE_SOURCE_LANGUAGES` is queried
- **THEN** it SHALL contain exactly `["rust", "typescript", "python", "go"]`

### Requirement: Extension-to-language detection
The system SHALL provide a `detect_language_from_extension(ext)` function in `cruxe-core::languages` that maps file extensions to canonical language identifiers.

#### Scenario: Known extension mapping
- **WHEN** `detect_language_from_extension` is called with `"rs"`
- **THEN** it SHALL return `Some("rust")`

#### Scenario: TypeScript extensions
- **WHEN** `detect_language_from_extension` is called with `"ts"` or `"tsx"`
- **THEN** it SHALL return `Some("typescript")`

#### Scenario: JavaScript extensions
- **WHEN** `detect_language_from_extension` is called with `"js"` or `"jsx"`
- **THEN** it SHALL return `Some("javascript")`
- **RATIONALE** JavaScript files are classified as `"javascript"` for metadata/reporting and semantic heuristics, while indexable-language scope remains limited to the canonical four (`rust`, `typescript`, `python`, `go`).

#### Scenario: Unknown extension
- **WHEN** `detect_language_from_extension` is called with an unrecognized extension
- **THEN** it SHALL return `None`

### Requirement: Semantic code language classification
The system SHALL provide an `is_semantic_code_language(lang)` function that returns `true` for all indexable source languages plus `"javascript"` to support mixed-repo heuristics.

#### Scenario: JavaScript classified as semantic code
- **WHEN** `is_semantic_code_language("javascript")` is called
- **THEN** it SHALL return `true`

#### Scenario: Non-code language rejected
- **WHEN** `is_semantic_code_language("markdown")` is called
- **THEN** it SHALL return `false`
