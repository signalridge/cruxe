# Evidence — chunking-quality-contract (2026-03-01)

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace
cargo test -p cruxe-indexer snippet_extract::tests
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-chunking-with-fallback.toml --query-pack target/openspec-evidence/chunking-fallback-query-pack.json --ref main --limit 10 --diversity true --output target/openspec-evidence/chunking-with-fallback-report.json
cargo run -p cruxe-query --example semantic_benchmark_eval -- --workspace . --config target/openspec-evidence/config-chunking-no-fallback.toml --query-pack target/openspec-evidence/chunking-fallback-query-pack.json --ref main --limit 10 --diversity true --output target/openspec-evidence/chunking-no-fallback-report.json
```

Artifacts:

- `target/openspec-evidence/test-workspace.log`
- `target/openspec-evidence/clippy-workspace.log`
- `target/openspec-evidence/test-chunking.log`
- `target/openspec-evidence/chunking-vector-count-summary.json`
- `target/openspec-evidence/chunking-fallback-query-pack.json`
- `target/openspec-evidence/chunking-fallback-comparison-summary.json`

## Vector count impact

From `chunking-vector-count-summary.json`:

- with fallback: total vectors `4058`, fallback vectors `2` (across `2` files)
- no-fallback clone (fallback rows removed): total vectors `4056`

This confirms fallback chunks are persisted and measurable in vector inventory.

## Recall comparison (fallback on vs off)

Fallback-focused query pack (`2` queries) was evaluated against cloned data roots:

| Mode | MRR | nDCG@10 | Zero-result rate | p95 latency (ms) |
| --- | ---: | ---: | ---: | ---: |
| with fallback | 0.0 | 0.0 | 0.0 | 423.852 |
| no fallback | 0.0 | 0.0 | 0.0 | 411.759 |

Observed quality delta is neutral on this small fixture slice; fallback inventory delta is non-zero and tracked.
