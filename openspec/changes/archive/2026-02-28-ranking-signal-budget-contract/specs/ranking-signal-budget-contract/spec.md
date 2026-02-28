## ADDED Requirements

### Requirement: Ranking signals MUST use bounded contribution budgets
Each ranking signal MUST be evaluated with explicit contribution bounds.

For each signal, runtime MUST compute:
- `raw_value`,
- `clamped_value` (bounded by signal budget),
- `effective_value` (after precedence guards).

#### Scenario: Out-of-range signal is clamped
- **WHEN** a signal raw value exceeds its configured max budget
- **THEN** runtime MUST clamp it to the max
- **AND** downstream scoring MUST use the clamped value

#### Scenario: In-range signal remains unchanged
- **WHEN** a signal raw value is within its configured budget
- **THEN** clamped and raw values MUST be equal

### Requirement: Lexical dominance precedence guard
Secondary structural or heuristic signals MUST NOT override exact lexical relevance invariants.

#### Scenario: Exact lexical match remains dominant
- **WHEN** candidate A has exact lexical match and candidate B only has high structural boosts
- **THEN** precedence guard MUST prevent B from outranking A solely due to secondary signals

### Requirement: Budget config MUST be normalized to safe defaults
Invalid or unsafe budget config values MUST be normalized to canonical safe ranges.

#### Scenario: Invalid budget config falls back safely
- **WHEN** configuration provides non-numeric or inverted range values
- **THEN** runtime MUST use canonical safe defaults
- **AND** MUST emit deterministic normalization diagnostics
