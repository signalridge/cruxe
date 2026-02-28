# Ranking Signal Budget Contract Evaluation (2026-03-01)

## Scope

Validate bounded ranking behavior and lexical-dominance precedence introduced by
`ranking-signal-budget-contract`.

## Commands

```bash
cargo run -p cruxe-query --example ranking_budget_eval -- --profile pre \
  > target/ranking-budget/pre.json

cargo run -p cruxe-query --example ranking_budget_eval -- --profile post \
  > target/ranking-budget/post.json

python scripts/ranking_budget_diff_report.py \
  --before target/ranking-budget/pre.json \
  --after target/ranking-budget/post.json \
  --out target/ranking-budget/diff.md
```

## Summary metrics (retrieval-eval-gate style)

| Metric | Pre (simulated) | Post (contracted) | Delta |
| --- | ---: | ---: | ---: |
| Top-1 hit rate | 0.0% | 100.0% | +100.0% |
| MRR | 0.50 | 1.00 | +0.50 |

Source: `target/ranking-budget/diff.md`.

## Explain sample (before → after)

Case: `exact_vs_structural_a`, query `validate_token`

- **Before (pre-contract simulated):**
  - top result: `structural_hotspot`
  - `exact_match_boost=0.0`
  - `precedence_audit.lexical_dominance_applied=false`
  - secondary effective total: `6.0`

- **After (post-contract):**
  - top result: `exact_validate_token`
  - `exact_match_boost=5.0`
  - `precedence_audit.lexical_dominance_applied=true`
  - secondary effective cap: `2.0`
  - `kind_match` signal accounting: raw `2.0` → clamped `2.0` → effective `1.3333`

This confirms raw/clamped/effective decomposition and precedence gating are visible in
full explain payload.
