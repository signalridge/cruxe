## Why

Cruxe currently relies on lexical reranking (`LocalRuleReranker`) unless external network rerank providers are configured. This creates two gaps:

- lexical rerank misses semantic matches with low token overlap,
- external rerank dependency is not ideal for local-first/offline workflows.

A local cross-encoder reranker gives language-agnostic semantic ranking improvement without adding per-language rules.

## What Changes

1. **Add local cross-encoder provider**
   - Integrate fastembed `TextRerank` as `provider = cross-encoder`.
   - Keep existing local rule reranker and external providers.

2. **Universal candidate rerank path**
   - Rerank candidate snippets independent of source language.
   - Works with symbol-origin and fallback-origin chunks uniformly.

3. **Fail-soft execution**
   - On model load/inference failure, fallback to local lexical reranker.
   - Never fail the search request solely due to reranker issues.

4. **Benchmark + latency gate**
   - Add explicit quality and latency targets before rollout default changes.

## Capabilities

### New Capabilities
- `local-cross-encoder-rerank`: local semantic reranking in offline/local-first mode.

### Modified Capabilities
- `002-agent-protocol`: `search.semantic.rerank.provider` accepts `cross-encoder`.

## Impact

- Affected crates: `cruxe-query`, `cruxe-core`.
- Dependency impact: add `fastembed` workspace dependency to `cruxe-query` (already present in workspace and `cruxe-state`).
- Runtime impact: first-use model load/download cost; mitigated by lazy load + fallback.
