## Context

Cruxe is moving from lexical-only ranking toward richer structural signals (centrality, imports, references). Edge extraction quality differs by provider and language completeness, so raw edge counts are an unstable foundation.

## Goals / Non-Goals

**Goals:**
1. Assign deterministic confidence scores to relation edges.
2. Use weighted graph aggregation for ranking signals.
3. Expose confidence provenance in explain/debug output.
4. Preserve fail-soft behavior when confidence metadata is missing.

**Non-Goals:**
1. Probabilistic ML calibration in this phase.
2. Perfect semantic correctness of every edge.
3. User-facing manual confidence tuning per edge.

## Decisions

### D1. Confidence buckets with canonical defaults
Use bounded confidence buckets (e.g. `high`, `medium`, `low`) mapped to numeric weights.
- resolved internal edge from high-quality provider -> high
- external reference edge -> medium
- unresolved heuristic edge -> low

**Why:** deterministic and easy to audit.

### D2. Weighted centrality instead of raw inbound count
Compute file/symbol structural salience using weighted edge sums.

**Why:** preserves structural value while reducing noise amplification.

### D3. Persist provenance dimensions
Keep existing `confidence` bucket as canonical categorical field, and add `edge_provider`, `resolution_outcome`, plus numeric `confidence_weight`.

**Why:** ranking explain and debugging must be traceable.

### D4. Guardrail thresholds
If confidence coverage in a query result set is below a threshold, reduce structural boost impact.

**Why:** avoid unstable behavior when graph extraction is sparse/weak.

## Risks / Trade-offs

- **[Risk] Added schema complexity** → Mitigation: additive columns + migration tests.
- **[Risk] Over-penalizing useful heuristic edges** → Mitigation: bounded floor and benchmark tuning.
- **[Risk] Query overhead** → Mitigation: materialize confidence-derived fields during indexing.

## Migration Plan

1. Add edge confidence schema fields and migrator.
2. Backfill confidence during indexing.
3. Switch ranking to weighted structural signals behind feature/config flag.
4. Enable by default after retrieval-eval validation.

Rollback: disable confidence weighting and revert to raw structural boost.

## Resolved Defaults

1. Confidence weight mapping is globally fixed in phase 1 for determinism.
2. Edge-confidence statistics are exposed via diagnostics output to support rollout tuning.

## Confidence interpretation and tuning guidance

Canonical confidence mapping:

| Bucket | Numeric weight | Typical outcome |
| --- | --- | --- |
| `high` | `1.0` | `resolved_internal` |
| `medium` | `0.6` | `external_reference` |
| `low` | `0.2` | `unresolved` or legacy heuristic |

Query-time diagnostics (`metadata.confidence_structural`) expose:

- `total_edges / high_edges / medium_edges / low_edges`
- `confidence_coverage = (high + medium) / total`
- `guardrail_applied`
- distribution maps `by_provider` and `by_outcome`

Operational guidance:

1. If `guardrail_applied=true` frequently, prioritize extraction quality (more deterministic resolver hits) before tuning ranking constants.
2. If `low_edges` dominates and `confidence_coverage < 0.45`, structural influence is intentionally dampened; this is expected fail-soft behavior.
3. Prefer tuning provider/outcome assignment quality first; confidence weights are intentionally fixed in phase 1 for reproducibility.

## Verification evidence and before/after explain examples

Verification commands run for this change:

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p cruxe-query confidence_weighted_structural_boost_suppresses_low_confidence_noise -- --nocapture`
- `cargo test -p cruxe-query low_confidence_edges_cannot_dominate_structural_boost -- --nocapture`

Deterministic comparison fixture (from `search::tests::confidence_weighted_structural_boost_suppresses_low_confidence_noise`):

- Candidate A (`stable-low`): 20 low-confidence inbound edges  
  - raw centrality: `1.00` (legacy raw-count dominant)
  - weighted centrality: `1.00`
  - coverage: `0.00` -> guardrail multiplier `0.00`
  - confidence structural contribution: `0.00`
- Candidate B (`stable-high`): 3 high-confidence inbound edges  
  - raw centrality: `0.15`
  - weighted centrality: `0.75`
  - coverage: `1.00` -> guardrail multiplier `1.00`
  - confidence structural contribution: `0.75`

Before/after ranking behavior:

| Mode | Top result |
| --- | --- |
| Raw-count intuition (legacy) | `stable-low` (noisy hub can dominate) |
| Confidence-weighted + guardrail | `stable-high` (trusted edges dominate) |

Example explain payload delta (representative):

```json
{
  "confidence_structural_boost": 0.75,
  "structural_weighted_centrality": 0.75,
  "structural_raw_centrality": 0.15,
  "structural_guardrail_multiplier": 1.0,
  "confidence_coverage": 1.0
}
```

Guardrail-suppressed candidate example:

```json
{
  "confidence_structural_boost": 0.0,
  "structural_weighted_centrality": 1.0,
  "structural_raw_centrality": 1.0,
  "structural_guardrail_multiplier": 0.0,
  "confidence_coverage": 0.0
}
```

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:
- **kythe/kythe** (Go, stars=2096)
  - Upstream focus: Kythe is a pluggable, (mostly) language-agnostic ecosystem for building tools that work with code.
  - Local clone: `<ghq>/github.com/kythe/kythe`
  - Applied insight: Cross-language graph indexing ontology and edge-type normalization ideas.
  - Source: https://github.com/kythe/kythe
- **sourcegraph/zoekt** (Go, stars=1420)
  - Upstream focus: Fast trigram based code search  
  - Local clone: `<ghq>/github.com/sourcegraph/zoekt`
  - Applied insight: File-level ranking tie-break design to keep structural signals conservative.
  - Source: https://github.com/sourcegraph/zoekt
