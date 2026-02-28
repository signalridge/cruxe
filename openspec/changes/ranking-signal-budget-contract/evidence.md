# ranking-signal-budget-contract — verification evidence

## 1) Unit + integration evidence

```bash
cargo test -p cruxe-core ranking_signal_budget -- --nocapture
cargo test -p cruxe-query ranking::tests:: -- --nocapture
cargo test -p cruxe-query explain_ranking_signal_accounting_matches_total_effective_score -- --nocapture
cargo test -p cruxe-mcp t124b_search_code_ranking_reasons_full_mode_includes_budget_fields -- --nocapture
```

Highlights:

- config normalization fallback for invalid budget ranges verified.
- conservative secondary-cap fixture (Zoekt-style) verified.
- raw/clamped/effective accounting sum equals total score.
- MCP full mode exposes additive `signal_contributions` + `precedence_audit` while preserving legacy fields.

## 2) Workspace gates

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 3) Retrieval evaluation (pre/post)

Generated artifacts:

- `target/ranking-budget/pre.json`
- `target/ranking-budget/post.json`
- `target/ranking-budget/diff.md`
- `benchmarks/semantic/reports/ranking-signal-budget-contract-2026-03-01.md`

Headline metrics:

- Top-1 hit rate: `0.0% -> 100.0%`
- MRR: `0.50 -> 1.00`

## 4) Before/after explain sample

Case `exact_vs_structural_a`:

- pre-contract top result: `structural_hotspot`
- post-contract top result: `exact_validate_token`
- `precedence_audit.lexical_dominance_applied`: `false -> true`
- `kind_match` signal accounting: `raw 2.0, clamped 2.0, effective 1.3333` (post)

## 5) Behavior-change notes (for release/changelog)

- Ranking score composition now uses `raw -> clamped -> effective` budgeting, so absolute
  `final_score` values are not directly comparable to pre-contract runs.
- Ranking policy changed: exact lexical matches are precedence-guarded and are forced to rank
  ahead of non-exact results that only win via secondary structural signals.
- Legacy explain fields (`exact_match_boost`, `qualified_name_boost`, `path_affinity`,
  `definition_boost`, `kind_match`, `test_file_penalty`, `bm25_score`) remain raw-signal
  compatible; budget-adjusted values are exposed via `signal_contributions`.
