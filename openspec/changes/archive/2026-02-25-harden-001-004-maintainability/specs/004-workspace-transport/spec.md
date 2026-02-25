## ADDED Requirements

### Requirement: Tool execution pipeline is transport-agnostic
`stdio` and `http` MCP transports MUST route tool execution through a shared
transport-agnostic dispatch pipeline.

Transport-specific layers MAY only handle:
- request decode
- workspace argument extraction
- response encode

Shared pipeline MUST own:
- workspace resolution
- index/runtime compatibility checks
- tool dispatch and execution semantics
- protocol metadata/error semantics

Enforcement requirements:
- transports MUST call a single shared execution entrypoint (`execute_transport_request` or equivalent)
- transport-specific execution options (progress notifier, progress token, logging policy) MUST be passed via a transport context adapter instead of inlined request branches
- new transports (WebSocket/SSE) MUST reuse the same shared execution entrypoint

#### Scenario: Same tool call yields equivalent semantics across transports
- **WHEN** the same tool request is sent via stdio and via HTTP JSON-RPC
- **THEN** both responses MUST be semantically equivalent in result payload, metadata semantics, and protocol error mapping

#### Scenario: Workspace auto-resolution behaves identically across transports
- **WHEN** an unknown workspace is submitted with auto-workspace enabled and valid allowed roots
- **THEN** both transports MUST apply the same registration, on-demand indexing, and status-tool/query-tool branching semantics

#### Scenario: Transport extension reuses shared execution seam
- **WHEN** a new transport is introduced
- **THEN** it MUST only implement decode/encode plus network IO and MUST delegate tool execution to the shared execution entrypoint with transport context

### Requirement: Health surfaces share a common core with explicit contract boundaries
`GET /health` and MCP `health_check` MUST reuse the same core health-state
aggregation logic, while preserving their own surface contracts.

Boundary requirements:
- shared core MUST provide consistent project/runtime status semantics
- surface-specific fields MAY differ only where contracts intentionally differ
- duplicated inline schema-status/interrupted-report construction logic MUST be
  eliminated through shared helper functions

#### Scenario: Shared health semantics remain aligned across surfaces
- **WHEN** runtime/index/project states are identical
- **THEN** `GET /health` and `health_check` MUST report equivalent status semantics for shared fields

#### Scenario: Surface-specific schema differences are intentional and tested
- **WHEN** a field is tool-only (for example MCP metadata extensions)
- **THEN** tests MUST assert that this difference is contract-defined rather than accidental drift

### Requirement: Degraded transport paths are observable
Transport paths that continue serving after recoverable failures MUST emit
structured warning logs with enough context for diagnosis.

Observable degrade requirements:
- DB/index open failure fallback paths MUST log warning context
- workspace resolution rejection paths MUST log reason and target workspace input
- degraded responses MUST continue following canonical protocol envelopes

#### Scenario: DB open failure falls back with warning log
- **WHEN** runtime cannot open SQLite/index in a request path but can still return a degraded response
- **THEN** the system MUST emit a structured warning log and return canonical response semantics

### Requirement: Performance verification uses split benchmark and smoke layers
Environment-sensitive p95 performance targets MUST be validated in a dedicated
benchmark layer, while integration tests retain deterministic smoke-level
performance guards.

#### Scenario: Benchmark suite enforces p95 targets
- **WHEN** benchmark suite runs against pinned fixtures and query packs
- **THEN** p95 targets from benchmark policy MUST be evaluated in the benchmark layer

#### Scenario: Integration suite keeps smoke performance guards
- **WHEN** integration tests execute on CI
- **THEN** they MUST enforce only stable smoke-level performance assertions that are resilient to normal CI jitter
