## 1. Benchmark suite and baseline model

- [ ] 1.1 Define versioned retrieval suite schema (`query`, `intent`, `expected_targets`, optional `negative_targets`).
- [ ] 1.2 Create initial fixture corpus covering symbol/path/error/natural-language intents.
- [ ] 1.3 Add baseline snapshot format for quality and latency metrics.
- [ ] 1.4 Add fixture validation tests (schema + deterministic ordering).

## 2. Evaluator implementation

- [ ] 2.1 Implement evaluator entrypoint (`cruxe eval retrieval` or equivalent script).
- [ ] 2.2 Implement metric computation (Recall@k, MRR, nDCG, clustering ratio).
- [ ] 2.3 Implement latency capture by intent bucket (p50/p95).
- [ ] 2.4 Emit machine-readable JSON report with failure taxonomy.

## 3. Gate policy and CI integration

- [ ] 3.1 Implement threshold + tolerance policy parser.
- [ ] 3.2 Add baseline comparison logic with deterministic pass/fail verdict.
- [ ] 3.3 Integrate gate into CI workflow for retrieval/ranking touching PRs.
- [ ] 3.4 Add non-blocking dry-run mode for rollout period.

## 4. Developer ergonomics

- [ ] 4.1 Add command/docs for local gate execution and baseline updates.
- [ ] 4.2 Add summary table output for quick terminal triage.
- [ ] 4.3 Add troubleshooting guide mapping failure category to likely subsystem.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Execute retrieval gate locally against baseline and confirm deterministic output.
- [ ] 5.4 Attach before/after gate evidence to OpenSpec artifacts.

## 6. External benchmark interoperability

- [ ] 6.1 Add BEIR-format loader compatibility (`queries`, `qrels`, `corpus`) for fixture import.
- [ ] 6.2 Add TREC run/qrels export mode and optional `trec_eval` adapter for parity checks.
- [ ] 6.3 Compare one sample suite against Pyserini reference outputs and document deviations.
