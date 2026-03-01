# Migration Coverage: `plan/` + `plan.md` -> `openspec/`

> Canonical migration audit for the planning-to-spec transition.
> Use this file to verify completeness and avoid drift between legacy plans and active specs.

## Scope

This audit checks two migration sources:

1. Legacy planning tree:
   - `plan/phase/*`
   - `plan/ops/*`
   - `plan/verify/*`
   - `plan/INDEX.md`
   - `plan/ROADMAP.md`
2. Monolithic design:
   - `plan.md`

## Canonical Ownership Model

- **Authoritative docs**:
  - `openspec/meta/design.md`
  - `openspec/meta/roadmap.md`
  - `openspec/meta/execution-order.md`
  - `openspec/meta/repo-maintenance.md`
  - `openspec/meta/testing-strategy.md`
  - `openspec/meta/benchmark-targets.md`
  - `openspec/specs/` capability specs + `openspec/changes/archive/` archived phases
- **Legacy docs (removed from repo after migration)**:
  - `plan.md` — migrated and deleted
  - `plan/` — migrated and deleted

## File-Level Coverage Matrix

| Legacy File | Migrated To | Coverage | Notes |
|---|---|---|---|
| `plan/phase/00-bootstrap.md` | `openspec/changes/archive/2026-02-22-001-core-mvp/` | Full | Merged with Phase 1 into spec `001` |
| `plan/phase/01-core-mvp.md` | `openspec/changes/archive/2026-02-22-001-core-mvp/` | Full | Core MVP baseline preserved |
| `plan/phase/01.1-agent-protocol.md` | `openspec/changes/archive/2026-02-22-002-agent-protocol/` | Full | Boundary clarification preserved |
| `plan/phase/01.5-structure-workspace.md` | `openspec/changes/archive/2026-02-22-003-structure-nav/` + `openspec/changes/archive/2026-02-22-004-workspace-transport/` | Full (Split) | Phase `1.5` split into `1.5a` + `1.5b` |
| `plan/phase/02-vcs-ga.md` | `openspec/changes/archive/2026-02-22-005-vcs-core/` + `openspec/changes/archive/2026-02-22-006-vcs-ga-tooling/` | Full (Split) | Phase `2` split into `2a` + `2b` |
| `plan/phase/02.5-call-graph.md` | `openspec/changes/archive/2026-02-22-007-call-graph/` | Full | Number shifted by +1 after VCS split |
| `plan/phase/03-semantic-hybrid.md` | `openspec/changes/archive/2026-02-22-008-semantic-hybrid/` | Full | Number shifted by +1 after VCS split |
| `plan/phase/04-distribution.md` | `openspec/changes/archive/2026-02-22-009-distribution/` | Full | Number shifted by +1 after VCS split |
| `plan/ops/ci-security.md` | `openspec/meta/repo-maintenance.md` §1-2 | Full | CI and security baseline retained |
| `plan/ops/repo-governance.md` | `openspec/meta/repo-maintenance.md` §3 | Full | Governance workflow retained |
| `plan/ops/release-pipeline.md` | `openspec/meta/repo-maintenance.md` §4 | Full | Release lifecycle retained |
| `plan/ops/maintenance-automation.md` | `openspec/meta/repo-maintenance.md` §5 | Full | Scheduler/dependency automation retained |
| `plan/ops/cicd-coverage-matrix.md` | `openspec/meta/repo-maintenance.md` §6 | Full | Coverage matrix retained |
| `plan/ops/cicd-brainstorm.md` | `openspec/meta/repo-maintenance.md` §7 | Full | Decision archive retained |
| `plan/verify/testing-strategy.md` | `openspec/meta/testing-strategy.md` | Full | Cross-spec strategy retained |
| `plan/verify/benchmark-targets.md` | `openspec/meta/benchmark-targets.md` | Full | Quant targets retained |
| `plan/INDEX.md` | `openspec/meta/INDEX.md` | Full | Canonical index moved to openspec |
| `plan/ROADMAP.md` | `openspec/meta/roadmap.md` + `openspec/meta/execution-order.md` | Full | Roadmap + sequencing split |

## `plan.md` Section Coverage Matrix

| `plan.md` Section | Migrated To | Coverage |
|---|---|---|
| 1. Executive Decision | `openspec/meta/design.md` §1 | Full |
| 2. Research Findings | `openspec/meta/design.md` §2 | Full |
| 3. Product Vision | `openspec/meta/design.md` §3 | Full |
| 4. Product Principles | `openspec/meta/design.md` §4 | Full |
| 5. Scope and Non-goals | `openspec/meta/design.md` §5 | Full |
| 5.1 VCS Mandatory Capability | `openspec/meta/design.md` §5.1 + archived phases 005/006 | Full |
| 6. Rust-first Architecture | `openspec/meta/design.md` §6 | Full |
| 7. Retrieval and Ranking Strategy | `openspec/meta/design.md` §7 | Full |
| 8. Feature Backlog and Algorithms | `openspec/meta/design.md` §8 + `openspec/meta/roadmap.md` backlog | Full |
| 9. Index Schema | `openspec/meta/design.md` §9 + spec data-model docs | Full |
| 10. MCP Tool Surface | `openspec/meta/design.md` §10 + per-spec contracts | Full |
| 11. CLI UX | `openspec/meta/design.md` §11 | Full |
| 12. Competitive Landscape | `openspec/meta/design.md` §12 | Full |
| 13. Phased Delivery Plan | `openspec/meta/roadmap.md` + `openspec/meta/execution-order.md` | Full |
| 14. Packaging and Distribution | archived phase 009 + `openspec/meta/repo-maintenance.md` §4 | Full |
| 15. Testing and Benchmark Plan | `openspec/meta/testing-strategy.md` + `openspec/meta/benchmark-targets.md` | Full |
| 16. Targets (Draft) | `openspec/meta/benchmark-targets.md` | Full |
| 17. Risks and Mitigations | `openspec/meta/design.md` §13 (Risk Register) | Full (Renamed) |
| 18. Security Model | `openspec/meta/design.md` §14 | Full |
| 19. Schema Versioning | `openspec/meta/design.md` §15 | Full |
| 20. Open Questions | `openspec/meta/design.md` §16 + `openspec/meta/roadmap.md` | Full |
| 21. Immediate Next Steps | `openspec/meta/execution-order.md` + archived phase task lists | Full (Operationalized) |

## Intentional Structural Deltas

These are deliberate optimizations, not migration gaps:

1. **VCS split**:
   - `plan` Phase `2` became:
     - `005-vcs-core` (correctness first)
     - `006-vcs-ga-tooling` (GA tool surface)
2. **Phase 1.5 split**:
   - `plan` Phase `1.5` became:
     - `003-structure-nav`
     - `004-workspace-transport`
3. **Cross-cutting extraction**:
   - Ops/verify materials consolidated under `openspec/meta/*` to reduce duplication and improve discoverability.

## Completeness Verdict

- Legacy source files reviewed: **18 / 18**
- `plan.md` H2 sections mapped: **21 / 21**
- Uncovered legacy source files: **0**
- Unmapped `plan.md` H2 sections: **0**

Migration is **complete**, with only intentional structural refactors listed above.
