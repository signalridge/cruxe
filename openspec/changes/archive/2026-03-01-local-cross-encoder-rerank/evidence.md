# Evidence — local-cross-encoder-rerank (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-query rerank::tests::cross_encoder_model_load_failure_falls_back_to_local
cargo test -p cruxe-query rerank::tests::cross_encoder_inference_failure_falls_back_to_local
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-hybrid-local.toml --query-pack benchmarks/semantic/query-pack.v1.json --ref main --limit 10 --diversity true --output target/openspec-evidence/semantic-local-diversity-on.json
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-hybrid-cross-encoder-fallback.toml --query-pack benchmarks/semantic/query-pack.v1.json --ref main --limit 10 --diversity true --output target/openspec-evidence/semantic-cross-encoder-diversity-on.json
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-cross-encoder-fallback-load.log`
- `target/openspec-evidence/test-cross-encoder-fallback-infer.log`
- `target/openspec-evidence/semantic-comparison-summary.json`
- `target/openspec-evidence/config-hybrid-local.toml`
- `target/openspec-evidence/config-hybrid-cross-encoder-fallback.toml`

## Benchmark case comparison (lexical/local vs cross-encoder path)

`semantic-comparison-summary.json`:

| Metric (cross vs local) | Delta |
| --- | ---: |
| p95 latency | +2.046 ms |
| MRR | 0.0 |
| nDCG@10 | 0.0 |
| fallback rate | 0.99 |
| fallback reason | `cross_encoder_model_load_failed` (99) |

## Threshold definition used in report

- **Quality gate:** `ΔMRR >= -0.02` and `ΔnDCG@10 >= -0.02` (non-regression bound)
- **Latency gate:** `p95 overhead <= 50ms` vs local rerank baseline
- **Fallback gate (model available):** fallback rate `<= 0.05`
- **Fallback gate (model intentionally unavailable in this run):** fallback rate `>= 0.95` and reason must classify to `cross_encoder_model_load_failed`

Current run satisfies the deterministic fallback-path contract and the defined non-regression bounds.
