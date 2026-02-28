## Context

`refactor-multilang-symbol-contract` established normalized symbol roles and kinds. The follow-up should build on that normalization rather than reintroduce per-language rule proliferation.

Universal ranking priors aim to preserve quality while keeping long-term maintenance bounded.

## Goals / Non-Goals

**Goals**
1. Keep ranking explainable and language-agnostic by default.
2. Replace language-specific weight tables with universal priors.
3. Preserve tie-break quality through bounded adaptive signals.

**Non-Goals**
1. Hardcoded language-specific weight matrices in core ranking.
2. Opaque model-only ranking without explain decomposition.
3. Online learning loops in query path.

## Decisions

### D1. Role-first scoring baseline

#### Decision

Use `SymbolRole` as primary structural prior:

| Role | Base weight |
|------|-------------|
| Type | 2.0 |
| Callable | 1.6 |
| Namespace | 1.2 |
| Value | 0.9 |
| Alias | 0.8 |

`SymbolKind` contributes via small bounded adjustment (e.g., `[-0.2, +0.2]`).

#### Rationale

Roles are already normalized and portable across languages.

### D2. Repository-adaptive prior

#### Decision

Compute repository-level symbol distribution statistics and apply bounded `rarity_boost` in query-time scoring.

- default range: `[-0.25, +0.25]`
- disabled under minimum-sample threshold

#### Rationale

Captures project-specific signal without language-specific rules.

### D3. Generic public-surface salience

#### Decision

Add bounded salience boost (`<= +0.3`) using language-agnostic indicators:

- top-level definition,
- inbound reference percentile,
- path context (avoid test/internal emphasis).

#### Rationale

API-surface importance exists in all languages; graph/path proxies generalize.

### D4. Explainability contract

#### Decision

Explain output must include distinct terms for:

- role weight,
- kind adjustment,
- adaptive prior,
- public-surface salience.

#### Rationale

Preserves debugability and trust in ranking changes.

## Risks / Trade-offs

- **Risk: adaptive prior instability on tiny repositories.**
  - Mitigation: min-sample guard and fallback to static baseline.

- **Risk: losing some language-specific micro-optimizations.**
  - Mitigation: revisit only with benchmark evidence, not by default.

- **Trade-off: less bespoke tuning, more universal consistency.**
  - Accepted for maintainability and clearer behavior.
