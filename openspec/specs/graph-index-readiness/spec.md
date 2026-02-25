# graph-index-readiness Specification

## Purpose
TBD - created by archiving change harden-001-004-maintainability. Update Purpose after archive.
## Requirements
### Requirement: symbol_edges query paths MUST have explicit index strategy
The system MUST define and apply an explicit SQLite index strategy for
`symbol_edges` hot query paths before 006/007 graph-heavy tooling is
implemented.

The strategy MUST cover:
- forward traversals keyed by `from_symbol_id` and `edge_type`
- reverse traversals keyed by `to_symbol_id` and `edge_type`
- ref/repo scoping predicates used by graph queries

#### Scenario: Reverse edge lookup is backed by indexed access path
- **WHEN** a reverse graph query resolves callers or inbound edges
- **THEN** query path MUST use an index strategy aligned with `repo/ref/to_symbol_id/edge_type` predicates

#### Scenario: Forward edge lookup is backed by indexed access path
- **WHEN** a forward graph query resolves callees or outbound edges
- **THEN** query path MUST use an index strategy aligned with `repo/ref/from_symbol_id/edge_type` predicates

