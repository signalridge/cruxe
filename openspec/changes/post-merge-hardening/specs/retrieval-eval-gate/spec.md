## ADDED Requirements

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
