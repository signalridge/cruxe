# Repository Maintenance

> CI/CD integration, release process, governance, dependency maintenance, and ongoing operational concerns.
> Consolidates all ops plans from `plan/ops/` into a single canonical reference.
> Cross-references verification plans in [testing-strategy.md](testing-strategy.md) and [benchmark-targets.md](benchmark-targets.md).

---

## 1. CI/CD Pipeline

> Migrated from: `plan/ops/ci-security.md`

### Objective

Establish reliable baseline automation for code quality and security without slowing development.

### Phase Integration

Execute in parallel with 001-core-mvp setup. CI must be functional before the first spec milestone (v0.1.0) -- every PR from that point onward must pass CI.

### CI Workflow (Every PR)

```yaml
# .github/workflows/ci.yml
Trigger: push / pull_request on main
Jobs:
  lint:    cargo clippy --workspace -- -D warnings
  format:  cargo fmt --check --all
  test:    cargo test --workspace
  build:   cargo build --workspace (Linux x86_64 + macOS arm64)
```

**Merge requirements**:
- All 4 jobs must pass
- No new clippy warnings
- Format compliant

**Design decisions**:
1. **CI decomposition**: Multi-job (`lint`, `test`, `build`) for failure isolation
2. **Permissions**: Per-workflow least privilege from start

### Deliverables

- `.github/workflows/ci.yml`
- Brief docs note for local/CI parity commands

### Acceptance Criteria

- [ ] PRs show required `CI` check
- [ ] Build/lint/test runs on clean checkout without manual patching

### CI Maturity Ladder

#### L1 -- Now (Baseline)

| Feature | Description |
|---------|-------------|
| Multi-job CI (`lint/test/build`) | Baseline reliability gate |
| Minimal workflow permissions | Per-workflow least privilege from start |
| `workflow_dispatch` escape hatch | Manual trigger for operability |

#### L2 -- Later (Trigger-Based Hardening)

| Feature | Trigger | Notes |
|---------|---------|-------|
| Change-aware CI fan-out (`changed-files`) | CI runtime > 10 min | Reduce unnecessary builds |
| Multi-OS matrix test/build | After baseline stabilizes | Cross-platform verification |
| Integration test + coverage union gate | On regression signals | Combined coverage reporting |
| `codecov` upload + threshold gate | Coverage SLO defined | Formal coverage policy |
| Nix/flake validation workflow | If Nix becomes first-class | Optional |
| Dedicated manual heavy-tests workflow | For expensive suites | Optional |

---

## 2. Security Scanning

> Migrated from: `plan/ops/ci-security.md`

### Security Workflow (Every PR)

```yaml
# .github/workflows/security.yml
Trigger: push / pull_request on main
Jobs:
  trivy:     Container and dependency vulnerability scan
  gitleaks:  Secret detection (API keys, tokens, credentials)
  sarif:     Upload results to GitHub Security tab
```

**Design decisions**:
1. **Security scanning frequency**: Every PR/push (reduce frequency later only if noisy)
2. **SARIF upload**: Results visible in GitHub Security tab from day one

### Deliverables

- `.github/workflows/security.yml`

### Acceptance Criteria

- [ ] PRs show required `Security` check
- [ ] SARIF results uploaded and visible in GitHub Security tab

### L2 Security Hardening (Trigger-Based)

| Feature | Trigger | Notes |
|---------|---------|-------|
| `govulncheck` SARIF upload | Supply-chain depth | Go/Rust supply-chain scanning |
| SBOM generation (`SPDX`/`CycloneDX`) | Compliance demand | Artifact retention |
| License compliance report | Compliance demand | Dependency licensing audit |

---

## 3. Repo Governance

> Migrated from: `plan/ops/repo-governance.md`

### Objective

Align local development checks with repository merge gates.

### Phase Integration

Execute in parallel with 001-core-mvp setup. Governance hooks should be in place before the first real PR lands.

### Scope

- PR title semantic validation (conventional commit format)
- Commit message conventional enforcement (local commit-msg stage)
- Baseline pre-commit hooks for YAML/Markdown/Actions/language lint
- Conditional OpenSpec trace gate (when OpenSpec assets are tracked)

**Out of scope**: Heavy template rendering checks; complex per-language local hooks not used by the codebase.

### Design Decisions

1. **Conventional enforcement**: Local hook + CI confirmation (not CI-only)
2. **OpenSpec gate**: Required when OpenSpec artifacts are tracked (matching workflow expectations)
3. **Hook scope**: Start minimal; expand only when repeated CI failures justify local pre-check

### PR Validation Workflow

```yaml
# .github/workflows/pr-title.yml
- Validates PR title follows conventional commit format
- Fast-fail: invalid PR titles rejected immediately
```

Parallel-review requirement:

- PRs touching high-conflict modules (`mcp/server`, `state/schema`, `core/config`)
  MUST follow `specs/meta/parallel-development-guardrails.md` ownership and
  boundary guidance.

### Pre-commit Configuration

```yaml
# .pre-commit-config.yaml
Hooks:
  - actionlint     # GitHub Actions lint
  - yamllint       # YAML syntax/style
  - markdownlint   # Markdown consistency
  - commit-msg     # Conventional commit enforcement (local)
```

### OpenSpec Trace Gate

```yaml
# .github/workflows/openspec-trace-gate.yml (conditional)
# .github/scripts/check_openspec_trace_gate.sh
- Enforced when OpenSpec assets are tracked in the repo
- Active change directories under openspec/changes/ must be archived before merge
```

### Branch Protection (main)

- Require PR reviews (1+ approval)
- Require all CI checks to pass
- No force pushes to `main`
- No direct commits to `main`
- Dismiss stale reviews on new pushes

### Code Ownership

| Path | Owner | Review Required |
|------|-------|----------------|
| `crates/cruxe-core/` | Core team | Always |
| `crates/cruxe-vcs/` | Core team | Always |
| `crates/cruxe-state/` | Core team | Always |
| `.github/workflows/` | Core team | Always |
| `crates/cruxe-mcp/` | Core team | Protocol changes |
| `docs/` | Any contributor | Spelling/accuracy |
| `configs/` | Any contributor | Correctness |

### Issue Labels

| Label | Meaning |
|-------|---------|
| `spec/001` - `spec/009` | Spec-scoped work |
| `priority/p1` | Must-have for current milestone |
| `priority/p2` | Should-have for current milestone |
| `priority/p3` | Nice-to-have |
| `type/bug` | Bug report |
| `type/feature` | Feature request |
| `type/refactor` | Internal improvement |
| `ops/ci` | CI/CD related |
| `ops/security` | Security related |

### Deliverables

- `.pre-commit-config.yaml`
- `.github/workflows/pr-title.yml`
- Optional `.github/workflows/openspec-trace-gate.yml`
- Optional `.github/scripts/check_openspec_trace_gate.sh`

### Acceptance Criteria

- [ ] Invalid PR titles fail fast
- [ ] Local commits fail on obvious policy/lint/secret violations
- [ ] Governance checks documented and reproducible
- [ ] OpenSpec trace gate enforced if OpenSpec assets are tracked

---

## 4. Release Process

> Migrated from: `plan/ops/release-pipeline.md`

### Objective

Create a repeatable release flow with clear separation between release planning and artifact publishing.

### Phase Integration

Set up before 001-core-mvp exit (v0.1.0). Spec 001 produces the first `cargo install`-able binary -- the release pipeline must be ready to publish it.

### Design Decisions

1. **Split strategy**: Separate `release-please` and `release` workflows for clear lifecycle
2. **Trigger**: Tag-based (`v*`) with manual dispatch fallback for explicit control
3. **Verification**: Build + smoke test now, full matrix later

### Release-Please Workflow

```yaml
# .github/workflows/release-please.yml
- Generates/updates release PR automatically on conventional commits
- Maintains changelog continuity
```

### Release Workflow (On Tag Push)

```yaml
# .github/workflows/release.yml
Trigger: push tag v* + manual workflow_dispatch
Jobs:
  build:     5 platform binaries via cargo-dist
  changelog: git-cliff generates changelog
  release:   GitHub release with binaries + checksums + changelog
  homebrew:  Update Homebrew formula (post-release)
```

**Target platforms**:
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-apple-darwin` (macOS Intel)
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `aarch64-unknown-linux-gnu` (Linux ARM64)
- `x86_64-pc-windows-msvc` (Windows)

### Version Tagging

Follow [roadmap.md version mapping](roadmap.md):

| Version | Gate | Tag Command |
|---------|------|-------------|
| v0.1.0 | Core MVP complete | `git tag v0.1.0 && git push origin v0.1.0` |
| v0.2.0 | Agent Protocol complete | `git tag v0.2.0 && git push origin v0.2.0` |
| v0.3.0-rc | Structure & Navigation complete | `git tag v0.3.0-rc && git push origin v0.3.0-rc` |
| v0.3.0 | Workspace & Transport complete | `git tag v0.3.0 && git push origin v0.3.0` |
| v0.9.0 | VCS Core complete | `git tag v0.9.0 && git push origin v0.9.0` |
| v1.0.0 | VCS GA (all acceptance criteria) | `git tag v1.0.0 && git push origin v1.0.0` |
| v1.1.0 | Call Graph complete | `git tag v1.1.0 && git push origin v1.1.0` |
| v1.2.0 | Semantic/Hybrid complete | `git tag v1.2.0 && git push origin v1.2.0` |
| v1.3.0 | Distribution complete | `git tag v1.3.0 && git push origin v1.3.0` |

### Pre-Release Checklist

Before tagging any version:

- [ ] All spec exit criteria for the target milestone are met
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check --all` passes
- [ ] `cargo build --release` succeeds
- [ ] CHANGELOG updated (or git-cliff will generate)
- [ ] Benchmark targets met (see [benchmark-targets.md](benchmark-targets.md))
- [ ] No open "blocker" issues for the milestone

### Hotfix Process

For critical fixes after a release:

1. Branch from the release tag: `git checkout -b fix/hotfix-description v1.0.0`
2. Apply minimal fix
3. Tag patch release: `v1.0.1`
4. Cherry-pick fix to `main` if applicable

### Deliverables

- `.github/workflows/release-please.yml`
- `.github/workflows/release.yml`
- `release-please-config.json`
- `.release-please-manifest.json` (if adopted)

### Acceptance Criteria

- [ ] Release PR generated/updated automatically on conventional commits
- [ ] Merging release PR and creating tag produces downloadable artifacts
- [ ] Released artifact passes smoke verification
- [ ] L2 hardening items tracked with trigger-based adoption

### L2 Release Hardening (Trigger-Based)

| Feature | Trigger | Notes |
|---------|---------|-------|
| Provenance attestation (`SLSA`/equivalent) | Compliance demand | Supply-chain trust |
| Verification matrix (container/Homebrew/deb/rpm) | Multi-channel demand | Multi-OS binaries |
| Docs publish workflow (GitHub Pages) | Docs cadence demand | Automated docs deployment |

---

## 5. Maintenance Automation

> Migrated from: `plan/ops/maintenance-automation.md`

### Objective

Automate recurring maintenance tasks while preventing noisy or unbounded PR churn.

### Phase Integration

Set up after 003/004 (structure-nav / workspace-transport). By that point the dependency tree is non-trivial. Before that, manual updates suffice.

### Design Decisions

1. **Update source**: Hybrid (Dependabot + custom scheduler) for ownership and fallback
2. **Frequency**: Weekly first, tune by PR volume
3. **Blast radius**: PR limits, group minor/patch, distinct labels for auto PRs

**Out of scope (early)**: Cross-workflow orchestration complexity; aggressive daily update cadence.

### Dependabot Configuration

```yaml
# .github/dependabot.yml
- Group minor/patch updates weekly
- Review major updates individually
- Always run full CI on dependency update PRs
- Distinct labels for automated PRs
```

### Scheduler Workflow

```yaml
# .github/workflows/scheduler.yml
Trigger: workflow_dispatch + schedule (weekly)
- Bounded concurrency for automation runs
- Fan-out to one or more update-* workflows
```

### Rust Dependencies

| Category | Crates | Update Frequency |
|----------|--------|-----------------|
| Core | `serde`, `tokio`, `tracing`, `clap` | Monthly check |
| Storage | `rusqlite`, `tantivy`, `lancedb` | Per-release review |
| Parsing | `tree-sitter`, language grammars | Per-release review |
| Security | `git2`, `blake3` | Security advisories immediately |
| Build | `thiserror`, `anyhow` | Low frequency |

### Dependency Update Workflow

```bash
# Check for outdated dependencies
cargo outdated --workspace

# Update within semver bounds
cargo update

# Audit for security vulnerabilities
cargo audit

# After updates, run full verification
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Deliverables

- `.github/dependabot.yml`
- `.github/workflows/scheduler.yml`
- One or more `update-*` workflow(s) with permissions and guardrails

### Acceptance Criteria

- [ ] Scheduled run triggers expected update workflow(s) only
- [ ] Update PR volume stays within configured limits
- [ ] Auto-update PRs are reviewable and traceable

---

## 6. CI/CD Coverage Matrix

> Migrated from: `plan/ops/cicd-coverage-matrix.md`

Cross-cutting checklist ensuring "good features" from reference repos are not missed.

### Status Legend

- **Now**: include in current implementation wave
- **Later**: tracked with explicit trigger conditions in owner plan
- **Optional**: adopt only when confirmed needed
- **Conditional**: depends on project configuration

### Feature Matrix

| Feature | Status | Owner Section | Notes |
|---------|--------|---------------|-------|
| Multi-job CI (`lint/test/build`) | Now | [CI/CD Pipeline](#1-cicd-pipeline) | Baseline reliability gate |
| Change-aware fan-out (`changed-files`) | Later | [CI/CD Pipeline](#1-cicd-pipeline) | Enable when CI runtime grows |
| Multi-OS matrix test/build | Later | [CI/CD Pipeline](#1-cicd-pipeline) | After baseline stabilizes |
| Integration test + coverage union gate | Later | [CI/CD Pipeline](#1-cicd-pipeline) | On regression signals |
| `codecov` upload and threshold policy | Later | [CI/CD Pipeline](#1-cicd-pipeline) | With formal coverage SLO |
| Nix/flake validation workflow | Optional | [CI/CD Pipeline](#1-cicd-pipeline) | Only if Nix becomes first-class |
| Dedicated manual heavy-tests workflow | Optional | [CI/CD Pipeline](#1-cicd-pipeline) | For expensive suites |
| `actionlint`/`yamllint`/`markdownlint` | Now | [Repo Governance](#3-repo-governance) | Local+CI parity |
| Trivy SARIF security scan | Now | [Security Scanning](#2-security-scanning) | Vulnerability baseline |
| Gitleaks SARIF secret scan | Now | [Security Scanning](#2-security-scanning) | Secret leak baseline |
| `govulncheck` SARIF | Later | [Security Scanning](#2-security-scanning) | Supply-chain depth |
| SBOM generation/upload | Later | [Security Scanning](#2-security-scanning) | Supply-chain evidence |
| License compliance report | Later | [Security Scanning](#2-security-scanning) | On compliance demand |
| `release-please` automation | Now | [Release Process](#4-release-process) | Release PR + changelog |
| Tag-triggered release pipeline | Now | [Release Process](#4-release-process) | Explicit release control |
| Release verification matrix | Later | [Release Process](#4-release-process) | Multi-channel demand |
| Provenance/SLSA attestation | Later | [Release Process](#4-release-process) | Compliance-driven |
| Docs deploy (GitHub Pages) | Optional | [Release Process](#4-release-process) | When docs cadence requires |
| Dependabot grouped updates | Now | [Maintenance Automation](#5-maintenance-automation) | Controlled dependency churn |
| Scheduler fan-out | Now | [Maintenance Automation](#5-maintenance-automation) | Recurring automation |
| PR title conventional gate | Now | [Repo Governance](#3-repo-governance) | Fast governance feedback |
| OpenSpec trace gate | Conditional | [Repo Governance](#3-repo-governance) | When OpenSpec assets tracked |
| Local pre-commit + commit-msg | Now | [Repo Governance](#3-repo-governance) | Shift-left checks |
| Least-privilege permissions | Now | All sections | Security baseline |
| `workflow_dispatch` escape hatch | Now | All sections | Operability |

### Closure Rule

Any `Later`/`Optional` feature is covered only if:
1. It has a named owner section in this document, and
2. Trigger conditions are documented in that section.

---

## 7. Design Decisions Archive

> Migrated from: `plan/ops/cicd-brainstorm.md`

### Context

Patterns observed from two reference repos:

- **chezmoi**: Multi-workflow CI, security SARIF, scheduled automation, OpenSpec trace gate, heavy pre-commit.
- **clinvoker**: Change-aware CI fan-out, multi-platform testing, split release (`release-please` + tag publish), docs deploy, Dependabot.

### Chosen Approach: Layered Hybrid (Approach C)

Ship baseline first, then add high-value controls in fixed layers:

1. **Layer 1**: CI + pre-commit baseline (Section 1 + Section 3)
2. **Layer 2**: Security scanning + SARIF (Section 2)
3. **Layer 3**: Release automation (Section 4)
4. **Layer 4**: Maintenance scheduler + dependency updates (Section 5)

### Rejected Approaches

- **A (Full parity fast-follow)**: Too heavy for early-stage repo
- **B (Minimal baseline only)**: Release and security process stays fragmented

---

## 8. Testing Infrastructure

Testing covers five layers (unit, integration, E2E, relevance benchmarks, and performance benchmarks) with dedicated fixture repos for each supported language. Regression gates block merges on latency regressions > 20% and precision drops > 5%.

For full testing strategy, see [testing-strategy.md](testing-strategy.md).
For benchmark thresholds, see [benchmark-targets.md](benchmark-targets.md).

---

## 9. Index Data Management

### Data Directory

```
~/.cruxe/
  data/
    <project_id>/
      base/
        symbols/      # Tantivy symbols index
        snippets/     # Tantivy snippets index
        files/        # Tantivy files index
      overlay/
        <branch>/     # Per-branch overlay indices (005-vcs-core+)
      state.db        # SQLite database
  models/             # ONNX embedding models (008-semantic-hybrid+)
  config.toml         # Global configuration
  logs/               # Hook and debug logs
```

### Cleanup Commands

```bash
# Remove all index data for a project
rm -rf ~/.cruxe/data/<project_id>

# Remove all Cruxe data
rm -rf ~/.cruxe

# Prune stale branch overlays (005-vcs-core+)
cruxe prune-overlays --ttl 30d
```

---

## 10. Monitoring and Observability

### Tracing

All operations emit structured tracing spans:
- Default level: `info`
- `--verbose` flag: `debug` level
- `CRUXE_LOG` env var: fine-grained control (e.g., `cruxe::query=trace`)

### Health Check

```bash
# CLI health check
cruxe doctor

# MCP health check (via tool call)
{ "method": "tools/call", "params": { "name": "health_check" } }

# HTTP health endpoint (004-workspace-transport+)
curl http://localhost:9100/health
```

### Key Metrics to Monitor

| Metric | Source | Alert Threshold |
|--------|--------|----------------|
| Query latency p95 | tracing spans | > 500ms (warm) |
| Index freshness | `branch_state.last_indexed_commit` | > 10 commits behind HEAD |
| SQLite DB size | `state.db` file size | > 100MB |
| Tantivy index size | `base/` directory | > 3x source size |
| Memory usage (serve-mcp) | OS metrics | > 500MB |

---

## 11. Backlog Items (Unscheduled)

Unscheduled features (search enhancements, index maintenance utilities, alternative distribution channels) are tracked in the roadmap rather than duplicated here.

For the unscheduled backlog, see [roadmap.md](roadmap.md#backlog-unscheduled).
