## Context

All six retrieval/ranking capabilities were merged and archived into main specs. A post-merge review found that the retrieval evaluation gate in CI was still configured in dry-run mode, which weakens enforcement despite the capability's goal of catching regressions before release. The same review also found maintenance debt: one test-only helper emitted dead-code warnings in non-test builds, and newly archived specs still contained placeholder Purpose text.

## Goals / Non-Goals

**Goals:**
- Ensure retrieval eval gate runs in enforcement mode in CI when retrieval-related paths change.
- Remove avoidable build warning noise from query crate code paths.
- Harmonize floating-point tolerance checks in ranking/explain code with explicit constants.
- Restore documentation quality for archived capability specs by replacing placeholder Purpose blocks.

**Non-Goals:**
- Redesign retrieval metrics, thresholds, or suite schema.
- Change MCP tool schemas or wire formats.
- Introduce new ranking signals or policy behaviors.

## Decisions

### D1. CI retrieval gate must execute non-dry-run

- **Decision:** Remove `--dry-run` from the retrieval gate workflow invocation.
- **Rationale:** The gate's purpose is enforcement. Keeping dry-run in CI allows regressions to pass while still producing reports, which is useful only during rollout, not steady state.
- **Alternative considered:** Keep dry-run and add warning-only reporting. Rejected because it preserves silent regressions.

### D2. Use explicit tolerance constants instead of `f64::EPSILON`

- **Decision:** Reuse domain-level tolerances (`1e-9`) for score comparisons in ranking/explain logic.
- **Rationale:** `f64::EPSILON` is machine precision, not an application tolerance. Explicit constants are more robust and readable for ranking semantics.
- **Alternative considered:** Leave existing checks unchanged. Rejected because it keeps behavior overly sensitive and inconsistent with existing ranking budget tolerance usage.

### D3. Mark test-only helper as `#[cfg(test)]`

- **Decision:** Annotate `semantic_fanout_limits` as test-only since it is only referenced by tests.
- **Rationale:** Removes dead-code warnings in production builds while preserving test coverage.
- **Alternative considered:** Inline helper logic into tests. Rejected to keep tests concise and avoid duplication.

### D4. Backfill archived spec Purpose sections

- **Decision:** Replace archive-time placeholder Purpose lines with concise capability intent statements for the six newly archived specs.
- **Rationale:** Purpose text is used as human and governance index metadata. Placeholder text reduces navigability and review quality.

### D5. Apply deep-review hardening to ranking/policy/eval/context-pack hot paths

- **Decision:** Fold top risk fixes from deep review into this hardening change:
  - non-finite ranking score guards and deterministic NaN ordering fallback,
  - OPA command name validation plus explicit stdin close semantics,
  - stricter symbol-kind allowlist behavior for missing kind and stronger PEM block redaction,
  - retrieval eval matching/parser correctness (exact/suffix match + BEIR/TREC qrels compatibility),
  - per-call context source cache to avoid repeated git subprocess reads.
- **Rationale:** These are high-leverage fixes that improve correctness and operational safety without changing public MCP schema.

## Risks / Trade-offs

- **[Risk] CI strictness may surface flaky eval behavior** → Mitigation: keep fixture-based deterministic suite and artifact upload for triage.
- **[Trade-off] Slightly stricter float tolerance may shift edge-case branch behavior** → Mitigation: use existing project-wide tolerance magnitude (1e-9) and retain tests.
- **[Risk] Documentation-only edits can drift from implementation if not reviewed** → Mitigation: keep Purpose statements descriptive and non-normative.
