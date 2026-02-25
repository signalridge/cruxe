## ADDED Requirements

### Requirement: Repository quality gates are automated and required
The repository MUST provide automated CI quality gates for pull requests,
including format, lint, and test verification for the Rust workspace.

Required baseline checks:
- `cargo fmt --all --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`

#### Scenario: Pull request fails when lint or tests regress
- **WHEN** a pull request introduces formatting, lint, or test failures
- **THEN** required CI checks MUST fail and block merge

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
