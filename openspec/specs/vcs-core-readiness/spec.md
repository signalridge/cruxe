# vcs-core-readiness Specification

## Purpose
TBD - created by archiving change harden-001-004-maintainability. Update Purpose after archive.
## Requirements
### Requirement: VCS adapter boundary skeleton MUST exist before 005 full implementation
The codebase MUST define a dedicated VCS adapter boundary that isolates VCS
operations from MCP transport and query orchestration paths.

The boundary MUST include:
- a `VcsAdapter` trait (or equivalent interface) for core VCS operations
- a minimal compilable implementation path that preserves current behavior
- module/crate boundaries that prevent future VCS logic from accumulating in
  MCP server dispatch modules

#### Scenario: VCS adapter abstraction compiles with current runtime
- **WHEN** the project is built after introducing the VCS adapter boundary
- **THEN** existing 001-004 behavior MUST remain compatible while the adapter abstraction is available for 005 implementation

### Requirement: Overlay merge key MUST use a unified domain type
Base/overlay merge logic MUST rely on a unified merge key domain type instead
of ad-hoc tuple/string concatenation in local call sites.

#### Scenario: Base and overlay entries with same logical identity resolve deterministically
- **WHEN** base and overlay records represent the same logical entity
- **THEN** merge logic MUST use the unified merge key domain type and produce deterministic overlay-wins behavior

