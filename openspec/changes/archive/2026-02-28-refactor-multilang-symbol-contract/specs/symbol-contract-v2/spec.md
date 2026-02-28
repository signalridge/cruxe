## ADDED Requirements

### Requirement: SymbolRole provides cross-language semantic grouping
The system MUST provide `SymbolRole` as a cross-language semantic grouping layer independent of fine-grained `SymbolKind`.

Role values and their constituent kinds:

| Role | Kinds | Use case |
|------|-------|----------|
| `Type` | Struct, Class, Enum, Trait, Interface | "Find all type definitions" across Rust+Python+TypeScript |
| `Callable` | Function, Method | "Find all callable symbols" |
| `Value` | Constant, Variable | "Find all value bindings" |
| `Namespace` | Module | "Find all namespace/module symbols" |
| `Alias` | TypeAlias | "Find all type aliases" |

The mapping from `SymbolKind` to `SymbolRole` MUST be:
- Deterministic: same kind always produces same role.
- Exhaustive: every `SymbolKind` variant maps to exactly one role.
- Stable: the mapping MUST NOT change between index-time and query-time.

`SymbolRole` MUST implement `Display`, `FromStr`, `Serialize`, `Deserialize` with lowercase string representation (`"type"`, `"callable"`, `"value"`, `"namespace"`, `"alias"`).

#### Scenario: Role mapping is deterministic for all kinds
- **WHEN** `SymbolKind::Struct` is mapped to a role at index time (via `kind.role()`)
- **THEN** the same mapping MUST produce `SymbolRole::Type` at query time

#### Scenario: Role-level retrieval works across language-specific kinds
- **WHEN** a query requests `role: "type"` across a repository with Rust and Python files
- **THEN** Rust `Struct`, `Enum`, `Trait`, `Interface` and Python `Class` symbols MUST all be included in results

#### Scenario: Every SymbolKind maps to exactly one role
- **WHEN** the `role()` method is called on any `SymbolKind` variant
- **THEN** it MUST return exactly one `SymbolRole` (total function, no panics, no `None`)

#### Scenario: SymbolRole round-trips through serialization
- **WHEN** `SymbolRole::Callable` is serialized to string
- **THEN** it MUST produce `"callable"`
- **AND WHEN** `"callable"` is deserialized
- **THEN** it MUST produce `SymbolRole::Callable`

#### Scenario: SymbolRole display matches filter parameter format
- **WHEN** `SymbolRole::Type` is formatted via `Display`
- **THEN** it MUST produce `"type"` (lowercase), matching the enum values accepted by the `role` filter parameter in MCP tools

### Requirement: Import kind is removed from SymbolKind
`SymbolKind::Import` MUST be removed as it is a dead value — `grep SymbolKind::Import` returns zero uses in enrichers/extractors, and no tag query produces this kind.

#### Scenario: Legacy deserialization handles import kind gracefully
- **WHEN** serialized data from an older index contains `"import"` as a kind value
- **THEN** deserialization MUST map it to `SymbolKind::Module` rather than failing
- **AND** the mapped `Module` kind MUST produce `SymbolRole::Namespace` via `role()`

#### Scenario: Import variant no longer exists in enum
- **WHEN** code attempts to construct `SymbolKind::Import`
- **THEN** it MUST fail at compile time (the variant is removed, not deprecated)

### Requirement: Role is materialized in search index
The `role` value MUST be stored as a STRING field in the Tantivy symbols index, derived from `kind.role()` at write time in `writer.rs`.

Critical constraint: the `role` field is NOT added to `SymbolRecord` — it is computed on-the-fly during Tantivy document construction. This avoids churn across the ~44 `SymbolRecord` construction sites in the codebase.

Implementation: in the Tantivy document builder (writer.rs), add:
```rust
doc.add_text(f_role, &sym.kind.role().to_string());
```

#### Scenario: Role field is queryable in Tantivy
- **WHEN** a search query specifies a `role` filter (e.g., `role: "type"`)
- **THEN** the query engine MUST filter results using the materialized `role` STRING field via Tantivy term query, without runtime kind→role conversion per result

#### Scenario: Role field value matches kind.role() output
- **WHEN** a symbol with `kind: "struct"` is indexed
- **THEN** the Tantivy document MUST contain `role: "type"` (derived from `SymbolKind::Struct.role() == SymbolRole::Type`)

#### Scenario: Index compatibility gate — missing role field
- **WHEN** an existing index (created before this change) lacks the `role` field in its Tantivy schema
- **THEN** the system MUST detect the incompatibility at index open time and require reindex before serving role-filtered queries
- **AND** the system MUST log a warning message indicating the required action

#### Scenario: Index compatibility gate — role field present
- **WHEN** an index created after this change includes the `role` field
- **THEN** the system MUST serve role-filtered queries normally without requiring reindex

#### Scenario: Role field not added to SymbolRecord struct
- **WHEN** `SymbolRecord` is constructed anywhere in the codebase
- **THEN** there MUST be no `role` field on the struct — role is derived at Tantivy write time only

## ADDED Requirements

### Requirement: Optional enrichment fields default to missing
The system MUST NOT substitute semantic placeholder values for enrichment fields when a safe value cannot be derived.

Enrichment fields and their optionality:

| Field | Status | Default when unavailable |
|-------|--------|------------------------|
| `qualified_name` | Optional | `None` |
| `parent_name` | Optional | `None` |
| `signature` | Optional (Function/Method only) | `None` |
| `visibility` | Optional (always `None` after this change) | `None` |
| `body` | Optional | `None` |

#### Scenario: Unknown visibility remains missing
- **WHEN** a declaration has no syntactic visibility modifier (or the generic mapper is used, which does not extract visibility)
- **THEN** visibility MUST be omitted or `null` and MUST NOT default to `"private"` or `"internal"` or any other placeholder

#### Scenario: Missing qualified_name is null, not empty string
- **WHEN** the generic mapper cannot find a parent scope for a top-level symbol
- **THEN** `qualified_name` MUST be `None`/`null`, not an empty string `""`

#### Scenario: Core fields are always present
- **WHEN** a symbol is successfully indexed in any supported language (Rust, TypeScript, Python, Go)
- **THEN** all required core fields MUST be present:
  - `repo` — repository identifier
  - `ref` — git ref or version
  - `symbol_stable_id` — deterministic content hash
  - `name` — symbol name
  - `kind` — `SymbolKind` value
  - `language` — source language
  - `path` — relative file path
  - `line_start` — start line number
  - `line_end` — end line number
- **AND** the derived `role` field MUST be present in the Tantivy search index document (but NOT on `SymbolRecord`)

#### Scenario: Signature present for callables, absent for others
- **WHEN** a Function or Method is extracted
- **THEN** `signature` MUST contain the first line of the definition text, trimmed
- **BUT WHEN** a Struct, Class, Enum, or other non-callable is extracted
- **THEN** `signature` MUST be `None`

### Requirement: symbol_stable_id computation is unchanged
The `compute_symbol_stable_id` function MUST NOT change its input formula. The inputs are: `language`, `kind`, `qualified_name`, `signature`. The `role` field is NOT an input to stable ID computation.

#### Scenario: Stable IDs are preserved across refactor
- **WHEN** the generic mapper replaces per-language enrichers
- **THEN** `symbol_stable_id` values for the same symbol MUST remain identical, given the same `language`, `kind`, `qualified_name`, and `signature` inputs

#### Scenario: Role does not affect stable ID
- **WHEN** two hypothetical symbols have identical `language`, `kind`, `qualified_name`, `signature` but different role derivations (impossible in practice since role is deterministic from kind)
- **THEN** `symbol_stable_id` MUST be identical (role is not an input)

#### Scenario: Generic mapper preserves qualified_name construction
- **WHEN** a method `handle` inside class `Server` is processed by the generic mapper in Rust
- **THEN** `qualified_name` MUST be `"Server::handle"` (same as the legacy per-language mapper output)
- **AND** `symbol_stable_id` MUST be identical to the legacy per-language mapper output

#### Scenario: Visibility change does not affect stable ID
- **WHEN** the generic mapper produces `visibility: None` for a symbol that legacy per-language mapping previously emitted as `visibility: "pub"`
- **THEN** `symbol_stable_id` MUST be unchanged (visibility is not an input to stable ID computation)
