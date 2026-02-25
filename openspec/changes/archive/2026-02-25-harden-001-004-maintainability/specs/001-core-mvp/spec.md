## ADDED Requirements

### Requirement: Unified index process launcher across runtime indexing paths
All runtime-triggered indexing paths MUST use a shared index process launcher.

The shared launcher MUST apply the same binary resolution and environment
propagation policy for:
- MCP `index_repo`
- MCP `sync_repo`
- auto-discovered workspace bootstrap indexing

Binary resolution order MUST be deterministic:
1. `CODECOMPASS_INDEX_BIN` override
2. sibling `codecompass` binary next to current executable
3. current executable path
4. `PATH` fallback (`codecompass`)

Launcher environment propagation MUST include:
- `CODECOMPASS_PROJECT_ID`
- `CODECOMPASS_STORAGE_DATA_DIR`
- `CODECOMPASS_JOB_ID` (for job-backed operations)

#### Scenario: index_repo uses canonical launcher policy
- **WHEN** `index_repo` starts a new indexing subprocess
- **THEN** the subprocess MUST be launched through the shared launcher with canonical binary resolution order and required environment variables

#### Scenario: auto-bootstrap uses same launcher semantics
- **WHEN** auto-discovered workspace bootstrap starts indexing
- **THEN** it MUST use the same launcher implementation and environment propagation policy as `index_repo`
