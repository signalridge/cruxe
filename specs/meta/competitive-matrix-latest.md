# Competitive Implementation Matrix (2026-02 refresh, universal-first pivot)

> Scope: active OpenSpec changes as of 2026-02-28  
> Goal: map each change to concrete OSS evidence while enforcing Cruxe's universal/low-maintenance direction.

## Matrix

| OpenSpec change | External evidence (repo + file) | Adopt into Cruxe | Do **not** copy directly |
|---|---|---|---|
| `local-cross-encoder-rerank` | `anush008/fastembed-rs` `src/reranking/impl.rs`, `src/models/reranking.rs` | Use fastembed `TextRerank` + fail-soft fallback; keep model configurable | Assume nonexistent `reranking` feature flag; hardcode model cache paths |
| `chunking-quality-contract` | `continuedev/continue` `core/indexing/chunk/chunk.ts` (structured + fallback), semantic systems with overlap chunking | Keep overlap-aware symbol chunking **and** add file-fallback chunking for universality | Depend only on symbol-origin chunks (causes unsupported-language blind spots) |
| `semantic-runtime-governance` (async enrichment pivot) | `zilliztech/claude-context` async indexing workflow docs | Split hot-path indexing and background semantic enrichment with backlog/degraded metadata | Keep embedding generation on synchronous indexing hot path |
| `import-resolution-phase2` (baseline-only pivot) | TS/Pyright resolver complexity evidence + compiler-grade adapter complexity evidence | Build provider interface + generic baseline only in this phase | Add heavy external adapter complexity before baseline quality/metrics justify it |
| `call-graph-ranking-signal` (pivoted to relation graph) | Sourcegraph ranking practice + Google code search query-independent signals | Compute low-cost centrality from resolved relation graph (edge-type agnostic in phase 1) | Couple centrality strictly to call extraction quality per language |
| `universal-ranking-priors` (renamed from per-language-kind-weights) | Zoekt `scoreSymbolKind`, plus maintenance trade-off lessons from large language-specific rule tables | Use role-first universal weights + bounded adaptive repository priors | Expand language-specific weight tables per language in core |
| `result-diversity` | Zoekt `boostNovelExtension` + conservative relevance guard patterns | Keep conservative file-spread rerank with score-floor guard | Aggressive diversity that breaks top relevance |

## Supplemental universality references

| Area | Evidence | Implication for Cruxe |
|---|---|---|
| Compiler-grade resolver complexity | TypeScript `moduleNameResolver.ts`; Pyright `configOptions.ts`; large code footprints in `gopls`/`rust-analyzer` | Avoid reimplementing full language resolvers in Cruxe core |
| Async indexing UX | `zilliztech/claude-context` async indexing docs | Preserve search-during-indexing behavior while adding universal retrieval robustness |

## Update policy

When adding/changing retrieval/ranking/indexing OpenSpec changes:

1. Add one matrix row with at least one file-level OSS evidence pointer.
2. Explicitly mark **adopt** vs **do not copy**.
3. Confirm alignment with universal-first constraints:
   - no mandatory external daemon,
   - no per-language maintenance explosion in core,
   - fail-soft behavior preserved.
