## Context

The ranking system is intentionally additive and explainable. That strength can degrade if new signals are added without explicit bounds and precedence checks. We need a hard contract so future changes remain safe by default.

## Goals / Non-Goals

**Goals:**
1. Define a canonical signal budget schema.
2. Guarantee lexical correctness cannot be overridden by weak secondary signals.
3. Expose explain output that makes clamping transparent.
4. Keep configuration deterministic and validated.

**Non-Goals:**
1. Replacing the current ranking model with opaque ML-only scoring.
2. Per-repo online auto-learning in query path.
3. Breaking existing request/response contracts.

## Decisions

### D1. Three-layer signal accounting
Each signal records:
- `raw_value`
- `clamped_value` (after per-signal bounds)
- `effective_value` (after global precedence gates)

**Why:** debugability and deterministic auditing.

### D2. Global precedence guard
Introduce invariant: if exact lexical match exists, secondary structural signals are capped by a tighter ceiling.

**Why:** preserves user trust in lexical precision.

### D3. Config normalization and lint
On startup/config load, normalize and validate all ranking weights; invalid ranges fall back to canonical safe defaults with warnings.

**Why:** prevent unsafe local configs from silently changing ranking semantics.

### D4. Explain protocol extension
Add additive explain fields under existing explain modes (basic/full), never removing legacy keys.

**Why:** backward compatibility while enabling deeper diagnostics.

## Risks / Trade-offs

- **[Risk] More complex explain payloads** → Mitigation: keep `basic` compact and put full budget info behind `full` mode.
- **[Risk] Existing tuned configs get clamped** → Mitigation: emit explicit normalization warnings and migration notes.
- **[Risk] Hard caps reduce some useful boosts** → Mitigation: make caps explicit and benchmark-gated before release.

## Migration Plan

1. Implement budget schema + clamping in ranking core.
2. Add explain extensions and config lint.
3. Run retrieval-eval-gate benchmarks and tune canonical bounds.
4. Enable strict lint in CI.

Rollback: disable strict lint while retaining runtime clamping.

## Resolved Defaults

1. Budget ranges are intent/plan-aware within globally bounded clamps.
2. MCP budget introspection is deferred; phase 1 exposes applied budgets only in response metadata.

## External References (2026-02-28 Investigation)

Investigated related open-source projects and extracted directly applicable design constraints:

- **quickwit-oss/tantivy** (Rust, stars=14631)
  - Upstream focus: Tantivy is a full-text search engine library inspired by Apache Lucene and written in Rust
  - Local clone: `<ghq>/github.com/quickwit-oss/tantivy`
  - Applied insight: Underlying Rust search engine semantics and explain-style scoring decomposition.
  - Source: https://github.com/quickwit-oss/tantivy
- **sourcegraph/zoekt** (Go, stars=1420)
  - Upstream focus: Fast trigram based code search  
  - Local clone: `<ghq>/github.com/sourcegraph/zoekt`
  - Applied insight: Conservative code-search ranking boosts and anti-overpromotion heuristics.
  - Source: https://github.com/sourcegraph/zoekt
- **typesense/typesense** (C++, stars=25289)
  - Upstream focus: Open Source alternative to Algolia + Pinecone and an Easier-to-Use alternative to ElasticSearch ⚡ 🔍 ✨ Fast, typo tolerant, in-memory fuzzy Search Engine for building delightful search experiences
  - Local clone: `<ghq>/github.com/typesense/typesense`
  - Applied insight: Production-grade bounded ranking knobs and relevance tuning ergonomics.
  - Source: https://github.com/typesense/typesense
