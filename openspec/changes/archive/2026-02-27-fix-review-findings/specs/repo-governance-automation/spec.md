## MODIFIED Requirements

### Requirement: Repository quality gates are automated and required
The repository MUST provide automated CI quality gates for pull requests,
including format, lint, and test verification for the Rust workspace, and MUST
provide executable benchmark harness entrypoints for runtime-sensitive gate
verification.

Required baseline checks:
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`

#### Scenario: Pull request fails when lint or tests regress
- **WHEN** a pull request introduces formatting, lint, or test failures
- **THEN** required CI checks MUST fail and block merge

#### Scenario: Benchmark harness executes governed runtime-sensitive checks
- **WHEN** maintainers run the repository benchmark harness entrypoint used by governance docs
- **THEN** the harness MUST execute both transport/runtime benchmark checks and semantic phase benchmark checks used for acceptance evidence

## ADDED Requirements

### Requirement: Optional all-features verification prerequisites MUST be explicit
Repository verification guidance MUST define preflight dependencies required for
optional all-features validation paths so operators can run deterministic checks
without trial-and-error.

#### Scenario: all-features run without protoc returns actionable preflight guidance
- **WHEN** an operator runs all-features verification in an environment missing `protoc`
- **THEN** repository guidance MUST identify the missing prerequisite and provide a concrete remediation command path

#### Scenario: Optional-feature preflight maps to reproducible verification commands
- **WHEN** an operator follows documented optional-feature preflight guidance
- **THEN** the operator MUST be able to run the documented verification command set deterministically for that feature lane

### Requirement: Review-driven maintainability cleanup MUST be evidence-backed
Repository workflow MUST keep review-driven dead-code/redundancy cleanup scope
explicit and behavior-safe.

#### Scenario: Redundancy cleanup is constrained to confirmed findings
- **WHEN** maintainers implement review-driven redundancy cleanup
- **THEN** changes MUST be limited to confirmed findings with behavior-preserving validation evidence (lint/tests)

#### Scenario: Dead-code cleanup avoids speculative deletions
- **WHEN** maintainers run dead-code checks for touched modules and no safe removal is confirmed
- **THEN** workflow artifacts MUST record the no-op evidence instead of forcing speculative deletions

#### Scenario: External review findings are triaged with explicit scope boundaries
- **WHEN** maintainers receive external spec-vs-code review findings spanning multiple domains
- **THEN** artifacts MUST classify each finding (confirmed/partial/not-confirmed) and map confirmed/partial items to explicit task groups in the active change workflow
