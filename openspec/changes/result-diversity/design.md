## Context

Without diversity controls, top results can cluster in one file even when comparable candidates exist elsewhere. This is a retrieval usability problem rather than a language problem, so the solution should stay universal.

## Goals / Non-Goals

**Goals**
1. Reduce excessive same-file concentration in top-k results.
2. Preserve high-relevance ordering with conservative swaps only.
3. Keep algorithm simple, deterministic, and low overhead.
4. Validate through diversity + relevance benchmark gates.

**Non-Goals**
1. Heavy MMR-style pairwise novelty optimization.
2. Per-language diversity rules.
3. Hard collapse to one result per file.

## Decisions

### D1. Sliding-window concentration control

#### Decision

Apply post-rank file-spread pass with defaults:

- `window_size = 5` (effective window uses `min(window_size, result_count)` )
- `max_per_file = 2`
- `min_score_ratio = 0.5`

Only rotate in a different-file candidate when score floor is respected.

#### Rationale

Fixes clustering while preserving top relevance stability.

### D2. Deterministic stable behavior

#### Decision

- Keep within-file relative order stable.
- Apply deterministic scan + rotate operations only.

#### Rationale

Stable outputs are critical for reproducibility and agent behavior.

### D3. Opt-out and compatibility

#### Decision

Respect request flag `diversity: false` to preserve pure score ordering (batch/refactor workflows).

#### Rationale

Some workflows explicitly want clustered same-file matches.

### D4. Benchmark gate

#### Decision

Track both relevance and diversity quality:

- relevance: NDCG@10 / MRR@10 delta vs baseline
- diversity: top-k unique file count, max-file-share@k

Rollout gate:

- diversity metrics improve,
- relevance regression stays within tolerated bound.

#### Rationale

Prevents “diversity looks nice but hurts quality” regressions.

## Risks / Trade-offs

- **Risk: over-diversification can demote highly relevant same-file hits.**
  - Mitigation: score floor and conservative defaults.

- **Trade-off: extra post-processing step.**
  - Accepted due to low complexity and high usability gain.
