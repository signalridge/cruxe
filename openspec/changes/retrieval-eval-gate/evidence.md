# retrieval-eval-gate verification evidence

Date: 2026-02-28
Worktree: `.worktrees/retrieval-eval-gate`
Branch: `feat/retrieval-eval-gate`

## 1) Workspace verification gates

### `cargo test --workspace`

- Command executed multiple times.
- Due benchmark-style smoke test sensitivity (`t274_incremental_sync_ten_file_smoke_under_five_seconds`) under default parallel execution, a serialized run was used for deterministic evidence:

```bash
RUST_TEST_THREADS=1 cargo test --workspace
```

- Result: **PASS** (all workspace crates/tests passed; benchmark-harness tests remained ignored as expected).

### `cargo clippy --workspace -- -D warnings`

```bash
cargo clippy --workspace -- -D warnings
```

- Result: **PASS** (no warnings).

## 2) Retrieval gate baseline run

```bash
cargo run -p cruxe -- eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --suite benchmarks/retrieval/query-pack.v1.json \
  --baseline benchmarks/retrieval/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --ref live \
  --limit 10 \
  --output target/retrieval-eval-report.json \
  --dry-run
```

Observed summary:

- suite: `retrieval-eval-suite-v1`
- total queries: `8`
- verdict: **PASS**
- recall@k: `0.8750`
- mrr: `0.8750`
- ndcg@k: `0.8750`
- zero_result_rate: `0.0000`
- clustering_ratio: `0.7571`
- degraded_query_rate: `0.0000`

Report artifact: `target/retrieval-eval-report.json`

Machine-readable output shape:

- top-level keys: `report`, `gate`
- `gate` includes `verdict`, `checks[]`, `taxonomy[]`

## 3) Determinism replay evidence

Two consecutive dry-run executions against the same `(workspace, ref)`:

- `target/retrieval-det-A.json`
- `target/retrieval-det-B.json`

Comparison result:

- `same_per_query_top_results = True`
- `same_core_metrics = True`
- latency varied as expected by runtime conditions (`p95_a=15.4955`, `p95_b=11.7563`) while staying within gate tolerance.

## 4) Before/after gate snapshots

Runs captured with full script workflow:

- `target/retrieval-eval-report-1.json`
- `target/retrieval-eval-report-2.json`

Key metric deltas:

- recall/mrr/ndcg unchanged at `0.8750`
- clustering unchanged at `0.7571`
- p95 latency varied (`13.2838` -> `13.6320`) within allowed threshold

## 5) BEIR + TREC interoperability evidence

```bash
cargo run -p cruxe -- eval retrieval \
  --workspace testdata/fixtures/rust-sample \
  --baseline benchmarks/retrieval/beir-sample/baseline.v1.json \
  --policy benchmarks/retrieval/gate-policy.v1.json \
  --beir-corpus benchmarks/retrieval/beir-sample/corpus.jsonl \
  --beir-queries benchmarks/retrieval/beir-sample/queries.jsonl \
  --beir-qrels benchmarks/retrieval/beir-sample/qrels.tsv \
  --trec-run-out target/retrieval-beir.run \
  --trec-qrels-out target/retrieval-beir.qrels \
  --dry-run \
  --output target/retrieval-beir-report.json
```

- BEIR suite loaded and executed.
- TREC artifacts generated:
  - `target/retrieval-beir.run`
  - `target/retrieval-beir.qrels`
- Gate verdict on BEIR sample baseline: **FAIL** with taxonomy `ranking_shift, recall_drop` (expected for this synthetic sample mismatch; dry-run mode preserved non-blocking behavior).

## 6) Contract hardening checks

### Required `intent` is enforced

- Fixture missing `intent` now fails fast with parse/validation error:
  - `missing field 'intent'`

### Baseline/suite compatibility is enforced

- Mismatched baseline (`suite_version != suite.version`) now fails fast before gate comparison.
