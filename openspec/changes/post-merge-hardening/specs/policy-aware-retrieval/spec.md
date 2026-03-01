## ADDED Requirements

### Requirement: OPA command execution MUST be constrained to OPA binaries
Policy runtime MUST reject OPA command values that are not `opa` executable names.

#### Scenario: Strict mode rejects invalid OPA command
- **WHEN** policy mode is `strict`
- **AND** `search.policy.opa.command` is not an `opa` executable name
- **THEN** runtime MUST fail policy initialization

### Requirement: OPA stdin MUST be explicitly closed after input write
OPA evaluation MUST close stdin after writing input payload so the child process receives EOF deterministically.

#### Scenario: OPA process does not hang waiting for stdin EOF
- **WHEN** runtime writes OPA input JSON to child stdin
- **THEN** runtime MUST close stdin before waiting on process output

### Requirement: Symbol-kind allowlist MUST fail closed for missing symbol kind
When `allow_symbol_kinds` is configured, symbol results missing `kind` MUST be treated as allowlist misses.

#### Scenario: Symbol result without kind is blocked under allowlist
- **WHEN** `allow_symbol_kinds` is non-empty
- **AND** a symbol candidate has no `kind`
- **THEN** policy MUST block that candidate as `symbol_kind_allow_miss`
