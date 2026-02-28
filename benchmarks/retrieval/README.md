# Retrieval Eval Gate Kit

Deterministic benchmark-kit for `retrieval-eval-gate`.

## Files

- `query-pack.v1.json`: versioned retrieval suite (`query`, `intent`, `expected_targets`, optional `negative_targets`).
- `gate-policy.v1.json`: threshold + tolerance policy used to compute pass/fail verdicts.
- `baseline.v1.json`: baseline snapshot for deterministic comparison.
- `beir-sample/`: BEIR-compatible sample files (`corpus.jsonl`, `queries.jsonl`, `qrels.tsv`).
- `beir-sample/baseline.v1.json`: baseline for the bundled BEIR sample suite (`beir-suite-v1`).

Notes:
- `intent` is a required suite field (`symbol`, `path`, `error`, `natural_language`).
- output JSON contains both run metrics and gate result (`report` + `gate`), including `taxonomy`.
- `run_retrieval_gate.sh` reuses an existing `target/debug/cruxe` (or `CRUXE_BIN`) when available; otherwise it falls back to `cargo run`.

## Local run

```bash
cruxe eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --suite benchmarks/retrieval/query-pack.v1.json \
  --baseline benchmarks/retrieval/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --ref live \
  --limit 10 \
  --output target/retrieval-eval-report.json \
  --dry-run
```

## Baseline update

```bash
cruxe eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --suite benchmarks/retrieval/query-pack.v1.json \
  --baseline benchmarks/retrieval/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --ref live \
  --limit 10 \
  --update-baseline \
  --output target/retrieval-eval-report.json
```

## TREC export

```bash
cruxe eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --suite benchmarks/retrieval/query-pack.v1.json \
  --baseline benchmarks/retrieval/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --ref live \
  --trec-run-out target/retrieval.run \
  --trec-qrels-out target/retrieval.qrels \
  --dry-run
```
