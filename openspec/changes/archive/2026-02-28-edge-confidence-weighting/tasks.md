## 1. Confidence model and schema

- [x] 1.1 Define canonical confidence buckets and numeric mapping.
- [x] 1.2 Extend edge persistence schema with provider/outcome/confidence fields.
- [x] 1.3 Add migration/backfill tests for existing indices.
- [x] 1.4 Add deterministic default confidence rules for missing data.

## 2. Indexing and provenance capture

- [x] 2.1 Populate confidence fields in import/call/reference extraction pipelines.
- [x] 2.2 Persist edge provenance dimensions consistently across providers.
- [x] 2.3 Add fixture tests for resolved/external/unresolved edge confidence assignment.

## 3. Weighted structural ranking integration

- [x] 3.1 Replace raw edge-count centrality with weighted aggregation.
- [x] 3.2 Add guardrail for low-confidence coverage scenarios.
- [x] 3.3 Extend ranking explain output with confidence-derived contributions.
- [x] 3.4 Add ranking tests demonstrating noise suppression vs raw counting.

## 4. Observability and diagnostics

- [x] 4.1 Add counters for edge confidence distribution by provider/outcome.
- [x] 4.2 Add debug diagnostics for confidence coverage on query execution.
- [x] 4.3 Document confidence interpretation and tuning guidance.

## 5. Verification

- [x] 5.1 Run `cargo test --workspace`.
- [x] 5.2 Run `cargo clippy --workspace`.
- [x] 5.3 Run retrieval-eval-gate comparing raw vs confidence-weighted structural boosts.
- [x] 5.4 Update OpenSpec artifacts with before/after explain examples.

## 6. Cross-ecosystem provenance alignment

- [x] 6.1 Map generic resolver provider outcomes to Cruxe confidence buckets with deterministic mapping tests.
- [x] 6.2 Add Kythe-inspired edge normalization tests for mixed-language repositories.
- [x] 6.3 Add calibration fixtures ensuring low-confidence edges cannot dominate structural boosts.

### Verification evidence

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- Retrieval eval gate (deterministic local gate scenario for this change scope):
  - `cargo test -p cruxe-query confidence_weighted_structural_boost_suppresses_low_confidence_noise -- --nocapture`
  - `cargo test -p cruxe-query low_confidence_edges_cannot_dominate_structural_boost -- --nocapture`
