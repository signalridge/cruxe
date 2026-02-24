## MODIFIED Requirements

### Requirement: 003 metadata contract uses canonical status enums
`get_code_context`, `get_symbol_hierarchy`, and `find_related_symbols` MUST
emit Protocol v1 metadata aligned with canonical enum values.

Required metadata fields include:

- `codecompass_protocol_version`
- `freshness_status`
- `indexing_status` (`not_indexed | indexing | ready | failed`)
- `result_completeness` (`complete | partial | truncated`)
- `ref`
- `schema_status`

#### Scenario: Structure tool metadata contains canonical statuses
- **WHEN** any 003 MCP tool returns success payload
- **THEN** `metadata.indexing_status` and `metadata.result_completeness` MUST use canonical enum values

#### Scenario: Truncation is represented explicitly
- **WHEN** 003 tool output is cut due to token/payload budget
- **THEN** metadata MUST include `result_completeness: "truncated"` and relevant follow-up guidance

### Requirement: 003 tools remain token-budget driven without compact input
003 tools MUST continue using token-budget controls (`max_tokens`, truncation
metadata, suggestion hints) and MUST NOT introduce a dedicated `compact`
parameter in this change.

#### Scenario: 003 schema excludes compact
- **WHEN** clients inspect 003 MCP tool input schemas
- **THEN** there MUST be no `compact` input parameter for 003 tools in this phase

#### Scenario: Budget control still provides safe degradation
- **WHEN** caller requests limited token budget in 003 tools
- **THEN** runtime MUST return bounded output with deterministic truncation metadata and guidance
