# semantic-config-readiness Specification

## Purpose
TBD - created by archiving change harden-001-004-maintainability. Update Purpose after archive.
## Requirements
### Requirement: Semantic config MUST use typed runtime substructure
Runtime configuration MUST model semantic controls with typed substructures,
not free-form string branches spread across handlers.

Typed semantic config MUST include:
- `semantic_mode` gate (`off | rerank_only | hybrid`)
- profile identifier and profile-specific overrides
- compatibility normalization from legacy/default config inputs

#### Scenario: Canonical semantic mode values load into typed config
- **WHEN** config provides supported semantic mode and profile values
- **THEN** runtime MUST parse them into typed semantic config structures without stringly-typed branching in downstream handlers

#### Scenario: Invalid semantic config values fall back safely
- **WHEN** config contains unsupported semantic mode/profile values
- **THEN** runtime MUST normalize to canonical fallback values and preserve stable startup behavior

### Requirement: Semantic profile gating MUST be explicitly evaluable
Profile and feature-gate resolution MUST be deterministic and testable before
and during semantic runtime execution, including degraded runtime states when
local embedding runtime dependencies are unavailable or runtime embedding calls
fail unexpectedly.

Implementation note: `embedding.rs:207-254` already provides these guarantees
via `match Ok/Err` on `TextEmbedding::try_new` and `.ok().and_then()` on
`runtime.embed`. The scenarios below formalize existing behavior as spec-level
requirements.

#### Scenario: Feature gate resolution is deterministic
- **WHEN** the same semantic config inputs are evaluated across runs
- **THEN** runtime MUST resolve the same effective semantic mode/profile and expose deterministic behavior to downstream query planning

#### Scenario: Missing local runtime dependencies degrade gracefully
- **WHEN** semantic execution requires local embedding runtime and runtime initialization fails due to missing local dependencies (for example ONNX runtime dylib)
- **THEN** runtime MUST NOT panic and MUST continue with deterministic fallback behavior that preserves successful request and test execution semantics
- **Evidence**: `embedding.rs:207-225` — `TextEmbedding::try_new` failure handled via `match Err` with `warn!` log and `None` cache entry, causing all subsequent calls to use `deterministic_embedding()` fallback.

#### Scenario: Runtime embedding invocation failure degrades gracefully
- **WHEN** semantic execution reaches local runtime embedding invocation and that invocation fails or panics
- **THEN** runtime MUST NOT panic and MUST continue with deterministic fallback behavior for the current request
- **Evidence**: `embedding.rs:230-254` — `runtime.embed` failure handled via `.lock().ok().and_then(|r| r.embed(...).ok())`; on failure, runtime ref is set to `None` and remaining inputs fall through to `deterministic_embedding()`.
- **Note**: `.ok()` handles Rust-level `Err` returns. FFI-level panics from ONNX C library are NOT caught by this path; `catch_unwind` would be needed for that scenario and is deferred as a separate scope decision.

#### Scenario: Degraded runtime fallback remains deterministic
- **WHEN** the same query and semantic config are executed repeatedly under degraded local runtime conditions
- **THEN** fallback indicators and response behavior MUST remain deterministic across runs
- **Evidence**: `deterministic_embedding()` uses a seeded hash of input text, producing identical vectors for identical inputs regardless of runtime state.

