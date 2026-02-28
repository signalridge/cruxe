## Context

Current verification primarily checks compile/lint/tests and selected benchmark examples. That is necessary but not sufficient for ranking-heavy work: subtle score shifts can reduce recall or increase same-file clustering while still passing unit tests.

We need a deterministic retrieval gate that:
- is reproducible per ref,
- exposes quality and latency together,
- classifies failures so developers can fix root cause quickly.

## Goals / Non-Goals

**Goals:**
1. Build a stable benchmark suite format and evaluator CLI.
2. Enforce explicit gate thresholds in CI with intent-segmented reporting.
3. Provide machine-readable diff output for regression triage.
4. Keep runtime-independent baseline mode for CI determinism.

**Non-Goals:**
1. Online learning or production traffic replay in this phase.
2. Replacing existing functional/unit tests.
3. Introducing cloud-only evaluation dependencies.

## Decisions

### D1. Versioned suite + baseline artifacts
Use versioned JSONL suites (`query`, `intent`, `expected targets`) and store baseline metric snapshots in-repo.

**Why:** supports deterministic replay and reviewable changes.

**Alternatives considered:** ephemeral benchmark runs only (rejected: non-reproducible).

### D2. Gate on both quality and latency
Gate dimensions:
- Quality: Recall@k, MRR, nDCG, clustering ratio.
- Latency: p50/p95 for each intent bucket.

**Why:** quality-only gates can hide unusable latency; latency-only gates can hide relevance loss.

### D3. Regression tolerance policy
Introduce per-metric tolerance bands (e.g. allow tiny noise but fail meaningful regressions).

**Why:** prevents flaky failures while preserving strictness.

### D4. Failure taxonomy in report output
Evaluator emits deterministic categories:
- `recall_drop`
- `ranking_shift`
- `latency_regression`
- `diversity_collapse`
- `semantic_degraded_spike`

**Why:** shortens diagnosis loop and aligns fixes to root cause.

## Risks / Trade-offs

- **[Risk] Baseline drift from accidental updates** → Mitigation: baseline update command requires explicit flag and reviewable diff.
- **[Risk] CI runtime increase** → Mitigation: split quick gate suite vs full nightly suite.
- **[Risk] Overfitting to benchmark fixtures** → Mitigation: include intent diversity + periodic fixture refresh process.

## Migration Plan

1. Add evaluator CLI + suite schema.
2. Introduce initial baseline from current `main` behavior.
3. Add non-blocking CI report mode for 1-2 iterations.
4. Flip to blocking gate once stability is confirmed.

Rollback: disable blocking mode while keeping report generation for visibility.

## Resolved Defaults

1. Phase 1 uses one default threshold profile to keep governance simple; tiered profiles can be added later.
2. Phase 1 keeps one canonical baseline run profile with semantic-enabled execution and explicit degraded-subset metrics.

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:

- **castorini/pyserini** (Python, stars=2024)
  - Upstream focus: Pyserini is a Python toolkit for reproducible information retrieval research with sparse and dense representations.
  - Local clone: `<ghq>/github.com/castorini/pyserini`
  - Applied insight: IR evaluation toolkit and BM25/dense hybrid baselines, useful for benchmark harness structure.
  - Source: https://github.com/castorini/pyserini
- **beir-cellar/beir** (Python, stars=2092)
  - Upstream focus: A Heterogeneous Benchmark for Information Retrieval. Easy to use, evaluate your models across 15+ diverse IR datasets.
  - Local clone: `<ghq>/github.com/beir-cellar/beir`
  - Applied insight: Standard heterogeneous retrieval benchmark format (queries/qrels/corpus).
  - Source: https://github.com/beir-cellar/beir
- **usnistgov/trec_eval** (C, stars=276)
  - Upstream focus: Evaluation software used in the Text Retrieval Conference
  - Local clone: `<ghq>/github.com/usnistgov/trec_eval`
  - Applied insight: Canonical TREC metric implementation for reproducible Recall/MRR-style gate checks.
  - Source: https://github.com/usnistgov/trec_eval
