## Why

Cruxe ranking now composes many additive signals. As new signals are introduced, unbounded contributions can cause instability (a weak structural signal overpowering exact lexical evidence) and make explainability harder to trust.

## What Changes

1. Introduce a formal signal budget contract with per-signal min/max contribution bounds.
2. Add precedence invariants (exact lexical relevance remains dominant under all configurations).
3. Standardize explain payload to include raw, clamped, and effective contribution for each signal.
4. Add config linting/normalization to reject unsafe weights and non-canonical ranges.

## Capabilities

### New Capabilities
- `ranking-signal-budget-contract`: deterministic signal composition contract with bounded contributions and precedence guarantees.

### Modified Capabilities
- `002-agent-protocol`: ranking explain metadata includes budget-aware signal fields (`raw`, `clamped`, `effective`) and precedence audit information.

## Impact

- Affected crates: `cruxe-query`, `cruxe-core`, `cruxe-mcp`.
- API impact: additive explain metadata fields only.
- Config impact: stricter validation for ranking-related weights.
- Product impact: ranking policy now explicitly enforces lexical dominance for exact matches, and
  score budgets change absolute score magnitudes (`final_score`) relative to pre-contract runs.
