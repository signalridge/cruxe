# Specs Index

> Master index for all Cruxe specification artifacts.
> This is the canonical entry point for design, execution order, and implementation-ready specs.

## Architecture

```text
specs/
  meta/
    INDEX.md                         # This file
    design.md                        # Authoritative design specification
    roadmap.md                       # Phase/version roadmap and milestone gates
    execution-order.md               # Cross-spec execution sequence
    migration-coverage.md            # Legacy plan-to-spec completeness mapping
    protocol-error-codes.md          # Canonical protocol error envelope and code registry
    development-guide.md             # Dev conventions and workflow
    repo-maintenance.md              # CI/CD, governance, release, maintenance
    testing-strategy.md              # Test layers and traceability
    benchmark-targets.md             # Quantitative quality targets
    parallel-development-guardrails.md # Module ownership + parallel touchpoints

  001-core-mvp/                      # v0.1.0
  002-agent-protocol/                # v0.2.0
  003-structure-nav/                 # v0.3.0-rc
  004-workspace-transport/           # v0.3.0
  005-vcs-core/                      # v0.9.0 (VCS correctness core)
  006-vcs-ga-tooling/                # v1.0.0 (GA tooling completion)
  007-call-graph/                    # v1.1.0
  008-semantic-hybrid/               # v1.2.0
  009-distribution/                  # v1.3.0
```

## Design Principle

- **`specs/meta/design.md`** is the single source of truth for architecture and design constraints.
- **`specs/meta/*.md`** are cross-cutting operational and planning documents.
- **`specs/00x-*`** are implementation specs with executable artifacts (`spec`, `plan`, `tasks`, optional `research`/`data-model`/`contracts`).
- VCS delivery is intentionally split into:
  - `005-vcs-core` (correctness foundation),
  - `006-vcs-ga-tooling` (GA tool surface and portability).

## Status Dashboard

| ID | Spec | Version | Tasks | Category | Status | Depends On | Blocks |
|----|------|---------|-------|----------|--------|------------|--------|
| 001 | [Core MVP](../001-core-mvp/) | v0.1.0 | 81 | C4 | implemented | -- | 002 |
| 002 | [Agent Protocol](../002-agent-protocol/) | v0.2.0 | 63 | C4 | pending | 001 | 003 |
| 003 | [Structure & Navigation](../003-structure-nav/) | v0.3.0-rc | 56 | C4 | pending | 002 | 004 |
| 004 | [Workspace & Transport](../004-workspace-transport/) | v0.3.0 | 47 | C4 | pending | 003 | 005 |
| 005 | [VCS Core](../005-vcs-core/) | v0.9.0 | 56 | C4 | pending | 004 | 006 |
| 006 | [VCS GA Tooling](../006-vcs-ga-tooling/) | v1.0.0 | 29 | C4 | pending | 005 | 007 |
| 007 | [Call Graph](../007-call-graph/) | v1.1.0 | 39 | C4 | pending | 006 | 008 |
| 008 | [Semantic/Hybrid](../008-semantic-hybrid/) | v1.2.0 | 53 | C4 | pending | 007 | 009 |
| 009 | [Distribution](../009-distribution/) | v1.3.0 | 39 | C4 | pending | 008 | -- |

| ID | Meta Document | Scope | Status |
|----|--------------|-------|--------|
| design | [Design Specification](design.md) | Architecture and constraints | active |
| roadmap | [Roadmap](roadmap.md) | Version and phase planning | active |
| exec-order | [Execution Order](execution-order.md) | Cross-spec execution sequencing | active |
| migration-coverage | [Migration Coverage](migration-coverage.md) | Legacy plan-to-spec completeness audit | active |
| protocol-errors | [Protocol Error Codes](protocol-error-codes.md) | Canonical transport error registry | active |
| dev-guide | [Development Guide](development-guide.md) | Development conventions | active |
| maintenance | [Repo Maintenance](repo-maintenance.md) | CI/CD and operations | active |
| testing | [Testing Strategy](testing-strategy.md) | Test plan and traceability | active |
| benchmarks | [Benchmark Targets](benchmark-targets.md) | Acceptance thresholds | active |
| parallel-guardrails | [Parallel Development Guardrails](parallel-development-guardrails.md) | Multi-stream change boundaries | active |

> Competitive optimization guidance is consolidated into `design.md`,
> `roadmap.md`, `testing-strategy.md`, and `benchmark-targets.md`.

## Dependency Graph

```text
001 -> 002 -> 003 -> 004 -> 005 -> 006 -> 007 -> 008 -> 009
```

## Task Summary

| Spec | Tasks | Phases | Task ID Range |
|------|-------|--------|---------------|
| 001-core-mvp | 81 | 8 | T001-T081 |
| 002-agent-protocol | 63 | 7 | T082-T139 (+ T451-T453, T462-T463) |
| 003-structure-nav | 56 | 7 | T140-T195 |
| 004-workspace-transport | 47 | 5 | T196-T239 (+ T454-T456) |
| 005-vcs-core | 56 | 6 | T240-T295 |
| 006-vcs-ga-tooling | 29 | 6 | T296-T324 |
| 007-call-graph | 39 | 6 | T325-T363 |
| 008-semantic-hybrid | 53 | 8 | T364-T411 (+ T457-T461) |
| 009-distribution | 39 | 6 | T412-T450 |
| **Total** | **463** | **59** | |

> **FR/SC Numbering Note**: FR/SC prefixes follow the original spec numbering before the VCS split.
> Specs 005-009 use FR/SC prefixes 4xx-8xx respectively (offset by one from spec IDs).
> All FR/SC numbers are globally unique with no collisions.
>
> **Alpha-suffix convention**: When a spec adds requirements after initial numbering,
> alpha suffixes (e.g. FR-101a, FR-101b) are used to maintain numeric ordering while
> keeping the original FR/SC numbers stable.

## Migration Notes

- Legacy `plan/` documents have been migrated into `specs/meta/` and are non-authoritative.
- File-level and section-level migration traceability is documented in [migration-coverage.md](migration-coverage.md).
- VCS scope was split from one large spec into two sequential specs (`005` + `006`) to reduce integration risk.
- Task IDs were globally renumbered to follow execution order and avoid cross-spec collisions.
