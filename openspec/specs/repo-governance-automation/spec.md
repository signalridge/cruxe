# repo-governance-automation Specification

## Purpose
Define enforceable repository governance via required CI quality gates and
benchmark harness entrypoints for regression-sensitive capabilities.
## Requirements
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

### Requirement: Security and policy checks are automated
The repository MUST provide automated security and policy checks for pull
requests, including secret-detection baseline and PR title policy validation.

#### Scenario: PR title policy rejects non-conforming title
- **WHEN** a pull request title violates repository title policy
- **THEN** policy check MUST fail with actionable remediation guidance

#### Scenario: Secret detection blocks credential leaks
- **WHEN** staged changes contain token/credential patterns detected by security scan
- **THEN** security check MUST fail and block merge until remediated

### Requirement: OpenSpec trace gate is enforced when OpenSpec assets are tracked
When OpenSpec artifacts are tracked in git, CI MUST enforce that active changes
are archived before merge according to repository OpenSpec trace policy.

#### Scenario: Active OpenSpec change blocks merge
- **WHEN** pull request contains non-archived active OpenSpec change artifacts that violate trace policy
- **THEN** OpenSpec trace gate MUST fail with remediation steps

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
