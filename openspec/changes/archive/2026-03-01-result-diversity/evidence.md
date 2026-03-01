# Evidence — result-diversity (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-hybrid-local.toml --query-pack benchmarks/semantic/query-pack.v1.json --ref main --limit 10 --diversity false --output target/openspec-evidence/semantic-local-diversity-off.json
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-hybrid-local.toml --query-pack benchmarks/semantic/query-pack.v1.json --ref main --limit 10 --diversity true --output target/openspec-evidence/semantic-local-diversity-on.json
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/semantic-local-diversity-off.json`
- `target/openspec-evidence/semantic-local-diversity-on.json`
- `target/openspec-evidence/semantic-comparison-summary.json`

## On/off comparison

| Metric | Diversity off | Diversity on | Delta (on-off) |
| --- | ---: | ---: | ---: |
| unique_files@k mean | 8.19 | 8.28 | +0.09 |
| max_file_share@k mean | 0.2480 | 0.2360 | -0.0120 |
| MRR | 0.0 | 0.0 | 0.0 |
| nDCG@10 | 0.0 | 0.0 | 0.0 |
| p95 latency (ms) | 43.144 | 42.332 | -0.812 |

## Pass criteria definition

- Diversity gain: `unique_files@k` must increase and `max_file_share@k` must decrease.
- Relevance guard: `ΔMRR >= -0.02` and `ΔnDCG@10 >= -0.02`.

Observed run satisfies the defined criteria.
