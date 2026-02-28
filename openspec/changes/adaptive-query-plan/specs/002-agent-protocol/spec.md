## ADDED Requirements

### Requirement: Protocol MUST expose adaptive query plan metadata
`search_code` metadata MUST include additive fields describing adaptive planning decisions.

Required additive fields:
- `query_plan_selected`
- `query_plan_selection_reason`
- `query_plan_downgraded` (boolean)
- `query_plan_downgrade_reason` (when downgraded)
- `query_plan_budget_used`

#### Scenario: Selected plan appears in metadata
- **WHEN** search executes with adaptive planning enabled
- **THEN** response metadata MUST include `query_plan_selected`

#### Scenario: Downgraded execution includes reason
- **WHEN** runtime downgrades from a deeper plan to a lighter plan
- **THEN** metadata MUST include `query_plan_downgraded=true` and a deterministic reason code

#### Scenario: Selection reason is always present
- **WHEN** adaptive planning is enabled
- **THEN** metadata MUST include `query_plan_selection_reason` with a deterministic rule code
