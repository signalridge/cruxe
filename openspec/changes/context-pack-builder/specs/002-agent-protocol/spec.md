## ADDED Requirements

### Requirement: Protocol MUST support build_context_pack tool
The MCP protocol surface MUST include a `build_context_pack` tool for structured, budgeted context assembly.

Request contract MUST include:
- `query`
- `ref` (optional, default current)
- `budget_tokens`
- optional tuning parameters for section priority.

#### Scenario: Tool invocation returns structured pack payload
- **WHEN** client calls `build_context_pack` with valid parameters
- **THEN** response MUST include sectioned pack items and pack-level diagnostics

### Requirement: Context pack response MUST expose coverage diagnostics
Response metadata MUST include additive coverage diagnostics for iterative retrieval.

Required diagnostics:
- `token_budget_used`
- `dropped_candidates`
- `coverage_summary`
- `suggested_next_queries` (when coverage is insufficient)

#### Scenario: Insufficient coverage emits next-query hints
- **WHEN** budget exhaustion prevents desired section coverage
- **THEN** response MUST include deterministic `suggested_next_queries`
