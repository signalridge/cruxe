## MODIFIED Requirements

### Requirement: Generic tag mapper replaces per-language enrichers
The system SHALL use a single generic tag→symbol mapper for all supported languages, replacing the per-language `LanguageEnricher` trait and its four implementations (~650 lines → ~120 lines).

The generic mapper provides:

1. **Kind mapping** — shared lookup table from tag kind strings to `SymbolKind`:
   - `has_parent` parameter enables Function→Method promotion for nested definitions.
   - `node.kind()` fallback enables struct/enum/union/type disambiguation when tag kind is ambiguous (Rust `struct_item`, `enum_item`, `union_item`, `type_item` all produce `"class"` tag kind via `@definition.class`).
   - Unrecognized tag kinds return `None` and are filtered from output.

2. **Parent scope** — generic AST parent walking:
   - `is_scope_node` match set: `impl_item`, `trait_item`, `struct_item`, `enum_item`, `mod_item`, `class_definition`, `class_declaration`, `interface_declaration`, `internal_module`, `method_declaration`, `type_declaration`, `abstract_class_declaration`.
   - `is_transparent_node` match set: `declaration_list`, `class_body`, `object_type`, `statement_block`, `block`, `decorated_definition`.
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
- **THEN** the generic mapper SHALL walk up from the method node, match `impl_item` as a scope node, extract name `Server`, and construct `qualified_name = "Server::handle"`

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

## ADDED Requirements

### Requirement: Unresolved imports use NULL representation
The import extraction pipeline SHALL store unresolved imports with `to_symbol_id = NULL` and `to_name` set to the import name, replacing the current `unresolved_symbol_stable_id()` blake3 hash approach (import_extract.rs:53,125-127).

This unifies the unresolved representation with call edges, which already use `to_symbol_id = NULL, to_name = Some(callee_name)` (call_extract.rs:34-35).

#### Scenario: Unresolved import stored with NULL target
- **WHEN** an import target (e.g., `import { Foo } from 'unknown-module'`) cannot be resolved to any indexed symbol
- **THEN** the import extractor SHALL store the edge with `to_symbol_id = NULL` and `to_name = "Foo"`
- **AND** SHALL NOT generate a blake3 hash ID via `unresolved_symbol_stable_id()` as the target

#### Scenario: Resolved import still uses actual symbol ID
- **WHEN** an import target can be resolved to an indexed symbol with known `symbol_stable_id`
- **THEN** the import extractor SHALL store `to_symbol_id = <resolved_id>` (existing behavior unchanged)

#### Scenario: Unified unresolved representation across edge types
- **WHEN** both import edges and call edges have unresolved targets
- **THEN** both MUST use `to_symbol_id = NULL, to_name = target_name` representation
- **AND** a single SQL query on `to_symbol_id IS NULL` MUST find both types of unresolved edges

#### Scenario: Existing blake3 hash IDs cleared on reindex
- **WHEN** the index is rebuilt after this change
- **THEN** previously stored blake3 hash IDs for unresolved imports SHALL be replaced with NULL representation

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
