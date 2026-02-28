# symbol-contract-v2 Specification

## Purpose
Define symbol-contract v2 for multilang indexing: optional enrichment defaults,
stable-ID invariants, and role-aware index semantics.
## Requirements
### Requirement: Optional enrichment fields default to missing
The system MUST NOT substitute semantic placeholder values for enrichment fields when a safe value cannot be derived.

Enrichment fields and their optionality:

| Field | Status | Default when unavailable |
|-------|--------|------------------------|
| `qualified_name` | Required (String) | Falls back to bare `name` when no parent scope |
| `parent_name` | Optional | `None` |
| `signature` | Optional (Function/Method only) | `None` |
| `visibility` | Optional (always `None` after this change) | `None` |
| `body` | Optional | `None` |

#### Scenario: Unknown visibility remains missing
- **WHEN** a declaration has no syntactic visibility modifier (or the generic mapper is used, which does not extract visibility)
- **THEN** visibility MUST be omitted or `null` and MUST NOT default to `"private"` or `"internal"` or any other placeholder

#### Scenario: Missing parent scope produces bare name as qualified_name
- **WHEN** the generic mapper cannot find a parent scope for a top-level symbol
- **THEN** `qualified_name` MUST be set to the bare symbol `name` (e.g., function `foo` gets `qualified_name = "foo"`), not an empty string `""`

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
