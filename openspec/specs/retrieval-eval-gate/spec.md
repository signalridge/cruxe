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

### Requirement: Retrieval eval gate CI execution MUST be enforcement mode
When retrieval-eval-gate CI checks are triggered, the workflow MUST execute the retrieval gate command without dry-run semantics so regression verdicts can fail the job.

#### Scenario: CI retrieval gate invocation is non-dry-run
- **WHEN** retrieval-related paths trigger the `retrieval-eval-gate` workflow job
- **THEN** CI MUST invoke the gate script without `--dry-run`
- **AND** a failing gate verdict MUST fail the workflow step

#### Scenario: Gate report remains available for triage
- **WHEN** the retrieval gate job runs in CI
- **THEN** the workflow MUST upload the generated gate report artifact for analysis
- **AND** this reporting behavior MUST NOT weaken fail-fast enforcement semantics

### Requirement: Retrieval eval target matching MUST avoid substring inflation
Evaluation target matching MUST use deterministic exact/suffix matching semantics, not arbitrary substring containment, for path/name/qualified-name fields.

#### Scenario: Short suffix does not falsely match unrelated filename
- **WHEN** expected target hint is `a.rs`
- **AND** a result path is `src/data.rs`
- **THEN** this candidate MUST NOT count as a match solely by substring overlap

### Requirement: BEIR loader MUST accept both 3-column and TREC-style 4-column qrels
The BEIR/qrels ingestion path MUST parse both `query_id doc_id score` and `query_id iter doc_id score` formats.

#### Scenario: Four-column qrels row is ingested correctly
- **WHEN** a qrels line is `q1 Q0 auth_doc 1`
- **THEN** loader MUST map `q1` as query id, `auth_doc` as target hint, and `1` as relevance score

