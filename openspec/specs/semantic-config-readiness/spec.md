# semantic-config-readiness Specification

## Purpose
TBD - created by archiving change harden-001-004-maintainability. Update Purpose after archive.
## Requirements
### Requirement: Semantic config MUST use typed runtime substructure
Runtime configuration MUST model semantic controls with typed substructures,
not free-form string branches spread across handlers.

Typed semantic config MUST include:
- `semantic_mode` gate (`off | rerank_only | hybrid`)
- profile identifier and profile-specific overrides
- compatibility normalization from legacy/default config inputs

#### Scenario: Canonical semantic mode values load into typed config
- **WHEN** config provides supported semantic mode and profile values
- **THEN** runtime MUST parse them into typed semantic config structures without stringly-typed branching in downstream handlers

#### Scenario: Invalid semantic config values fall back safely
- **WHEN** config contains unsupported semantic mode/profile values
- **THEN** runtime MUST normalize to canonical fallback values and preserve stable startup behavior

### Requirement: Semantic profile gating MUST be explicitly evaluable
Profile and feature-gate resolution MUST be deterministic and testable before
008 full semantic implementation lands.

#### Scenario: Feature gate resolution is deterministic
- **WHEN** the same semantic config inputs are evaluated across runs
- **THEN** runtime MUST resolve the same effective semantic mode/profile and expose deterministic behavior to downstream query planning

