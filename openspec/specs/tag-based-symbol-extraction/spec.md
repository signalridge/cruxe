# tag-based-symbol-extraction Specification

## Purpose
Define tree-sitter-tags based symbol extraction with a generic cross-language
mapper, precise import resolution semantics, and module-scope edge capture.
## Requirements
### Requirement: Unified tag-based symbol extraction
The system SHALL extract symbols from source files using `tree-sitter-tags` `TagsContext::generate_tags()` with grammar-provided `tags.scm` queries as the primary extraction mechanism, replacing per-language recursive AST walkers.

#### Scenario: Extract symbols from a Rust file
- **WHEN** a Rust source file containing a `pub struct`, a `pub fn`, and an `impl` block with methods is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries for each symbol with correct `name`, `qualified_name`, `kind` (Struct, Function, Method), `signature`, `line_start`, and `line_end`
- **AND** `visibility` MAY be absent/`None` when not explicitly extracted

#### Scenario: Rust generic impl parent scope normalization
- **WHEN** a Rust method is defined in `impl<T> Foo<T>`
- **THEN** the parent scope used for `qualified_name` SHALL normalize generic arguments (`Foo<T>` -> `Foo`) to preserve stable name matching

#### Scenario: Extract symbols from a TypeScript file
- **WHEN** a TypeScript source file containing an exported class, an interface, an enum, and module-level functions is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with correct kind discrimination (`Class`, `Interface`, `Enum`, `Function`) and stable parent/qualified-name derivation

#### Scenario: TypeScript namespace parent scope detection
- **WHEN** a TypeScript function is declared inside a `namespace` block
- **THEN** parent scope extraction SHALL include the namespace name for `qualified_name` construction

#### Scenario: Extract symbols from a Python file
- **WHEN** a Python source file containing top-level functions, classes with methods, and module-level assignments is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with method parent scopes derived from class ancestry

#### Scenario: Python dunder methods are indexed without visibility special-casing
- **WHEN** a Python symbol name follows double-underscore protocol naming (e.g., `__init__`, `__str__`)
- **THEN** the symbol SHALL be indexed as normal and `visibility` remains optional (`None` unless explicitly extracted)

#### Scenario: Extract symbols from a Go file
- **WHEN** a Go source file containing package-level functions, methods with receivers, struct types, and interface types is indexed
- **THEN** the extraction pipeline SHALL produce `ExtractedSymbol` entries with parent scope derived from receiver type

#### Scenario: Go generic receiver parent scope normalization
- **WHEN** a Go method receiver type includes generic arguments (e.g., `*Foo[T]`)
- **THEN** the parent scope used for `qualified_name` SHALL normalize generic arguments (`Foo[T]` -> `Foo`)

#### Scenario: Visibility remains missing when unavailable
- **WHEN** a supported language declaration has no explicit visibility modifier (e.g., Rust `fn` in an `impl`, TypeScript non-exported declaration)
- **THEN** the generic mapper SHALL emit `visibility = None` rather than inventing a placeholder value

#### Scenario: Signature is emitted for callable symbols only
- **WHEN** symbols are extracted from a source file
- **THEN** `signature` SHALL be populated for callable kinds (functions/methods) and omitted for non-callable kinds (e.g., struct/class/type declarations)

#### Scenario: Unsupported language produces no symbols
- **WHEN** a source file with an unrecognized language identifier is processed
- **THEN** the extraction pipeline SHALL return an empty symbol list without errors

### Requirement: Generic tag mapper replaces per-language enrichers
The system SHALL use a single generic tag→symbol mapper for all supported languages, replacing the per-language `LanguageEnricher` trait and its four implementations (~650 lines → ~120 lines).

The generic mapper provides:

1. **Kind mapping** — shared lookup table from tag kind strings to `SymbolKind`:
   - `has_parent` parameter enables Function→Method promotion for nested definitions.
   - `node.kind()` fallback enables struct/enum/union/type disambiguation when tag kind is ambiguous (Rust `struct_item`, `enum_item`, `union_item`, `type_item` all produce `"class"` tag kind via `@definition.class`).
   - Unrecognized tag kinds return `None` and are filtered from output.

2. **Parent scope** — generic AST parent walking:
   - `impl_item` is handled as a special case (name extracted from type, not from `is_scope_node`).
   - `is_scope_node` match set: `class_declaration`, `abstract_class_declaration`, `interface_declaration`, `class_definition`, `trait_item`, `struct_item`, `enum_item`, `mod_item`, `internal_module`, `namespace_definition`, `function_item`, `function_definition`, `function_declaration`.
   - `is_transparent_node` match set: `declaration_list`, `class_body`, `object_type`, `statement_block`, `block`, `decorated_definition`, `program`, `source_file`.
   - Name extraction via `child_by_field_name("name")` with fallback to `child_by_field_name("type")` for Go method receivers.
   - Universal generic argument stripping: `Foo<T>` → `Foo`, `Bar[T]` → `Bar` (handles nested: `Foo<Bar<T>>` → `Foo`).

3. **Separator** — language-derived: `"::"` for Rust, `"."` for all others.

4. **Signature** — first-line extraction for Function/Method kinds only. Language-agnostic, preserves existing `tag_extract.rs` behavior and `symbol_stable_id` stability (signature is an input to stable ID computation).

5. **Visibility** — not extracted, defaults to `None`. Zero search impact confirmed by audit (visibility is stored in Tantivy but never queried, filtered, or ranked across all 18 MCP tools).

#### Scenario: Kind disambiguation via node kind fallback — Rust struct
- **WHEN** a tag with kind `"class"` is produced from a Rust file where the underlying AST node is `struct_item`
- **THEN** the generic mapper SHALL return `SymbolKind::Struct` rather than `SymbolKind::Class`
- **AND** the resulting `symbol_stable_id` MUST match the value produced by the legacy Rust-specific mapper before this refactor

#### Scenario: Kind disambiguation via node kind fallback — Rust enum
- **WHEN** a tag with kind `"class"` is produced from a Rust file where the underlying AST node is `enum_item`
- **THEN** the generic mapper SHALL return `SymbolKind::Enum` rather than `SymbolKind::Class`

#### Scenario: Kind disambiguation via node kind fallback — Rust type alias
- **WHEN** a tag with kind `"class"` is produced from a Rust file where the underlying AST node is `type_item`
- **THEN** the generic mapper SHALL return `SymbolKind::TypeAlias` rather than `SymbolKind::Class`

#### Scenario: Kind disambiguation via node kind fallback — Rust union
- **WHEN** a tag with kind `"class"` is produced from a Rust file where the underlying AST node is `union_item`
- **THEN** the generic mapper SHALL return `SymbolKind::Struct` rather than `SymbolKind::Class`

#### Scenario: Kind disambiguation passes through when node kind is not special
- **WHEN** a tag with kind `"class"` is produced and the AST node kind is `class_declaration` (TypeScript/Python)
- **THEN** the generic mapper SHALL return `SymbolKind::Class` (no fallback override needed)

#### Scenario: Function-to-Method promotion for nested definitions
- **WHEN** a tag with kind `"function"` is produced and the symbol has a parent scope (e.g., inside a Python class or Rust impl block)
- **THEN** the generic mapper SHALL return `SymbolKind::Method` rather than `SymbolKind::Function`

#### Scenario: Function remains Function when top-level
- **WHEN** a tag with kind `"function"` is produced and the symbol has no parent scope (module-level definition)
- **THEN** the generic mapper SHALL return `SymbolKind::Function` (no promotion)

#### Scenario: Method tag kind bypasses promotion logic
- **WHEN** a tag with kind `"method"` is produced (already explicitly a method)
- **THEN** the generic mapper SHALL return `SymbolKind::Method` regardless of `has_parent` value

#### Scenario: Generic mapper returns None for unrecognized tag kind
- **WHEN** a tag with an unrecognized kind string (e.g., `"parameter"`, `"import"`) is processed
- **THEN** the generic mapper SHALL return `None` and the tag SHALL be filtered from output

#### Scenario: Parent scope extracted via generic AST walking — Rust impl
- **WHEN** a method `fn handle(&self)` is defined inside `impl Server`
- **THEN** the generic mapper SHALL walk up from the method node, match `impl_item` as a special case, extract name `Server` from its type field, and construct `qualified_name = "Server::handle"`

#### Scenario: Parent scope extracted — Python class
- **WHEN** a method `def validate(self)` is defined inside `class UserService`
- **THEN** the generic mapper SHALL walk up, skip `block` (transparent node), match `class_definition` as a scope node, extract name `UserService`, and construct `qualified_name = "UserService.validate"`

#### Scenario: Parent scope extracted — TypeScript class
- **WHEN** a method `async fetchData()` is defined inside `class ApiClient`
- **THEN** the generic mapper SHALL walk up, skip `class_body` (transparent node), match `class_declaration` as a scope node, and construct `qualified_name = "ApiClient.fetchData"`

#### Scenario: Transparent nodes are skipped during walking
- **WHEN** a method is inside a class body (`class_body`, `declaration_list`, or `block`)
- **THEN** the walker SHALL skip these transparent nodes and continue to the enclosing scope node

#### Scenario: Walking stops at non-scope, non-transparent nodes
- **WHEN** the walker encounters a node kind that is neither a scope node nor a transparent node
- **THEN** the walker SHALL return `None` (no parent scope found)

#### Scenario: Generic arguments stripped from parent scope names — Rust
- **WHEN** a parent scope name is `Foo<T, U>` from a Rust `impl_item`
- **THEN** the generic mapper SHALL strip the generic arguments, producing `Foo`

#### Scenario: Generic arguments stripped — Go type parameters
- **WHEN** a parent scope name is `Container[T]` from a Go type declaration
- **THEN** the generic mapper SHALL strip the type parameters, producing `Container`

#### Scenario: Nested generic arguments stripped correctly
- **WHEN** a parent scope name is `Foo<Bar<T>>` (nested generics)
- **THEN** the generic mapper SHALL strip all generic arguments using depth tracking, producing `Foo`

#### Scenario: Go method receiver extracted as parent scope
- **WHEN** a Go method `func (s *Server) Handle()` is defined with a receiver
- **THEN** the generic mapper SHALL use `child_by_field_name("type")` on the `method_declaration` node to extract `Server` as the parent scope, and construct `qualified_name = "Server.Handle"`

#### Scenario: Go pointer receiver stripped
- **WHEN** a Go method receiver type is `*Server` (pointer receiver)
- **THEN** the generic mapper SHALL extract `Server` (without the `*` prefix)

#### Scenario: Visibility defaults to None for all languages
- **WHEN** a symbol is extracted by the generic mapper from any supported language
- **THEN** `visibility` SHALL be `None` (not a fake default like `"private"` or `"internal"`)

#### Scenario: Signature extracted for callable symbols only
- **WHEN** a Function or Method symbol is extracted
- **THEN** `signature` SHALL contain the first line of the definition text, trimmed (e.g., `pub fn handle(&self, req: Request) -> Response`)

#### Scenario: Signature not extracted for non-callable symbols
- **WHEN** a Struct, Class, Enum, or other non-callable symbol is extracted
- **THEN** `signature` SHALL be `None`

#### Scenario: Separator is language-specific
- **WHEN** a Rust symbol has parent scope `Server` and name `handle`
- **THEN** `qualified_name` SHALL be `Server::handle` (Rust separator `::`)
- **AND WHEN** a Python symbol has parent scope `Server` and name `handle`
- **THEN** `qualified_name` SHALL be `Server.handle` (Python separator `.`)

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

### Requirement: Unresolved imports use NULL representation
The import extraction pipeline SHALL store unresolved imports with `to_symbol_id = NULL` and `to_name` set to the import name. The legacy `unresolved_symbol_stable_id()` blake3 hash approach has been removed.

This unifies the unresolved representation with call edges, which also use `to_symbol_id = NULL, to_name = Some(callee_name)`.

#### Scenario: Unresolved import stored with NULL target
- **WHEN** an import target (e.g., `import { Foo } from 'unknown-module'`) cannot be resolved to any indexed symbol
- **THEN** the import extractor SHALL store the edge with `to_symbol_id = NULL` and `to_name = "Foo"`
- **AND** SHALL NOT generate a synthetic hash ID as the target

#### Scenario: Resolved import still uses actual symbol ID
- **WHEN** an import target can be resolved to an indexed symbol with known `symbol_stable_id`
- **THEN** the import extractor SHALL store `to_symbol_id = <resolved_id>` (existing behavior unchanged)

#### Scenario: Unified unresolved representation across edge types
- **WHEN** both import edges and call edges have unresolved targets
- **THEN** both MUST use `to_symbol_id = NULL, to_name = target_name` representation
- **AND** a single SQL query on `to_symbol_id IS NULL` MUST find both types of unresolved edges

#### Scenario: Reindex produces clean NULL representation
- **WHEN** the index is rebuilt
- **THEN** all unresolved imports SHALL use NULL representation (no legacy hash IDs)

### Requirement: Import path resolution for relative paths
The import extraction pipeline SHALL resolve relative import paths to actual indexed file paths before performing symbol lookup. This constrains the symbol search to the resolved file (more precise than the current qualified_name → name fallback).

Resolution rules per language:

| Language | Relative pattern | Resolution strategy | File existence check |
|----------|-----------------|--------------------|--------------------|
| TypeScript | `./foo`, `../foo` | Join with importing file's directory | Try `.ts`, `.tsx`, `/index.ts` |
| Rust | `super::module`, `self::module` | Walk module tree (`super` = parent dir) | Try `module.rs`, `module/mod.rs` |
| Python | `.utils`, `..utils` | Relative to `__init__.py` package directory | Try `utils.py`, `utils/__init__.py` |
| Go | N/A | Go uses absolute module paths | Skip in Phase 1 |

After resolving to a file path:
1. Check `file_manifest` to confirm the file is indexed.
2. Query `symbol_relations` constrained to the resolved file: `WHERE path = ?3 AND name = ?4`.

#### Scenario: TypeScript relative import resolves to .ts file
- **WHEN** `src/api/handler.ts` imports `import { Foo } from './services/user'`
- **THEN** the import extractor SHALL resolve `./services/user` to `src/api/services/user.ts`, confirm it exists in `file_manifest`, and look up symbol `Foo` in that file's indexed symbols

#### Scenario: TypeScript relative import resolves to index.ts
- **WHEN** `src/api/handler.ts` imports `import { Bar } from './services'`
- **THEN** the import extractor SHALL try `src/api/services.ts`, `src/api/services.tsx`, then `src/api/services/index.ts`, and use the first match found in `file_manifest`

#### Scenario: TypeScript parent directory import
- **WHEN** `src/api/handlers/user.ts` imports `import { Config } from '../config'`
- **THEN** the import extractor SHALL resolve `../config` to `src/api/config.ts` (or `.tsx`, or `/index.ts`)

#### Scenario: Rust super relative use resolves
- **WHEN** `src/api/handlers.rs` contains `use super::models::User`
- **THEN** the import extractor SHALL resolve `super::` to `src/` (parent module), then try `src/models.rs` and `src/models/mod.rs`, and match `User` against the target module's indexed symbols

#### Scenario: Rust self relative use resolves
- **WHEN** `src/api/mod.rs` contains `use self::handlers::handle`
- **THEN** the import extractor SHALL resolve `self::` to `src/api/` (current module), then try `src/api/handlers.rs` and `src/api/handlers/mod.rs`

#### Scenario: Python relative import resolves
- **WHEN** `src/api/handler.py` contains `from .utils import helper`
- **THEN** the import extractor SHALL resolve `.utils` to `src/api/utils.py` (or `src/api/utils/__init__.py`), and match `helper` against the target module's indexed symbols

#### Scenario: Python parent package import
- **WHEN** `src/api/handlers/user.py` contains `from ..models import User`
- **THEN** the import extractor SHALL resolve `..models` to `src/api/models.py` (or `src/api/models/__init__.py`)

#### Scenario: Unresolved import path produces NULL edge
- **WHEN** an import path resolves to a file not found in `file_manifest` (e.g., external dependency, missing file)
- **THEN** the import extractor SHALL store the edge with `to_symbol_id = NULL` and `to_name` set to the import name

#### Scenario: Symbol not found in resolved file produces NULL edge
- **WHEN** an import path resolves to an indexed file, but the target symbol name is not found in that file's indexed symbols
- **THEN** the import extractor SHALL store the edge with `to_symbol_id = NULL` and `to_name` set to the import name

#### Scenario: Go absolute imports skip resolution
- **WHEN** a Go file imports an absolute module path (e.g., `import "github.com/pkg/errors"`)
- **THEN** the import extractor SHALL skip relative path resolution and use the existing lookup strategy

### Requirement: Module-scoped call edges use file-level caller
The call extraction pipeline SHALL capture call sites at module scope (outside any function/method body) using a file-level pseudo-symbol as the caller, instead of dropping the edge.

The pseudo-symbol format: `file::<relative_path>` (e.g., `file::src/db/pool.ts`), consistent with the format used for import edge `from_symbol_id`.

#### Scenario: Top-level call produces edge — TypeScript
- **WHEN** `src/db/pool.ts` contains a module-level call `const pool = createPool(config)` (not inside any function)
- **THEN** the call extractor SHALL create an edge with `from_symbol_id = "file::src/db/pool.ts"` and `to_name = "createPool"`

#### Scenario: Top-level call produces edge — Rust
- **WHEN** `src/lib.rs` contains a module-level initialization `static POOL: Lazy<Pool> = Lazy::new(|| Pool::create())`
- **THEN** the call extractor SHALL create an edge with `from_symbol_id = "file::src/lib.rs"` and `to_name = "create"` (or `Pool::create` depending on call site resolution)

#### Scenario: Top-level call produces edge — Python
- **WHEN** `src/config.py` contains a module-level call `settings = load_config("prod")`
- **THEN** the call extractor SHALL create an edge with `from_symbol_id = "file::src/config.py"` and `to_name = "load_config"`

#### Scenario: Function-scoped call still uses function symbol
- **WHEN** `fn handle() { let db = connect(); }` calls `connect()` inside the function body
- **THEN** the call extractor SHALL use the function's `symbol_stable_id` as `from_symbol_id` (existing behavior, unchanged)

#### Scenario: File-level caller format is consistent with import edges
- **WHEN** module-scoped call edges use `from_symbol_id = "file::src/db/pool.ts"`
- **THEN** this MUST be the same format as import edges use for their `from_symbol_id` (generated by `source_symbol_id_for_path`)
