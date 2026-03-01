# Evidence — universal-ranking-priors (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-query ranking::tests::role_weight_drives_deterministic_order_when_lexical_signals_tie
cargo test -p cruxe-query ranking::tests::adaptive_prior_boosts_rare_kinds_and_penalizes_common_kinds
cargo test -p cruxe-query ranking::tests::public_surface_salience_boosts_api_symbols_only
cargo run -p cruxe-query --example ranking_budget_eval -- --profile pre > target/openspec-evidence/ranking-budget-pre.json
cargo run -p cruxe-query --example ranking_budget_eval -- --profile post > target/openspec-evidence/ranking-budget-post.json
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-universal-priors-role.log`
- `target/openspec-evidence/test-universal-priors-adaptive.log`
- `target/openspec-evidence/test-universal-priors-salience.log`
- `target/openspec-evidence/ranking-budget-summary.json`
- `target/openspec-evidence/ranking-case-deltas.json`

## Contribution breakdown + quality deltas

`ranking-budget-summary.json` captures raw/effective contributions for prior signals:

- `role_weight`
- `kind_adjustment`
- `adaptive_prior`
- `public_surface_salience`

Quality summary (pre → post):

| Metric | Pre | Post | Delta |
| --- | ---: | ---: | ---: |
| Top-1 hit rate | 0.0 | 1.0 | +1.0 |
| MRR | 0.5 | 1.0 | +0.5 |

## Gate definition

Non-regression gate: `post.NDCG@10` and `post.MRR` must be `>= baseline - 0.02` (or explicitly exceed baseline).

Current run exceeds baseline on tracked quality metrics.
