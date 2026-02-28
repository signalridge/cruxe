## 1. Role-first baseline (cruxe-query)

- [ ] 1.1 Implement `role_weight(role: SymbolRole) -> f64`.
- [ ] 1.2 Add bounded `kind_adjustment(kind) -> f64` independent of language.
- [ ] 1.3 Update ranking formula to use `role_weight + kind_adjustment`.
- [ ] 1.4 Add unit tests for deterministic ordering by role.

## 2. Adaptive prior (cruxe-query + index/state support)

- [ ] 2.1 Define repository stats needed for adaptive priors.
- [ ] 2.2 Compute/persist stats at index time (or derive efficiently at query time).
- [ ] 2.3 Add bounded `rarity_boost` with min-sample guard.
- [ ] 2.4 Add tests for bound safety and fallback behavior.

## 3. Public-surface salience (cruxe-query)

- [ ] 3.1 Implement language-agnostic `public_surface_boost`.
- [ ] 3.2 Bound contribution to avoid overriding lexical precision.
- [ ] 3.3 Add explain output fields for salience.
- [ ] 3.4 Tests: API-facing symbol boosts, test/internal-only symbol does not.

## 4. Benchmark gate and verification

- [ ] 4.1 Extend ranking benchmark report with contribution breakdown and quality deltas.
- [ ] 4.2 Define gate: NDCG@10 / MRR@10 non-regression vs baseline (or explicit target uplift).
- [ ] 4.3 Run `cargo test --workspace` and `cargo clippy --workspace`.
- [ ] 4.4 Update OpenSpec evidence with benchmark outputs and explain snapshots.

## Dependency order

```
1 (role baseline) → 2 (adaptive prior) → 3 (salience) → 4 (benchmark/verification)
```
