# Ranking Signal Budget Tuning Workflow

This document describes the safe workflow for tuning `search.ranking_signal_budgets`.

## 1) Start from canonical defaults

Use `SearchConfig::default()` or keep these config defaults:

- `exact_match.default = 5.0`
- `qualified_name.default = 2.0`
- `path_affinity.default = 1.0`
- `definition_boost.default = 1.0`
- `kind_match.default = 2.0`
- `test_file_penalty.default = -0.5`
- `secondary_cap_when_exact.default = 2.0`

The config loader normalizes invalid ranges and logs deterministic taxonomy codes:

- `non_finite_range`
- `inverted_range`
- `default_out_of_range`

Normalization behavior detail:

- if range bounds are invalid/missing, runtime falls back to canonical min/max
- if `default` is finite, runtime preserves it by clamping into the normalized range
- if `default` is also invalid, runtime falls back to canonical default

## 1.5) Ranking behavior contract notes

- Ranking score remains additive (`bm25 + effective contributions`) even though the
  implementation now sets the final score directly from the normalized breakdown.
- Exact lexical matches are intentionally ordered ahead of non-exact matches as a
  deterministic precedence policy.
- Legacy explain fields (`exact_match_boost`, `kind_match`, etc.) preserve
  **raw** signal values for backward compatibility.
- Post-clamp / post-precedence values are available in `signal_accounting` /
  `signal_contributions`.

## 2) Run fixture-based retrieval evaluation (before/after)

Generate two reports:

```bash
cargo run -p cruxe-query --example ranking_budget_eval -- --profile pre \
  > target/ranking-budget/pre.json

cargo run -p cruxe-query --example ranking_budget_eval -- --profile post \
  > target/ranking-budget/post.json
```

Produce diff report:

```bash
python scripts/ranking_budget_diff_report.py \
  --before target/ranking-budget/pre.json \
  --after target/ranking-budget/post.json \
  --out target/ranking-budget/diff.md
```

Review `Top-1 hit rate` and `MRR` before applying runtime config changes.

## 3) Validate explainability compatibility

Run full-mode explain tests to ensure additive metadata contract stays stable:

```bash
cargo test -p cruxe-mcp t124b_search_code_ranking_reasons_full_mode_includes_budget_fields
cargo test -p cruxe-query explain_ranking_signal_accounting_matches_total_effective_score
```

## 4) Workspace gates before merge

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Do not merge budget changes without both gates passing.
