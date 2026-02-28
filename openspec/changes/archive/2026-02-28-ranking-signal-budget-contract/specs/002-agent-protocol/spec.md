## ADDED Requirements

### Requirement: Explain output MUST expose budgeted signal accounting
When ranking explainability is enabled, protocol metadata MUST expose budget accounting fields for each signal contribution.

Required fields per signal in full mode:
- `raw_value`
- `clamped_value`
- `effective_value`

#### Scenario: Full explain includes budget accounting fields
- **WHEN** `ranking_explain_level` resolves to `full`
- **THEN** each emitted signal contribution MUST include raw/clamped/effective fields

#### Scenario: Basic explain remains compact
- **WHEN** `ranking_explain_level` resolves to `basic`
- **THEN** response MAY omit per-signal raw/clamped breakdown while preserving compatibility

### Requirement: Precedence audit metadata is additive
The protocol MUST include additive precedence-audit metadata showing whether lexical-dominance guards altered effective signal contributions.

#### Scenario: Guard-triggered adjustment is observable
- **WHEN** precedence guard reduces one or more secondary contributions
- **THEN** metadata MUST include deterministic audit information describing the adjustment
