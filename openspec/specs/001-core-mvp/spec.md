## Purpose
Define the canonical Protocol v1 metadata enum contract for runtime responses and
legacy deserialization compatibility for core status fields.
## Requirements
### Requirement: Protocol v1 metadata canonical enums
All Protocol v1 responses MUST emit canonical metadata enum values for
`indexing_status` and `result_completeness`.

Canonical values:

- `indexing_status`: `not_indexed | indexing | ready | failed`
- `result_completeness`: `complete | partial | truncated`

The runtime MUST NOT emit legacy enum values in responses.

#### Scenario: Query response emits canonical indexing status
- **WHEN** an MCP tool response includes `metadata.indexing_status`
- **THEN** the value MUST be one of `not_indexed`, `indexing`, `ready`, or `failed`

#### Scenario: Query response emits canonical completeness status
- **WHEN** an MCP tool response includes `metadata.result_completeness`
- **THEN** the value MUST be one of `complete`, `partial`, or `truncated`

### Requirement: Legacy enum compatibility on input/deserialization
The runtime MUST preserve compatibility for legacy serialized values during
deserialization or migration paths.

Legacy aliases (deserialization default when context is unavailable):

- `idle` MUST deserialize to `ready` (safe default; legacy `Idle` was
  overloaded across healthy, not-indexed, and error states)
- `partial_available` MUST deserialize to `ready`

`not_indexed` and `failed` are new variants with no legacy alias; they MUST be
accepted only as their canonical string values.

Note: the primary migration path is at the protocol metadata builder layer
(`ProtocolMetadata::new`, `not_indexed`, `reindex_required`,
`corrupt_manifest`), where each builder MUST emit the semantically correct
canonical value directly. The deserialization alias is a fallback for stale
cached payloads only.

#### Scenario: Legacy idle alias is accepted
- **WHEN** a persisted or test payload contains `indexing_status: "idle"`
- **THEN** runtime deserialization MUST succeed and normalize the value to `ready`

#### Scenario: Legacy partial alias is accepted
- **WHEN** a persisted or test payload contains `indexing_status: "partial_available"`
- **THEN** runtime deserialization MUST succeed and normalize the value to `ready`

### Requirement: Unified index process launcher across runtime indexing paths
All runtime-triggered indexing paths MUST use a shared index process launcher.

The shared launcher MUST apply the same binary resolution and environment
propagation policy for:
- MCP `index_repo`
- MCP `sync_repo`
- auto-discovered workspace bootstrap indexing

Binary resolution order MUST be deterministic:
1. `CRUXE_INDEX_BIN` override
2. sibling `cruxe` binary next to current executable
3. current executable path
4. `PATH` fallback (`cruxe`)

Launcher environment propagation MUST include:
- `CRUXE_PROJECT_ID`
- `CRUXE_STORAGE_DATA_DIR`
- `CRUXE_JOB_ID` (for job-backed operations)

#### Scenario: index_repo uses canonical launcher policy
- **WHEN** `index_repo` starts a new indexing subprocess
- **THEN** the subprocess MUST be launched through the shared launcher with canonical binary resolution order and required environment variables

#### Scenario: auto-bootstrap uses same launcher semantics
- **WHEN** auto-discovered workspace bootstrap starts indexing
- **THEN** it MUST use the same launcher implementation and environment propagation policy as `index_repo`

