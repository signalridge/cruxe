# Adaptive Query Plan Retrieval Report (2026-02-28)

## Run command

```bash
cargo test -p cruxe-query --test adaptive_query_plan_router_fixtures -- --nocapture
```

## Retrieval-eval-gate style baseline comparison

This run compares:

- **Baseline:** `search.adaptive_plan.enabled=false` (override ignored)
- **Adaptive enabled:** `search.adaptive_plan.enabled=true` (override honored)

Both runs executed 30 identical NL queries with `plan=lexical_fast`.

| Metric | Baseline (disabled) | Adaptive (enabled) | Delta |
|---|---:|---:|---:|
| p95 latency (ms) | 0.964 | 0.859 | -0.105 |
| downgrade rate | 1.000 | 0.000 | -1.000 |
| selected plan distribution | `{"hybrid_standard": 30}` | `{"lexical_fast": 30}` | override policy diverges as expected |

## Plan budget assertions (task 6.3)

| Selected plan | p95 latency (ms) | Config budget (ms) | Status |
|---|---:|---:|---|
| `lexical_fast` | 0.410 | 120 | PASS |
| `hybrid_standard` | 0.868 | 300 | PASS |
| `semantic_deep` | 1.009 | 700 | PASS |

No p95 latency exceeded its configured budget.

## Downgrade-rate assertions (task 6.3)

In no-semantic-runtime mode (`conn=None`) for override-driven benchmark loops:

- `hybrid_standard` downgrade rate: `1.000` (30/30)
- `semantic_deep` downgrade rate: `1.000` (30/30)

Both satisfy the enforced floor (`>= 0.95`) in `adaptive_query_plan_router_fixtures.rs`.
