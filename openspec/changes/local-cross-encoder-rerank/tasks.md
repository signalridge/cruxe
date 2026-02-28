## 1. Reranker implementation (cruxe-query)

- [ ] 1.1 Add `LocalCrossEncoderReranker` implementation using `fastembed::TextRerank`.
- [ ] 1.2 Implement `Rerank` trait integration and result mapping.
- [ ] 1.3 Wrap model with `Mutex` and lazy singleton (`OnceLock`).

## 2. Provider dispatch and fallback (cruxe-query)

- [ ] 2.1 Add provider branch `cross-encoder` in rerank dispatch.
- [ ] 2.2 Implement fallback to local lexical reranker with reason codes.
- [ ] 2.3 Ensure fallback metadata is surfaced in execution/report fields.

## 3. Config and budget controls (cruxe-core + cruxe-query)

- [ ] 3.1 Add config fields: `cross_encoder_model`, `cross_encoder_max_length`.
- [ ] 3.2 Add rerank budget controls (candidate cap / timeout budget).
- [ ] 3.3 Validate provider normalization accepts `cross-encoder`.

## 4. Tests

- [ ] 4.1 Unit tests for success path ordering and score mapping.
- [ ] 4.2 Unit tests for model-load failure fallback.
- [ ] 4.3 Unit tests for inference failure fallback.
- [ ] 4.4 Integration test (optional/skip-in-CI when model unavailable).

## 5. Benchmark gate and verification

- [ ] 5.1 Add benchmark cases comparing lexical rerank vs cross-encoder rerank.
- [ ] 5.2 Report NDCG@10, MRR@10, p95 latency, fallback rate.
- [ ] 5.3 Define acceptance thresholds in benchmark report.
- [ ] 5.4 Run `cargo test --workspace` and `cargo clippy --workspace`.
- [ ] 5.5 Update OpenSpec evidence with benchmark output and configuration used.

## Dependency order

```
1 (implementation) → 2 (fallback dispatch) → 3 (config/budgets) → 4 (tests) → 5 (benchmark/verification)
```
