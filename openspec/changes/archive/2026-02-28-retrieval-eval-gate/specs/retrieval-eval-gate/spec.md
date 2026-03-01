# retrieval-eval-gate Specification

## Purpose
Define deterministic retrieval evaluation suites and gate verdict rules so quality and latency regressions are caught consistently before release.

## Requirements
### Requirement: Deterministic retrieval evaluation suite
The system MUST provide a deterministic retrieval evaluation suite format that can be replayed across refs and environments.

Suite contract:
- each case MUST include `query`, `intent`, and one or more expected targets,
- fixtures MUST be versioned and schema-validated,
- evaluation order MUST be deterministic.

#### Scenario: Replaying the same suite yields stable ordering
- **WHEN** the same suite is executed twice against the same `(project, ref)`
- **THEN** per-query evaluated candidate ordering MUST be deterministic
- **AND** aggregate metrics MUST be reproducible within configured tolerance

#### Scenario: Invalid fixture schema is rejected early
- **WHEN** a suite entry is missing required fields such as `query` or `intent`
- **THEN** evaluation MUST fail fast with a deterministic validation error

### Requirement: Quality and latency gate verdict
The evaluator MUST produce a pass/fail gate verdict by comparing run metrics against baseline thresholds and tolerances.

Gate dimensions:
- quality: `Recall@k`, `MRR`, `nDCG`, and clustering ratio,
- latency: `p50` and `p95` by intent bucket.

#### Scenario: Meaningful quality regression fails the gate
- **WHEN** run metrics fall below baseline beyond configured tolerance for any required quality metric
- **THEN** the gate verdict MUST be `fail`
- **AND** the report MUST include the failing metric and delta

#### Scenario: Small metric noise within tolerance passes
- **WHEN** metric deltas stay within configured tolerance bands
- **THEN** the gate verdict MUST be `pass`

### Requirement: Machine-readable regression taxonomy
Evaluation output MUST include deterministic regression categories for triage.

Required categories:
- `recall_drop`
- `ranking_shift`
- `latency_regression`
- `diversity_collapse`
- `semantic_degraded_spike`

#### Scenario: Report includes categorized failures
- **WHEN** one or more gate checks fail
- **THEN** the JSON report MUST include one or more taxonomy categories matching the observed failure patterns

