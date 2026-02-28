## Why

Cruxe is landing multiple ranking and retrieval changes in parallel (diversity, rerank, centrality, import quality, chunking). Without a deterministic evaluation gate, regressions can hide behind local improvements and only appear after merge.

## What Changes

1. Add a deterministic retrieval evaluation harness with intent-segmented benchmark suites (`symbol`, `path`, `error`, `natural_language`).
2. Define quality + latency gate thresholds (Recall@k, MRR, nDCG, clustering ratio, p95 latency) and fail CI when thresholds regress beyond tolerance.
3. Add machine-readable regression reports with failure taxonomy (`recall_drop`, `ranking_shift`, `latency_regression`, `ref_scope_mismatch`).
4. Provide baseline snapshot management and comparison tooling for reproducible before/after analysis.

## Capabilities

### New Capabilities
- `retrieval-eval-gate`: deterministic retrieval quality and latency gates for change validation and release readiness.

## Impact

- Affected crates: `cruxe-query`, `cruxe-cli`, `cruxe-core`.
- CI/workflow impact: adds mandatory retrieval gate step for ranking/retrieval touching changes.
- Data impact: introduces benchmark suite fixtures + baseline snapshots under version control.
- Developer impact: changes can be validated with deterministic, comparable quality evidence instead of ad-hoc manual checks.
