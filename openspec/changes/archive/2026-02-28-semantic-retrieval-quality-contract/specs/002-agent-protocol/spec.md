## ADDED Requirements

### Requirement: Semantic runtime metadata must be explicit and additive
`search_code` protocol metadata MUST expose semantic runtime state as explicit additive fields, preserving backward compatibility with existing clients.

Required semantic metadata set for this change:
- `semantic_mode` (existing)
- `semantic_enabled` (existing)
- `semantic_ratio_used` (existing)
- `semantic_triggered` (existing)
- `semantic_skipped_reason` (existing)
- `semantic_fallback` (existing)
- `semantic_degraded` (**new**, additive)
- `semantic_limit_used` (**new**, additive)
- `lexical_fanout_used` (**new**, additive)
- `semantic_fanout_used` (**new**, additive)
- `semantic_budget_exhausted` (**new**, additive)

Additive compatibility rules:
- Existing fields MUST keep current semantics.
- New fields MUST be optional/omittable in serialized output when unavailable.
- Clients that ignore unknown fields MUST continue to function unchanged.

#### Scenario: Degraded semantic fallback emits normalized metadata
- **WHEN** semantic execution falls back due to backend failure
- **THEN** metadata MUST include `semantic_fallback=true` and `semantic_degraded=true`
- **AND** MUST include deterministic `semantic_skipped_reason`

#### Scenario: Budget metadata reflects effective runtime values
- **WHEN** hybrid semantic search executes with configured multipliers
- **THEN** metadata MUST include `semantic_limit_used`, `lexical_fanout_used`, and `semantic_fanout_used`
- **AND** these values MUST reflect post-floor/post-cap effective values

#### Scenario: Legacy clients remain compatible
- **WHEN** a client only reads legacy semantic metadata fields
- **THEN** the response MUST remain parseable and semantically valid without requiring new-field handling
