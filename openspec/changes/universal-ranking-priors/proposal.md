## Why

The previous change name (`per-language-kind-weights`) implied language-specific ranking tables. That direction conflicts with Cruxe's universality target:

- adding languages means adding and tuning more rule tables,
- maintenance cost grows non-linearly,
- ranking logic becomes fragmented and harder to explain.

Cruxe needs a stable ranking core that works across languages by default.

## What Changes

This change is re-scoped as **universal ranking priors**:

1. **Role-first baseline weights (universal)**
   - Rank by `SymbolRole` (`Type`, `Callable`, `Value`, `Namespace`, `Alias`) before language-specific kinds.

2. **Bounded kind adjustment (generic)**
   - Keep a small intra-role `kind_adjustment` for precision, but no per-language tables in core path.

3. **Repository-adaptive prior (data-driven)**
   - Add bounded boosts from repository-wide symbol distribution statistics.

4. **Generic public-surface salience**
   - Use top-level + graph + path context proxies instead of language-specific exported rules.

## Capabilities

### Modified Capabilities
- `002-agent-protocol`: ranking becomes role-first + adaptive-prior based, without per-language weight tables.

## Impact

- Affected crate: `cruxe-query` (primary), optional support in `cruxe-indexer/cruxe-state` for prior statistics.
- API impact: none.
- Maintenance impact: materially lower than per-language scoring tables.
