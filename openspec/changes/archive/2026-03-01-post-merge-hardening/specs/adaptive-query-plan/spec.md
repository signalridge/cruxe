## ADDED Requirements

### Requirement: Semantic-unavailable fallback MUST select lexical-fast across intents
When semantic runtime is unavailable, adaptive plan selection MUST choose `lexical_fast` regardless of query intent.

#### Scenario: Natural-language query falls back to lexical-fast when semantic is unavailable
- **WHEN** query intent is `natural_language`
- **AND** semantic runtime is unavailable
- **THEN** selected plan MUST be `lexical_fast`
- **AND** selection reason MUST be `semantic_unavailable`
