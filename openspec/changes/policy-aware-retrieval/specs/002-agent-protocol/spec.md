## ADDED Requirements

### Requirement: Protocol MUST expose policy-governance metadata
Search and context-pack responses MUST include additive policy metadata fields.

Required fields:
- `policy_mode`
- `policy_blocked_count`
- `policy_redacted_count`
- `policy_warnings` (when present)

#### Scenario: Blocked content count is observable
- **WHEN** policy filtering removes one or more candidates
- **THEN** response metadata MUST include `policy_blocked_count` greater than zero

#### Scenario: Redaction is observable
- **WHEN** one or more snippets are redacted
- **THEN** response metadata MUST include `policy_redacted_count` greater than zero

### Requirement: Policy override requests MUST honor governance constraints
If protocol allows per-request policy override parameters, runtime MUST enforce governance constraints on allowed override modes.

#### Scenario: Disallowed override is rejected deterministically
- **WHEN** request attempts to set a policy override not permitted by runtime governance
- **THEN** runtime MUST reject or normalize the override using deterministic policy rules
