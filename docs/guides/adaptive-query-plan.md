# Adaptive Query Plan Tuning Guide

This guide explains how to tune adaptive query-plan thresholds in production.

## What is tuned

The selector chooses one of three plans:

- `lexical_fast`
- `hybrid_standard`
- `semantic_deep`

Primary knobs:

- `search.adaptive_plan.high_confidence_threshold`
- `search.adaptive_plan.low_confidence_threshold`
- per-plan fanout and latency budgets

## Recommended tuning process

1. Start from defaults and collect at least 1k mixed queries.
2. Measure:
   - plan-selection distribution
   - degraded-query rate
   - p95 latency by intent
   - recall@k / mrr on retrieval fixtures
3. Tune one threshold at a time.
4. Re-run retrieval gate and compare to baseline before rollout.

## Practical heuristics

- If `lexical_fast` over-triggers for natural-language queries:
  - increase `high_confidence_threshold` by small steps (for example `+0.02`).
- If `semantic_deep` under-triggers on ambiguous NL queries:
  - increase `low_confidence_threshold` by small steps (for example `+0.02`).
- If p95 latency regresses without quality gain:
  - reduce semantic fanout multipliers before changing thresholds.

## Guardrails

- Keep `high_confidence_threshold > low_confidence_threshold`.
- Validate any change with:
  - `cargo test -p cruxe-query`
  - `cruxe eval retrieval ... --dry-run`
- Avoid combining threshold and fanout changes in one rollout; split them so regressions are attributable.
