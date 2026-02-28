# Pyserini comparison (sample suite)

Date: 2026-02-28

Scope: qualitative comparison between Cruxe retrieval gate output and Pyserini-style BM25 expectations for one small sample suite (`benchmarks/retrieval/query-pack.v1.json`).

## Method

- Cruxe run: `cruxe eval retrieval ... --suite benchmarks/retrieval/query-pack.v1.json --ref live`
- Reference expectations from Pyserini toolkit behavior were used at methodological level:
  - document-id centric qrels/run format
  - rank-based metrics (MRR/nDCG/Recall)

## Observed compatibility

1. **Metric compatibility**
   - Cruxe exports TREC run/qrels files (`--trec-run-out`, `--trec-qrels-out`), enabling optional `trec_eval` parity checks.

2. **Primary output format differences**
   - Pyserini centers on external corpus document IDs.
   - Cruxe centers on code-path/symbol artifacts and then derives document IDs for TREC export.

3. **Deviations (intentional)**

| Area | Pyserini typical behavior | Cruxe behavior | Rationale |
|---|---|---|---|
| Judgment target | corpus document IDs | code path / symbol hint matching | preserves code-search semantics |
| Report metadata | retrieval metrics only | retrieval metrics + semantic degradation + clustering ratio | supports agent-facing quality diagnostics |
| Gate semantics | benchmark report | CI-oriented pass/fail with tolerance policy + taxonomy | governance and regression triage |

## Conclusion

Cruxe remains interoperable with BEIR/TREC-style evaluation artifacts while intentionally extending diagnostics for code-search and agent workflows.
