# Semantic Benchmark Kit

This directory provides a reproducible benchmark-kit harness for semantic/hybrid search work.

## Contents

- `fixtures.lock.json`: pinned fixture sources used by the benchmark run.
- `query-pack.v1.json`: versioned natural-language benchmark query pack.
- `run_semantic_benchmarks.sh`: deterministic harness entrypoint.
- `reports/`: optional output directory for committed examples; default runs write to `target/semantic-benchmark-reports`.

## Reproducibility Contract

The harness computes a deterministic `run_key` from:

1. `fixtures.lock.json` SHA-256
2. `query-pack.v1.json` SHA-256
3. current git revision

As long as those inputs are unchanged, reruns produce the same report file name and metadata envelope.

By default, reports are written under `target/semantic-benchmark-reports` to keep the working tree clean.

## Run

```bash
benchmarks/semantic/run_semantic_benchmarks.sh
```

Optional output directory:

```bash
benchmarks/semantic/run_semantic_benchmarks.sh --output /tmp/semantic-reports
```

## Query Pack Quality Gate

The `query-pack.v1.json` currently includes 100 NL queries stratified across:

- Rust: 25
- TypeScript: 25
- Python: 25
- Go: 25

The structure and counts are validated by automated tests in `crates/cruxe-query/src/semantic_advisor.rs` and `crates/cruxe-query/tests/semantic_query_pack.rs`.
