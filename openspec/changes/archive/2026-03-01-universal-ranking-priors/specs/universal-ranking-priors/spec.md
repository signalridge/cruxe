## ADDED Requirements

### Requirement: Ranking priors MUST be universal and role-first
The ranking system MUST use a universal role-first prior and MUST NOT rely on per-language kind weight tables in the core path.

Universal baseline:
- Primary prior: `role_weight(SymbolRole)`.
- Secondary adjustment: bounded `kind_adjustment(SymbolKind)`.
- No language-dispatch matrix for baseline scoring.

#### Scenario: Role-first ordering is deterministic
- **WHEN** two candidates have equivalent lexical signals and one has role `Type` while another has role `Value`
- **THEN** the `Type` candidate MUST receive higher structural prior contribution from `role_weight`

#### Scenario: Language does not change baseline kind adjustment
- **WHEN** two `function` candidates from different languages are scored under identical non-structural signals
- **THEN** they MUST receive the same `kind_adjustment` contribution

#### Scenario: Unsupported language still uses full baseline priors
- **WHEN** a symbol language is unknown or missing
- **THEN** ranking MUST still apply role-first baseline and bounded kind adjustment without special-case fallback tables

### Requirement: Kind adjustment MUST be bounded and language-agnostic
`kind_adjustment(kind)` MUST be bounded to a narrow range and MUST remain independent of source language.

Bounds:
- contribution range MUST stay within a conservative bound (for example `[-0.2, +0.2]`),
- adjustment MUST not dominate exact/qualified lexical relevance signals.

#### Scenario: Kind adjustment cannot dominate exact lexical match
- **WHEN** an exact symbol name match competes with a non-exact candidate that has a higher kind adjustment
- **THEN** exact lexical match MUST remain dominant in final order

### Requirement: Repository-adaptive prior MUST be bounded and fail-safe
When enabled, the repository-adaptive prior MUST compute a bounded `rarity_boost` from repository-level symbol distribution statistics.

Constraints:
- contribution range MUST stay within `[-0.25, +0.25]`,
- MUST be disabled when sample count is below a minimum threshold (min-sample guard),
- MUST fall back to zero contribution (static baseline) when statistics are unavailable or insufficient.

#### Scenario: Rare symbol kind receives positive adaptive boost
- **WHEN** a symbol kind is statistically rare in the repository (below median frequency)
- **THEN** its `rarity_boost` MUST be positive and within the `[0, +0.25]` bound

#### Scenario: Common symbol kind receives negative or zero adaptive adjustment
- **WHEN** a symbol kind is statistically dominant in the repository (above median frequency)
- **THEN** its `rarity_boost` MUST be non-positive and within the `[-0.25, 0]` bound

#### Scenario: Adaptive prior is disabled on small repositories
- **WHEN** the repository contains fewer symbols than the minimum sample threshold
- **THEN** `rarity_boost` MUST be zero for all candidates
- **AND** explain output MUST indicate adaptive prior is disabled due to insufficient data

#### Scenario: Adaptive prior does not override lexical precision
- **WHEN** an exact lexical match competes with a non-exact candidate that has a favorable rarity boost
- **THEN** the exact lexical match MUST remain dominant in final order

### Requirement: Public-surface salience MUST use generic signals only
Public-surface salience, if enabled, MUST use language-agnostic evidence and MUST NOT use language-specific exported-symbol rules.

Allowed evidence (bounded contribution):
- top-level definition indicator,
- inbound reference percentile,
- path-context heuristics (for example test/internal penalties).

#### Scenario: Salience boost uses graph/path evidence
- **WHEN** a top-level symbol has strong inbound references and is not under a test/internal path
- **THEN** it MUST receive a bounded positive salience contribution

#### Scenario: No language-specific export rule in core path
- **WHEN** a symbol name is uppercase in one language and lowercase in another
- **THEN** no language-scoped export heuristic MUST be applied in this change's core scoring path

### Requirement: Explain output MUST decompose universal prior components
Ranking explanation output MUST expose universal prior decomposition.

Required explain fields:
- `role_weight`
- `kind_adjustment`
- `adaptive_prior` (if enabled)
- `public_surface_salience` (if enabled)

#### Scenario: Explain output is auditable
- **WHEN** a ranked result is inspected via explain metadata
- **THEN** the response MUST include the structural prior component values as separate terms
