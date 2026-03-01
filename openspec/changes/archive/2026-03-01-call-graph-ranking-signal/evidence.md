# Evidence — call-graph-ranking-signal (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-query ranking::tests::file_centrality_breaks_lexical_ties_when_structure_differs
cargo run -p cruxe-query --example ranking_budget_eval -- --profile pre > target/openspec-evidence/ranking-budget-pre.json
cargo run -p cruxe-query --example ranking_budget_eval -- --profile post > target/openspec-evidence/ranking-budget-post.json
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-call-graph-centrality-tie.log`
- `target/openspec-evidence/ranking-case-deltas.json`
- `target/openspec-evidence/ranking-budget-summary.json`

## Ranking delta checks (structure-aware fixture)

`ranking_budget_eval` shows exact-match precedence surviving structural pressure:

| Case | Pre top-1 | Post top-1 | RR (pre→post) |
| --- | --- | --- | --- |
| `exact_vs_structural_a` | `structural_hotspot` | `exact_validate_token` | `0.5 → 1.0` |
| `exact_vs_structural_b` | `structural_auth_module` | `exact_auth_service` | `0.5 → 1.0` |
| `exact_with_test_penalty` | `structural_refresh` | `exact_refresh_session` | `0.5 → 1.0` |

In post-contract output, `precedence_audit.lexical_dominance_applied=true` and secondary effective total is capped (`2.0`).

## Lexical tie + structural difference check

Added and executed unit test:

- `ranking::tests::file_centrality_breaks_lexical_ties_when_structure_differs`

Result: high-centrality candidate wins under lexical tie (pass), proving structural signal is applied deterministically without violating lexical-dominance guardrails.
