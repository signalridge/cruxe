## 1. Confidence model and schema

- [ ] 1.1 Define canonical confidence buckets and numeric mapping.
- [ ] 1.2 Extend edge persistence schema with provider/outcome/confidence fields.
- [ ] 1.3 Add migration/backfill tests for existing indices.
- [ ] 1.4 Add deterministic default confidence rules for missing data.

## 2. Indexing and provenance capture

- [ ] 2.1 Populate confidence fields in import/call/reference extraction pipelines.
- [ ] 2.2 Persist edge provenance dimensions consistently across providers.
- [ ] 2.3 Add fixture tests for resolved/external/unresolved edge confidence assignment.

## 3. Weighted structural ranking integration

- [ ] 3.1 Replace raw edge-count centrality with weighted aggregation.
- [ ] 3.2 Add guardrail for low-confidence coverage scenarios.
- [ ] 3.3 Extend ranking explain output with confidence-derived contributions.
- [ ] 3.4 Add ranking tests demonstrating noise suppression vs raw counting.

## 4. Observability and diagnostics

- [ ] 4.1 Add counters for edge confidence distribution by provider/outcome.
- [ ] 4.2 Add debug diagnostics for confidence coverage on query execution.
- [ ] 4.3 Document confidence interpretation and tuning guidance.

## 5. Verification

- [ ] 5.1 Run `cargo test --workspace`.
- [ ] 5.2 Run `cargo clippy --workspace`.
- [ ] 5.3 Run retrieval-eval-gate comparing raw vs confidence-weighted structural boosts.
- [ ] 5.4 Update OpenSpec artifacts with before/after explain examples.

## 6. Cross-ecosystem provenance alignment

- [ ] 6.1 Map generic resolver provider outcomes to Cruxe confidence buckets with deterministic mapping tests.
- [ ] 6.2 Add Kythe-inspired edge normalization tests for mixed-language repositories.
- [ ] 6.3 Add calibration fixtures ensuring low-confidence edges cannot dominate structural boosts.
