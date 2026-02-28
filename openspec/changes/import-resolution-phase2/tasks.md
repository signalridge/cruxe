## 1. Resolver contract and baseline chain (cruxe-indexer)

- [ ] 1.1 Introduce `ResolveOutcome` enum and `ImportResolverProvider` trait.
- [ ] 1.2 Implement deterministic resolver chain executor.
- [ ] 1.3 Add provider precedence + fail-soft tests.

## 2. Generic baseline provider

- [ ] 2.1 Implement `GenericHeuristicResolverProvider` (normalize + relative resolution).
- [ ] 2.2 Implement language-agnostic candidate generation (file/index-file/module-style).
- [ ] 2.3 Validate candidates against file manifest + symbol lookup.
- [ ] 2.4 Add mixed-language fixture tests.

## 3. Observability and tuning

- [ ] 3.1 Emit counters: attempts, resolved, unresolved by provider.
- [ ] 3.2 Emit per-run unresolved/import-resolution rates.
- [ ] 3.3 Add benchmark/log evidence for baseline quality tuning.

## 4. Verification

- [ ] 4.1 Run `cargo test --workspace`.
- [ ] 4.2 Run `cargo clippy --workspace`.
- [ ] 4.3 Validate indexing latency non-regression.
- [ ] 4.4 Update OpenSpec evidence with unresolved-ratio + latency comparison.

## Dependency order

```
1 (contract) → 2 (baseline) → 3 (metrics) → 4 (verification)
```
