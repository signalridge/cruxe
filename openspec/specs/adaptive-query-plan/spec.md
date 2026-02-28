# adaptive-query-plan Specification

## Purpose
TBD - created by archiving change adaptive-query-plan. Update Purpose after archive.
## Requirements
### Requirement: Query execution MUST select a deterministic retrieval plan
Runtime MUST select one canonical plan for each query using deterministic selector logic.

Canonical plans:
- `lexical_fast`
- `hybrid_standard`
- `semantic_deep`

Selector inputs include query intent, confidence, semantic runtime availability, and explicit overrides.

Selector rule order:
1. explicit plan override (if allowed),
2. semantic unavailable fallback rule,
3. high-confidence lexical rule,
4. low-confidence exploratory semantic-deep rule,
5. default hybrid rule.

#### Scenario: Symbol query selects lightweight plan
- **WHEN** query intent is symbol-oriented and lexical confidence is high
- **THEN** selector MUST choose `lexical_fast` unless an explicit override is provided

#### Scenario: Ambiguous natural-language query selects deeper plan
- **WHEN** query intent is natural-language and lexical confidence is low
- **AND** semantic runtime is available
- **THEN** selector MUST choose `semantic_deep`

#### Scenario: Deterministic rule order resolves ambiguous conditions
- **WHEN** both an explicit override and low lexical confidence are present
- **THEN** selector MUST apply explicit override first (if policy allows overrides)
- **AND** MUST NOT continue evaluating lower-priority rules

#### Scenario: Semantic unavailable forces bounded fallback
- **WHEN** intent is natural-language, lexical confidence is low, but semantic runtime is unavailable
- **THEN** selector MUST choose `hybrid_standard` (not `semantic_deep`)
- **AND** metadata MUST include deterministic downgrade/selection reason code

### Requirement: Plan execution MUST honor bounded budgets and fail-soft downgrade
Each plan MUST enforce bounded fanout/latency budgets and degrade one-way on budget/runtime pressure.

#### Scenario: Deep plan downgrades without hard failure
- **WHEN** `semantic_deep` exceeds budget or runtime constraints
- **THEN** runtime MUST downgrade to `hybrid_standard` or `lexical_fast`
- **AND** MUST still return a valid response

### Requirement: Plan selection and downgrade MUST be observable
Runtime MUST emit plan metadata including selected plan, executed plan, and deterministic reason codes for both selection and downgrade paths.

#### Scenario: Downgrade reason is reported
- **WHEN** a query plan is downgraded
- **THEN** metadata MUST include a deterministic downgrade reason code

#### Scenario: Budget metadata follows executed plan after downgrade
- **WHEN** runtime downgrades from selected plan to a lighter executed plan
- **THEN** reported budget metadata MUST reflect the executed plan budget profile

