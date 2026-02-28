## ADDED Requirements

### Requirement: Unified tag-based symbol extraction
The system SHALL extract symbols from source files using `tree-sitter-tags` `TagsContext::generate_tags()` with grammar-provided `tags.scm` queries as the primary extraction mechanism, replacing per-language recursive AST walkers.

#### Scenario: Extract symbols from a Rust file
- **WHEN** a Rust source file containing a `pub struct`, a `pub fn`, and an `impl` block with methods is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries for each symbol with correct `name`, `qualified_name`, `kind` (Struct, Function, Method), `visibility`, `signature`, `line_start`, and `line_end`

#### Scenario: Rust generic impl parent scope normalization
- **WHEN** a Rust method is defined in `impl<T> Foo<T>`
- **THEN** the parent scope used for `qualified_name` SHALL normalize generic arguments (`Foo<T>` -> `Foo`) to preserve stable name matching

#### Scenario: Extract symbols from a TypeScript file
- **WHEN** a TypeScript source file containing an exported class, an interface, an enum, and module-level functions is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with correct kind discrimination (`Class`, `Interface`, `Enum`, `Function`) and visibility reflecting export status

#### Scenario: TypeScript namespace parent scope detection
- **WHEN** a TypeScript function is declared inside a `namespace` block
- **THEN** parent scope extraction SHALL include the namespace name for `qualified_name` construction

#### Scenario: Extract symbols from a Python file
- **WHEN** a Python source file containing top-level functions, classes with methods, and module-level assignments is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with method parent scopes derived from class ancestry and visibility derived from underscore naming convention

#### Scenario: Python dunder methods are treated as public
- **WHEN** a Python symbol name follows double-underscore protocol naming (e.g., `__init__`, `__str__`)
- **THEN** visibility extraction SHALL classify the symbol as public instead of private

#### Scenario: Extract symbols from a Go file
- **WHEN** a Go source file containing package-level functions, methods with receivers, struct types, and interface types is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with parent scope derived from receiver type and visibility derived from capitalization convention

#### Scenario: Go generic receiver parent scope normalization
- **WHEN** a Go method receiver type includes generic arguments (e.g., `*Foo[T]`)
- **THEN** the parent scope used for `qualified_name` SHALL normalize generic arguments (`Foo[T]` -> `Foo`)

#### Scenario: Visibility normalization for default-private declarations
- **WHEN** a supported language declaration has no explicit visibility modifier (e.g., Rust `fn` in an `impl`, TypeScript non-exported declaration)
- **THEN** the enricher SHALL emit a normalized non-public visibility value instead of `None`, so downstream consumers do not need to interpret missing visibility as a separate state

#### Scenario: Signature is emitted for callable symbols only
- **WHEN** symbols are extracted from a source file
- **THEN** `signature` SHALL be populated for callable kinds (functions/methods) and omitted for non-callable kinds (e.g., struct/class/type declarations)

#### Scenario: Unsupported language produces no symbols
- **WHEN** a source file with an unrecognized language identifier is processed
- **THEN** the extraction pipeline SHALL return an empty symbol list without errors

### Requirement: Per-language enricher trait
The system SHALL define a `LanguageEnricher` trait with methods `language`, `map_kind`, `extract_visibility`, `find_parent_scope`, and `separator` that each supported language MUST implement to supplement tag-provided metadata.

#### Scenario: Kind disambiguation via enricher
- **WHEN** a tag with kind `"class"` is produced from a Rust file where the underlying AST node is `enum_item`
- **THEN** the enricher's `map_kind` SHALL return `SymbolKind::Enum` rather than `SymbolKind::Class`

#### Scenario: Enricher returns None for unrecognized tag kind
- **WHEN** a tag with an unrecognized kind string is processed
- **THEN** the enricher's `map_kind` SHALL return `None` and the tag SHALL be filtered from output

### Requirement: Tag registry with thread-local storage
The system SHALL maintain `TagsConfiguration` instances per language and `TagsContext` in thread-local storage, accessible via a `with_tags(|configs, ctx| ...)` closure API.

#### Scenario: Concurrent thread safety
- **WHEN** multiple threads invoke the tag extraction pipeline concurrently
- **THEN** each thread SHALL use its own thread-local `TagsConfiguration` and `TagsContext` instances without data races

#### Scenario: Lazy initialization
- **WHEN** the tag registry is first accessed on a thread
- **THEN** `TagsConfiguration` objects for all supported languages SHALL be initialized from grammar `TAGS_QUERY` constants plus any custom query additions

### Requirement: Custom query additions per language
The tag registry SHALL support appending custom tree-sitter query patterns to a language's built-in `TAGS_QUERY` to cover constructs not captured by upstream queries.

#### Scenario: Rust custom queries for const and static items
- **WHEN** a Rust file containing `const` and `static` declarations is indexed
- **THEN** these symbols SHALL be captured via custom query additions appended to the Rust `TAGS_QUERY`, with `const` mapped to `Constant` and `static` mapped to a non-constant symbol kind

#### Scenario: Go custom queries for const and var items
- **WHEN** a Go file containing top-level `const` and `var` declarations is indexed
- **THEN** these symbols SHALL be captured via custom query additions appended to the Go `TAGS_QUERY`, mapped to `Constant` and `Variable` kinds respectively

#### Scenario: TypeScript custom queries for declarations
- **WHEN** a TypeScript file containing function declarations, class declarations, interface declarations, enum declarations, type alias declarations, const/variable declarations, and method definitions is indexed
- **THEN** symbol extraction SHALL be covered by upstream TypeScript `TAGS_QUERY` plus project custom query additions, with custom additions at minimum covering `function_declaration`, `class_declaration`, `enum_declaration`, `type_alias_declaration`, `lexical_declaration` (const/let), `variable_declaration` (legacy `var`), and `method_definition`
- **NOTE** `interface_declaration` coverage MAY come from upstream `TAGS_QUERY` or project custom additions, but the extraction output SHALL include interface symbols

### Requirement: Preserved call-site and import extraction
Per-language call-site extraction (`extract_call_sites`) and import extraction (`extract_imports`) SHALL remain in their respective language modules, unchanged by the tags migration.

#### Scenario: Call sites extracted alongside tag-based symbols
- **WHEN** a source file is processed through the full artifact pipeline
- **THEN** call edges SHALL be extracted by the existing per-language `extract_call_sites` function, independent of the tags-based symbol extraction
