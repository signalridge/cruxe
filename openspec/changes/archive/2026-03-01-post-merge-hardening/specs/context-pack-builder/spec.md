## ADDED Requirements

### Requirement: Context-pack source reads MUST reuse per-call source cache
Context-pack assembly MUST cache source file content per `(ref, path)` within a single build call to avoid repeated subprocess/file reads for repeated snippets.

#### Scenario: Repeated snippets from same file reuse cached source content
- **WHEN** multiple selected candidates reference the same `(ref, path)`
- **THEN** context-pack assembly MUST load source content once for that key
- **AND** subsequent snippets MUST derive line ranges from cached content
