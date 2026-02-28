# Retrieval Eval Gate

`retrieval-eval-gate` adds deterministic quality/latency evaluation for ranking and retrieval changes.

Suite requirements:
- each query case MUST include `query`, `intent`, and `expected_targets`.
- missing `intent` is treated as an invalid fixture and fails fast.
- baseline `suite_version` MUST match the evaluated suite version.

## Quick start

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

## Baseline update workflow

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

## BEIR interoperability

Use BEIR-format files (`corpus.jsonl`, `queries.jsonl`, `qrels.tsv`) directly:

```bash
cruxe eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --baseline benchmarks/retrieval/beir-sample/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --beir-corpus benchmarks/retrieval/beir-sample/corpus.jsonl \
  --beir-queries benchmarks/retrieval/beir-sample/queries.jsonl \
  --beir-qrels benchmarks/retrieval/beir-sample/qrels.tsv \
  --dry-run
```

## TREC export

```bash
cruxe eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --suite benchmarks/retrieval/query-pack.v1.json \
  --baseline benchmarks/retrieval/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --trec-run-out target/retrieval.run \
  --trec-qrels-out target/retrieval.qrels \
  --dry-run
```

You can then run optional parity checks with `trec_eval`:

```bash
# Example (if trec_eval installed)
trec_eval target/retrieval.qrels target/retrieval.run
```

## Summary table interpretation

The command prints a compact summary and per-metric checks:

- `recall@k`: expected-target coverage
- `mrr`: reciprocal-rank quality
- `ndcg@k`: rank-quality weighting
- `zero_rate`: proportion of empty queries
- `cluster_ratio`: same-file concentration (high = low diversity)
- `p50_ms` / `p95_ms`: latency median/tail

Output JSON schema:
- `report`: raw evaluation metrics (`metrics`, `latency_by_intent`, `per_query`, ...)
- `gate`: gate verdict + checks + taxonomy

## Failure taxonomy troubleshooting

| Taxonomy | Typical symptom | Likely subsystem | First checks |
|---|---|---|---|
| `recall_drop` | expected targets disappear | index freshness / candidate fanout / lexical regression | reindex fixture, inspect candidate counts, compare before/after suite hits |
| `ranking_shift` | relevant hit still present but down-ranked | rerank weights / signal budget / dedup/diversity ordering | inspect ranking reasons, compare raw vs clamped signal values |
| `latency_regression` | p95 rises above tolerance | semantic fanout / rerank provider / runtime fallback loops | inspect semantic metadata (`semantic_degraded`, fanout used, provider fallback) |
| `diversity_collapse` | top-k over-clustered on one file | diversity pass / tie-break logic | check clustering ratio and same-file counts in top-k |
| `semantic_degraded_spike` | many queries degraded | model/runtime health / semantic backend failures | inspect `semantic_degraded` reason and backend availability |

## Pyserini comparison notes (sample-suite level)

We align with Pyserini concepts but intentionally differ in protocol details:

1. **Judgment granularity**
   - Pyserini commonly evaluates document IDs from external corpora.
   - Cruxe fixture suite often uses code-path/symbol-hint matching for code search realism.

2. **Pipeline coupling**
   - Pyserini focuses retrieval-layer metrics.
   - Cruxe report also tracks semantic degradation and structural clustering for agent workflows.

3. **Gate semantics**
   - Pyserini emphasizes benchmark reporting.
   - Cruxe adds CI-oriented pass/fail gate with tolerance policy and taxonomy output.

These differences are deliberate for local-first code-search quality governance.
