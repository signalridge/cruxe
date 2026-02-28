## MODIFIED Requirements

### Requirement: Import resolution MUST use provider-chain architecture
Import resolution MUST run through a deterministic provider-chain contract with normalized outcomes.

Normalized outcomes:
- `InternalResolved { to_symbol_id, to_name }`
- `ExternalReference { to_name }`
- `Unresolved { to_name }`

Provider-chain semantics:
- providers are evaluated in deterministic order,
- first provider returning an outcome wins,
- providers returning no decision allow chain continuation.

#### Scenario: Deterministic provider-chain decision
- **WHEN** multiple providers are registered and the first provider returns `None`
- **THEN** runtime MUST continue to the next provider
- **AND** MUST stop at the first provider that returns a concrete outcome

### Requirement: Generic heuristic resolver MUST be universal baseline
This phase MUST include a generic heuristic resolver as always-on baseline and MUST NOT require language-specific compiler adapters.

Baseline responsibilities:
- normalize source/import path forms,
- generate candidate file/module targets using generic path/manifest heuristics,
- validate candidates against indexed manifest/symbol data.

Out of scope for this phase:
- external adapter integrations,
- language-specific heavy resolver integrations.

#### Scenario: Baseline resolves relative import with manifest evidence
- **WHEN** an import has a relative path form and a matching candidate exists in manifest/symbol relations
- **THEN** resolver MUST emit `InternalResolved` with stable target symbol linkage

#### Scenario: Baseline unresolved path remains fail-soft
- **WHEN** no candidate passes validation
- **THEN** resolver MUST emit `Unresolved` (or `ExternalReference` when external form is recognized)
- **AND** indexing MUST continue without hard failure

### Requirement: Unresolved/external persistence semantics remain stable
Persistence semantics for unresolved imports MUST remain compatible with existing symbol contract behavior.

Persistence rules:
- unresolved/external outcomes MUST persist with `to_symbol_id = NULL`,
- `to_name` MUST preserve import target text for diagnostics,
- runtime MUST NOT synthesize fake symbol IDs for unresolved edges.

#### Scenario: Unresolved import persists nullable target
- **WHEN** provider-chain outcome is `Unresolved { to_name: "foo.bar" }`
- **THEN** persisted edge MUST store `to_symbol_id = NULL` and `to_name = "foo.bar"`

### Requirement: Resolution observability MUST be emitted
Import resolution MUST emit deterministic metrics for quality tracking and future architecture decisions.

Required metrics:
- `import_resolution_rate`,
- unresolved import ratio,
- per-provider decision counters.

#### Scenario: Metrics reflect fallback-heavy run
- **WHEN** most imports are handled by generic baseline and many remain unresolved
- **THEN** metrics MUST expose baseline decision volume and unresolved ratio for that run
