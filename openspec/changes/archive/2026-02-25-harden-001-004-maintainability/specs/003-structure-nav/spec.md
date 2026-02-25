## ADDED Requirements

### Requirement: Structure-navigation behavior remains stable under handler decomposition
Refactoring and decomposition of structure-navigation handlers MUST preserve
external tool semantics for:
- `get_symbol_hierarchy`
- `find_related_symbols`
- `get_code_context`

Stability includes:
- unchanged input schema and parameter meaning
- unchanged response field semantics
- unchanged canonical metadata behavior
- unchanged deterministic truncation guidance behavior

#### Scenario: get_symbol_hierarchy semantics remain unchanged after refactor
- **WHEN** the same fixture and request inputs are executed before and after handler decomposition
- **THEN** response semantics (hierarchy direction, chain semantics, metadata contract) MUST remain equivalent

#### Scenario: get_code_context truncation contract remains unchanged after refactor
- **WHEN** token budget truncation is triggered in `get_code_context`
- **THEN** result bounding and deterministic truncation guidance MUST remain equivalent to pre-refactor behavior
