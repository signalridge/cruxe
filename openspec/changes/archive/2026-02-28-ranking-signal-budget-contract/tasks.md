## 1. Signal budget schema and core scoring

- [x] 1.1 Define per-signal budget registry (min/max/default).
- [x] 1.2 Implement raw→clamped→effective scoring flow in ranking core.
- [x] 1.3 Add unit tests for bound enforcement and deterministic ordering.
- [x] 1.4 Add precedence invariants for exact lexical dominance.

## 2. Config validation and normalization

- [x] 2.1 Extend ranking config model with explicit budget ranges.
- [x] 2.2 Implement startup lint/normalization for invalid weight ranges.
- [x] 2.3 Add warning/error taxonomy for config violations.
- [x] 2.4 Add tests for canonical fallback behavior.

## 3. Explain and protocol integration

- [x] 3.1 Extend explain structures with `raw`, `clamped`, `effective` per signal.
- [x] 3.2 Preserve backward-compatible legacy explain fields.
- [x] 3.3 Add precedence audit fields for `full` explain mode.
- [x] 3.4 Add MCP tests for explain payload compatibility.

## 4. Quality regression safeguards

- [x] 4.1 Add targeted ranking fixtures where secondary signals could dominate.
- [x] 4.2 Validate bounded behavior with retrieval-eval-gate metrics.
- [x] 4.3 Document tuning workflow for budget adjustments.

## 5. Verification

- [x] 5.1 Run `cargo test --workspace`.
- [x] 5.2 Run `cargo clippy --workspace`.
- [x] 5.3 Run retrieval evaluation comparing pre/post budget contract.
- [x] 5.4 Record OpenSpec evidence with before/after explain samples.

## 6. External ranking calibration

- [x] 6.1 Add score-budget fixture cases inspired by Zoekt's conservative boost strategy.
- [x] 6.2 Add Tantivy-oriented explain parity tests for raw/clamped/effective decomposition.
- [x] 6.3 Add a budget-diff report script for pre/post ranking contract changes.
