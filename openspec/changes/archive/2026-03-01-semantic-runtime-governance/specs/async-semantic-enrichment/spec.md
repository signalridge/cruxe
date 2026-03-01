## ADDED Requirements

### Requirement: Semantic enrichment MUST run asynchronously from indexing hot path
The indexing hot path MUST commit lexical/symbol retrieval data without waiting for semantic enrichment completion.

#### Scenario: Index commit succeeds even when enrichment worker is unavailable
- **WHEN** background enrichment worker is unavailable or paused
- **THEN** indexing commit MUST still complete successfully
- **AND** lexical/symbol retrieval MUST remain available immediately

### Requirement: Enrichment queue MUST implement latest-wins coalescing
Queue semantics MUST coalesce repeated updates for the same `(project_id, ref, path)` using generation ordering.

#### Scenario: Rapid edits coalesce to latest generation
- **WHEN** multiple updates for the same file are enqueued rapidly
- **THEN** worker processing MUST prioritize the latest generation
- **AND** older pending generations MUST be skipped or superseded

### Requirement: Worker execution MUST be bounded and fail-soft
Background enrichment workers MUST use bounded concurrency, retries, and timeout controls, and MUST NOT block serving queries.

#### Scenario: Worker timeout degrades semantic freshness but not availability
- **WHEN** enrichment jobs repeatedly time out
- **THEN** runtime MUST expose degraded semantic freshness metadata
- **AND** query serving MUST continue with lexical fallback semantics

### Requirement: Enrichment queue MUST have deterministic retention and cleanup policy
Queue persistence MUST define retention windows and cleanup behavior for terminal states.

Retention policy:
- `done` and superseded rows MUST be compacted/pruned after configurable TTL,
- `failed` rows MUST retain enough history for triage before TTL expiry,
- cleanup execution MUST be bounded and MUST NOT block query/indexing paths.

#### Scenario: Terminal rows are pruned without impacting serving
- **WHEN** queue cleanup runs and terminal rows exceed retention TTL
- **THEN** expired rows MUST be deleted/compacted in bounded batches
- **AND** query/indexing availability MUST remain unaffected
