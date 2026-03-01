## ADDED Requirements

### Requirement: Ranking budget scoring MUST guard against non-finite runtime values
Reranking score computation MUST coerce non-finite raw/budget values to deterministic safe fallbacks before clamping and sorting.

#### Scenario: Non-finite budget default does not poison score
- **WHEN** a signal budget default/min/max contains non-finite values at runtime
- **THEN** reranking MUST emit a finite score for every result
- **AND** sorting MUST remain deterministic

### Requirement: Non-finite scores MUST sort after finite scores
When any candidate has a non-finite score, deterministic ordering MUST place those candidates after finite-score candidates.

#### Scenario: NaN score does not outrank finite score
- **WHEN** one candidate has a finite score and another has `NaN`
- **THEN** finite candidate MUST sort ahead of the non-finite candidate
